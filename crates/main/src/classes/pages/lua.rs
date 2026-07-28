use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use archive::{ArchiveExtractor, ArchiveFormat};
use log::*;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};

use crate::{LuaScriptItem, LuaScriptsAdapter, MainWindow};

const UE4SS_DOWNLOAD_URL: &str = "https://host.getaurora.moe/files/addons/lua/main.zip";

static RUNTIME: Lazy<tokio::runtime::Runtime> =
    Lazy::new(|| tokio::runtime::Runtime::new().expect("could not create tokio runtime"));

/// Backend for the Lua Scripts page.
///
/// `LuaScriptsPage`'s script data and callbacks live on the `LuaScriptsAdapter`
/// global (see lua.slint) rather than directly on `MainWindow`, so we reach
/// them via `window.global::<LuaScriptsAdapter>()` instead of `window.*`.
/// This works regardless of where `LuaScriptsPage` ends up instantiated in
/// the component tree.
///
/// Disk layout:
///   `<bin_dir>\ue4ss`                                   <- existence check ({A} vs {B})
///   `<bin_dir>\Lua`                                      <- extraction target for the addon zip
///   `<bin_dir>\Lua\ue4ss\Mods\<name>\Scripts\main.lua`   <- per-script save location
///
/// NOTE: the existence check looks at `<bin_dir>\ue4ss`, but the zip is
/// extracted into `<bin_dir>\Lua` (landing at `<bin_dir>\Lua\ue4ss` once
/// unpacked). Those are two different folders per the spec this was written
/// against — worth double-checking whether that's intentional (e.g.
/// detecting an externally-installed UE4SS vs. the Lua-addon one), since as
/// written a fresh install here won't satisfy the existence check again on
/// next launch.
pub struct LuaScriptsHandler;

impl LuaScriptsHandler {
    /// `bin_dir` is the game's `\Bin` directory. Wire this up to whatever
    /// already resolves the active game/install path elsewhere in the app
    /// (mirrors the `VersionPaths` resolution from the v1.x launcher).
    pub fn setup(window: &Weak<MainWindow>, bin_dir: PathBuf) {
        let win = window.unwrap();
        let adapter = win.global::<LuaScriptsAdapter>();

        // --- {A}/{B} branch --------------------------------------------
        let ue4ss_marker = bin_dir.join("Lua").join("ue4ss").join("UE4SS.dll");
        let already_installed = ue4ss_marker.exists();
        info!(
            "UE4SS marker {} -> ue4ss_installed = {already_installed}",
            ue4ss_marker.display()
        );
        adapter.set_ue4ss_installed(already_installed);

        // Load whatever scripts already exist on disk (if any) instead of
        // hardcoded demo data.
        let mods_dir = Self::mods_dir(&bin_dir);
        let model: Rc<VecModel<LuaScriptItem>> =
            Rc::new(VecModel::from(scan_existing_scripts(&mods_dir)));
        adapter.set_scripts(ModelRc::from(model.clone()));

        let next_id = Rc::new(RefCell::new(
            model
                .iter()
                .filter_map(|i| i.id.parse::<u32>().ok())
                .max()
                .unwrap_or(0)
                + 1,
        ));

        // --- refresh ---------------------------------------------------------
        {
            let model = model.clone();
            let mods_dir = mods_dir.clone();
            let next_id = next_id.clone();
            adapter.on_refresh(move || {
                info!("Refreshing Lua scripts list from disk");
                let fresh = scan_existing_scripts(&mods_dir);

                *next_id.borrow_mut() = fresh
                    .iter()
                    .filter_map(|i| i.id.parse::<u32>().ok())
                    .max()
                    .unwrap_or(0)
                    + 1;

                model.set_vec(fresh);
            });
        }

        // --- install-ue4ss -------------------------------------------------
        {
            let window = window.clone();
            let bin_dir = bin_dir.clone();
            adapter.on_install_ue4ss(move || {
                Self::start_install(window.clone(), bin_dir.clone());
            });
        }

        // --- create-script -------------------------------------------------
        {
            let model = model.clone();
            let next_id = next_id.clone();
            let mods_dir = mods_dir.clone();
            adapter.on_create_script(move || {
                let mut id_ref = next_id.borrow_mut();
                let id = id_ref.to_string();
                *id_ref += 1;

                // Guard against colliding with a reserved built-in helper
                // mod name (e.g. if a future id sequence ever produced
                // "new_script_shared" or similar) — not currently possible
                // given the "new_script_<id>" format, but kept in case this
                // naming scheme changes later.
                let mut name = format!("new_script_{id}");
                if is_blocklisted_name(&name) {
                    name = format!("{name}_script");
                }
                let code = "-- new script\n".to_string();

                info!("Creating new Lua script '{name}' (id {id})");

                if let Err(e) = write_script(&mods_dir, &name, &code) {
                    error!("Failed to write new script '{name}' to disk: {e}");
                }
                register_mod_in_config(&mods_dir, &name, false);

                model.push(LuaScriptItem {
                    id: id.into(),
                    name: name.into(),
                    enabled: false,
                    is_editing: false,
                    code: code.into(),
                });
            });
        }

        // --- toggle-script --------------------------------------------------
        {
            let model = model.clone();
            let mods_dir = mods_dir.clone();
            adapter.on_toggle_script(move |id| {
                if let Some((idx, mut item)) = find_by_id(&model, &id) {
                    item.enabled = !item.enabled;
                    info!("Toggled script {id} -> enabled = {}", item.enabled);
                    set_mod_enabled_in_config(&mods_dir, &item.name, item.enabled);
                    model.set_row_data(idx, item);
                }
            });
        }

        // --- rename-script ---------------------------------------------------
        {
            let model = model.clone();
            let mods_dir = mods_dir.clone();
            let window = window.clone();
            adapter.on_rename_script(move |id, new_name| {
                if is_blocklisted_name(&new_name) {
                    warn!("Rejected rename to blocklisted name '{new_name}'");
                    if let Some(win) = window.upgrade() {
                        win.global::<LuaScriptsAdapter>().set_toast_message(
                            format!("\"{new_name}\" is a reserved name and can't be used.").into(),
                        );
                    }
                    return;
                }

                if let Some((idx, mut item)) = find_by_id(&model, &id) {
                    let old_name = item.name.to_string();
                    info!("Renaming script {id}: '{old_name}' -> '{new_name}'");

                    match rename_script_folder(&mods_dir, &old_name, &new_name) {
                        Ok(()) => {
                            rename_mod_in_config(&mods_dir, &old_name, &new_name);
                        }
                        Err(e) => {
                            error!(
                                "Failed to rename script folder '{old_name}' -> '{new_name}': {e}"
                            );
                        }
                    }

                    item.name = new_name;
                    model.set_row_data(idx, item);
                }
            });
        }

        // --- delete-script ---------------------------------------------------
        {
            let model = model.clone();
            let mods_dir = mods_dir.clone();
            adapter.on_delete_script(move |id| {
                if let Some((idx, item)) = find_by_id(&model, &id) {
                    info!("Deleting script {id}");

                    let script_dir = mods_dir.join(item.name.as_str());
                    if let Err(e) = fs::remove_dir_all(&script_dir) {
                        if e.kind() != std::io::ErrorKind::NotFound {
                            error!(
                                "Failed to delete script folder '{}': {e}",
                                script_dir.display()
                            );
                        }
                    }
                    unregister_mod_from_config(&mods_dir, &item.name);

                    model.remove(idx);
                }
            });
        }

        // --- save-script-code -------------------------------------------------
        {
            let model = model.clone();
            let mods_dir = mods_dir.clone();
            adapter.on_save_script_code(move |id, code| {
                if let Some((idx, mut item)) = find_by_id(&model, &id) {
                    info!("Saving code for script {id} ({} bytes)", code.len());

                    if let Err(e) = write_script(&mods_dir, &item.name, &code) {
                        error!("Failed to save script '{}' to disk: {e}", item.name);
                    }

                    item.code = code;
                    model.set_row_data(idx, item);
                }
            });
        }
    }

    fn mods_dir(bin_dir: &Path) -> PathBuf {
        bin_dir.join("Lua").join("ue4ss").join("Mods")
    }

    /// Kicks off the download + extract on the shared tokio runtime so the
    /// UI thread never blocks on network/disk IO. Progress is reported back
    /// via `slint::invoke_from_event_loop`.
    fn start_install(window: Weak<MainWindow>, bin_dir: PathBuf) {
        RUNTIME.spawn(async move {
            let result = Self::download_and_extract(&bin_dir, &window).await;

            let window = window.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(win) = window.upgrade() else {
                    return;
                };
                let adapter = win.global::<LuaScriptsAdapter>();

                match result {
                    Ok(()) => {
                        adapter.set_installing(false);
                        adapter.set_install_progress(100);
                        adapter.set_install_error("".into());
                        adapter.set_ue4ss_installed(true);
                    }
                    Err(e) => {
                        error!("UE4SS install failed: {e}");
                        adapter.set_installing(false);
                        adapter.set_install_error(e.to_string().into());
                    }
                }
            });
        });
    }

    async fn download_and_extract(
        bin_dir: &Path,
        window: &Weak<MainWindow>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Self::report(window, true, 0, "Downloading UE4SS…");

        let resp = reqwest::get(UE4SS_DOWNLOAD_URL).await?.error_for_status()?;
        let bytes = resp.bytes().await?;

        Self::report(window, true, 50, "Extracting UE4SS…");

        let lua_dir = bin_dir.join("Lua");
        tokio::fs::create_dir_all(&lua_dir).await?;

        let extractor = ArchiveExtractor::new();
        let files = extractor.extract(&bytes, ArchiveFormat::Zip)?;
        let count = files.len().max(1);

        for (i, entry) in files.iter().enumerate() {
            let Some(relative_path) = sanitize_archive_path(&entry.path) else {
                // Skip anything with a path that could escape lua_dir
                // (zip-slip protection).
                warn!("Skipping unsafe archive entry: {:?}", entry.path);
                continue;
            };
            let out_path = lua_dir.join(relative_path);

            if entry.is_directory {
                tokio::fs::create_dir_all(&out_path).await?;
            } else {
                if let Some(parent) = out_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(&out_path, &entry.data).await?;
            }

            let pct = 50 + ((i + 1) * 50 / count) as i32;
            Self::report(window, true, pct.clamp(0, 100), "Extracting UE4SS…");
        }

        Ok(())
    }

    fn report(window: &Weak<MainWindow>, installing: bool, progress: i32, status: &str) {
        let window = window.clone();
        let status = status.to_string();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(win) = window.upgrade() {
                let adapter = win.global::<LuaScriptsAdapter>();
                adapter.set_installing(installing);
                adapter.set_install_progress(progress);
                adapter.set_install_status(status.into());
            }
        });
    }
}

/// Validates that an archive entry's path is relative and doesn't try to
/// escape the extraction directory (no `..` components, no absolute paths).
/// Mirrors the protection `zip::enclosed_name()` used to give us for free.
fn sanitize_archive_path(path: &str) -> Option<PathBuf> {
    use std::path::Component;

    let path = Path::new(path);
    let mut safe = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    Some(safe)
}

/// Finds a `LuaScriptItem` by id in the model, returning its row index
/// alongside a cloned copy so callers can mutate + `set_row_data` it back.
fn find_by_id(
    model: &Rc<VecModel<LuaScriptItem>>,
    id: &str,
) -> Option<(usize, LuaScriptItem)> {
    model.iter().enumerate().find(|(_, item)| item.id == id)
}

/// Writes `code` to `<mods_dir>\<name>\Scripts\main.lua`, creating any
/// missing directories along the way.
fn write_script(mods_dir: &Path, name: &str, code: &str) -> std::io::Result<()> {
    let scripts_dir = mods_dir.join(name).join("Scripts");
    fs::create_dir_all(&scripts_dir)?;
    fs::write(scripts_dir.join("main.lua"), code)
}

/// Renames `<mods_dir>\<old_name>` to `<mods_dir>\<new_name>` (moving the
/// `Scripts\main.lua` inside it along for the ride). No-ops if the old
/// folder doesn't exist yet (e.g. a script that was never saved).
fn rename_script_folder(mods_dir: &Path, old_name: &str, new_name: &str) -> std::io::Result<()> {
    let old_dir = mods_dir.join(old_name);
    if !old_dir.exists() {
        return Ok(());
    }
    fs::rename(old_dir, mods_dir.join(new_name))
}

/// Built-in helper mod folders (`shared`, `Keybinds`) that ship alongside
/// the UE4SS addon zip. These aren't user scripts, so they're:
///   1. Refused as a rename target (`on_rename_script` below rejects the
///      rename and surfaces a toast instead of touching disk/model state).
///   2. Skipped entirely when scanning `<mods_dir>\*` for the script list
///      (see `scan_existing_scripts`).
///   3. Always kept at the bottom of `mods.txt` (see `write_mods_txt`).
/// Matched case-insensitively since folder casing isn't guaranteed to stay
/// consistent across zip repackagings.
const BLOCKLISTED_NAMES: &[&str] = &["shared", "keybinds"];

fn is_blocklisted_name(name: &str) -> bool {
    BLOCKLISTED_NAMES
        .iter()
        .any(|blocked| blocked.eq_ignore_ascii_case(name))
}

/// `mods.txt` doesn't support spaces in mod identifiers (e.g. "Line Trace
/// Mod" needs to become "LineTraceMod"), and `mods.json`'s `mod_name` has
/// to match whatever identifier `mods.txt` uses for the two files to stay
/// in sync — so this same sanitizer is used for both. Strips all
/// whitespace, not just plain spaces, to be safe against tabs or stray
/// characters from copy-pasted names.
fn sanitize_mod_config_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_whitespace()).collect()
}

/// A single entry in `mods.json` — UE4SS's record of which mod folders
/// exist and whether each is registered/enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModJsonEntry {
    mod_name: String,
    mod_enabled: bool,
}

fn mods_json_path(mods_dir: &Path) -> PathBuf {
    mods_dir.join("mods.json")
}

fn mods_txt_path(mods_dir: &Path) -> PathBuf {
    mods_dir.join("mods.txt")
}

/// Reads `mods.json`. Returns an empty list if the file doesn't exist yet
/// or fails to parse (logged), rather than erroring out — a missing/blank
/// mods.json shouldn't block the rest of the app from working.
fn read_mods_json(mods_dir: &Path) -> Vec<ModJsonEntry> {
    let path = mods_json_path(mods_dir);
    let Ok(contents) = fs::read_to_string(&path) else {
        return Vec::new();
    };

    match serde_json::from_str(&contents) {
        Ok(entries) => entries,
        Err(e) => {
            error!("Failed to parse {}: {e}", path.display());
            Vec::new()
        }
    }
}

fn write_mods_json(mods_dir: &Path, entries: &[ModJsonEntry]) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(entries)
        .unwrap_or_else(|_| "[]".to_string());
    fs::write(mods_json_path(mods_dir), json)
}

/// Reads `mods.txt`, returning `(name, enabled)` pairs in on-disk order.
/// Lines are formatted `Name : 1` / `Name : 0`; blank or malformed lines
/// are skipped.
fn read_mods_txt(mods_dir: &Path) -> Vec<(String, bool)> {
    let path = mods_txt_path(mods_dir);
    let Ok(contents) = fs::read_to_string(&path) else {
        return Vec::new();
    };

    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (name, flag) = line.split_once(':')?;
            Some((name.trim().to_string(), flag.trim() == "1"))
        })
        .collect()
}

/// Writes `mods.txt`, enforcing the invariant that built-in mods
/// (`BLOCKLISTED_NAMES`) always sit at the bottom of the file — UE4SS
/// breaks otherwise. Non-built-in entries are written first, in whatever
/// order they're passed in (callers prepend newly-created mods so they
/// land at the very top), followed by built-in entries in their existing
/// relative order.
fn write_mods_txt(mods_dir: &Path, entries: &[(String, bool)]) -> std::io::Result<()> {
    let (builtin, user): (Vec<_>, Vec<_>) = entries
        .iter()
        .cloned()
        .partition(|(name, _)| is_blocklisted_name(name));

    let lines: Vec<String> = user
        .into_iter()
        .chain(builtin)
        .map(|(name, enabled)| format!("{name} : {}", if enabled { 1 } else { 0 }))
        .collect();

    fs::write(mods_txt_path(mods_dir), lines.join("\r\n"))
}

/// Registers a newly-created mod in `mods.json` and `mods.txt`, inserting
/// it at the very top of `mods.txt` (ahead of every existing mod, but
/// still above the built-in block per `write_mods_txt`'s invariant).
fn register_mod_in_config(mods_dir: &Path, name: &str, enabled: bool) {
    let config_name = sanitize_mod_config_name(name);

    let mut json_entries = read_mods_json(mods_dir);
    json_entries.retain(|e| e.mod_name != config_name);
    json_entries.push(ModJsonEntry {
        mod_name: config_name.clone(),
        mod_enabled: enabled,
    });
    if let Err(e) = write_mods_json(mods_dir, &json_entries) {
        error!("Failed to write mods.json: {e}");
    }

    let mut txt_entries = read_mods_txt(mods_dir);
    txt_entries.retain(|(n, _)| *n != config_name);
    txt_entries.insert(0, (config_name, enabled));
    if let Err(e) = write_mods_txt(mods_dir, &txt_entries) {
        error!("Failed to write mods.txt: {e}");
    }
}

/// Removes a mod's entry from `mods.json` and `mods.txt`.
fn unregister_mod_from_config(mods_dir: &Path, name: &str) {
    let config_name = sanitize_mod_config_name(name);

    let mut json_entries = read_mods_json(mods_dir);
    json_entries.retain(|e| e.mod_name != config_name);
    if let Err(e) = write_mods_json(mods_dir, &json_entries) {
        error!("Failed to write mods.json: {e}");
    }

    let mut txt_entries = read_mods_txt(mods_dir);
    txt_entries.retain(|(n, _)| *n != config_name);
    if let Err(e) = write_mods_txt(mods_dir, &txt_entries) {
        error!("Failed to write mods.txt: {e}");
    }
}

/// Renames a mod's entry in `mods.json`/`mods.txt` in place — position in
/// `mods.txt` (and therefore the built-in-mods-at-bottom invariant) is
/// preserved, only the identifier text changes.
fn rename_mod_in_config(mods_dir: &Path, old_name: &str, new_name: &str) {
    let old_config = sanitize_mod_config_name(old_name);
    let new_config = sanitize_mod_config_name(new_name);

    let mut json_entries = read_mods_json(mods_dir);
    for entry in json_entries.iter_mut() {
        if entry.mod_name == old_config {
            entry.mod_name = new_config.clone();
        }
    }
    if let Err(e) = write_mods_json(mods_dir, &json_entries) {
        error!("Failed to write mods.json: {e}");
    }

    let mut txt_entries = read_mods_txt(mods_dir);
    for (n, _) in txt_entries.iter_mut() {
        if *n == old_config {
            *n = new_config.clone();
        }
    }
    if let Err(e) = write_mods_txt(mods_dir, &txt_entries) {
        error!("Failed to write mods.txt: {e}");
    }
}

/// Updates a mod's enabled flag in both `mods.json` and `mods.txt` in
/// place, without touching ordering. If the mod has no existing entry
/// (shouldn't normally happen), it's added at the top of `mods.txt`.
fn set_mod_enabled_in_config(mods_dir: &Path, name: &str, enabled: bool) {
    let config_name = sanitize_mod_config_name(name);

    let mut json_entries = read_mods_json(mods_dir);
    if let Some(entry) = json_entries.iter_mut().find(|e| e.mod_name == config_name) {
        entry.mod_enabled = enabled;
    } else {
        json_entries.push(ModJsonEntry {
            mod_name: config_name.clone(),
            mod_enabled: enabled,
        });
    }
    if let Err(e) = write_mods_json(mods_dir, &json_entries) {
        error!("Failed to write mods.json: {e}");
    }

    let mut txt_entries = read_mods_txt(mods_dir);
    if let Some(entry) = txt_entries.iter_mut().find(|(n, _)| *n == config_name) {
        entry.1 = enabled;
    } else {
        txt_entries.insert(0, (config_name, enabled));
    }
    if let Err(e) = write_mods_txt(mods_dir, &txt_entries) {
        error!("Failed to write mods.txt: {e}");
    }
}

/// Scans `<mods_dir>\*` for existing mod folders and loads their
/// `Scripts\main.lua` contents, so a previously-installed UE4SS with
/// existing Lua mods shows up instead of an empty list.
///
/// Skips the built-in helper folders (see `BLOCKLISTED_NAMES`) so they
/// don't show up as editable "scripts" in the UI. Each script's `enabled`
/// state is read from `mods.txt` (matched via the same sanitized
/// identifier used when writing it), not assumed — a mod not listed in
/// `mods.txt` at all is treated as disabled.
fn scan_existing_scripts(mods_dir: &Path) -> Vec<LuaScriptItem> {
    let mut scripts = Vec::new();

    let Ok(entries) = fs::read_dir(mods_dir) else {
        return scripts;
    };

    let enabled_map: HashMap<String, bool> = read_mods_txt(mods_dir).into_iter().collect();

    let mut next_id = 1u32;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_blocklisted_name(&name) {
            continue;
        }

        let enabled = enabled_map
            .get(&sanitize_mod_config_name(&name))
            .copied()
            .unwrap_or(false);

        let main_lua = entry.path().join("Scripts").join("main.lua");
        let code = fs::read_to_string(&main_lua).unwrap_or_default();

        scripts.push(LuaScriptItem {
            id: next_id.to_string().into(),
            name: name.into(),
            enabled,
            is_editing: false,
            code: code.into(),
        });
        next_id += 1;
    }

    scripts
}