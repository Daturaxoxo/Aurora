use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{Result, anyhow};
use archive::{ArchiveExtractor, ArchiveFormat};
use log::*;
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};

use crate::bridge::PopupSpec;
use crate::{LuaScriptItem, LuaScriptsAdapter, MainWindow};

const UE4SS_DOWNLOAD_URL: &str = "https://host.getaurora.moe/files/addons/lua/core.zip";
const BLOCKLISTED_NAMES: &[&str] = &["shared", "keybinds"];
const INVALID_NAME_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const RESERVED_DEVICE_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

static PENDING_DELETE: std::sync::Mutex<Option<usize>> = std::sync::Mutex::new(None);
static MODS_DIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub struct LuaScriptsHandler;

impl LuaScriptsHandler {
    pub fn setup(window: &Weak<MainWindow>, bin_dir: &Path) {
        let win = window.unwrap();
        let adapter = win.global::<LuaScriptsAdapter>();

        // [A]/[B] branch switch
        let ue4ss_marker = bin_dir.join("Lua").join("ue4ss").join("UE4SS.dll");
        let already_installed = ue4ss_marker.exists();
        info!(
            "UE4SS marker {} -> ue4ss_installed = {already_installed}",
            ue4ss_marker.display()
        );
        adapter.set_ue4ss_installed(already_installed);
        adapter.set_debug_enabled(read_debug_enabled(bin_dir));
        {
            let window = window.clone();
            let bin_dir = bin_dir.to_path_buf();
            adapter.on_toggle_debug(move || {
                let Some(win) = window.upgrade() else {
                    return;
                };
                let adapter = win.global::<LuaScriptsAdapter>();
                let enabled = !adapter.get_debug_enabled();
                if let Err(e) = set_debug_enabled(&bin_dir, enabled) {
                    adapter.set_toast_message(
                        format!("Couldn't change the debugging setting: {e}").into(),
                    );
                    return;
                }
                adapter.set_debug_enabled(enabled);
            });
        }

        // Load scripts from disk
        let mods_dir = Self::mods_dir(bin_dir);
        *MODS_DIR.lock().unwrap() = Some(mods_dir.clone());
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

        // Refresh
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

        // Install Handler
        {
            let window = window.clone();
            let bin_dir = bin_dir.to_path_buf();
            adapter.on_install_ue4ss(move || {
                Self::start_install(window.clone(), bin_dir.clone());
            });
        }

        // Create Script
        {
            let model = model.clone();
            let mods_dir = mods_dir.clone();
            let window = window.clone();
            adapter.on_create_script(move || {
                let name = next_available_script_name(&mods_dir);
                let code = "-- Aurora uses UE4SS' LUA API to load scripts, for more information see their documentation.\n".to_string();

                if let Err(e) = create_script(&mods_dir, &name, &code) {
                    error!("Failed to write new script '{name}' to disk: {e}");
                    if let Some(win) = window.upgrade() {
                        win.global::<LuaScriptsAdapter>().set_toast_message(
                            format!("Couldn't create \"{name}\": {e}").into(),
                        );
                    }
                    return;
                }
                register_mod_in_config(&mods_dir, &name, false);

                let id = {
                    let mut id_ref = next_id.borrow_mut();
                    let id = id_ref.to_string();
                    *id_ref += 1;
                    id
                };

                model.push(LuaScriptItem {
                    id: id.into(),
                    name: name.into(),
                    enabled: false,
                    is_editing: false,
                    code: code.into(),
                });
            });
        }

        // Toggle
        {
            let model = model.clone();
            let mods_dir = mods_dir.clone();
            adapter.on_toggle_script(move |id| {
                if let Some((idx, mut item)) = find_by_id(&model, &id) {
                    item.enabled = !item.enabled;
                    set_mod_enabled_in_config(&mods_dir, &item.name, item.enabled);
                    model.set_row_data(idx, item);
                }
            });
        }

        // -Rename
        {
            let model = model.clone();
            let mods_dir = mods_dir.clone();
            let window = window.clone();
            adapter.on_rename_script(move |id, new_name| {
                if let Err(message) = validate_script_name(&new_name) {
                    warn!("Rejected rename to invalid name '{new_name}': {message}");
                    if let Some(win) = window.upgrade() {
                        win.global::<LuaScriptsAdapter>()
                            .set_toast_message(message.into());
                    }
                    return;
                }

                if let Some((idx, mut item)) = find_by_id(&model, &id) {
                    let old_name = item.name.to_string();

                    if let Err(e) = rename_script_folder(&mods_dir, &old_name, &new_name) {
                        error!(
                            "Failed to rename script folder from '{old_name}' to '{new_name}': {e}"
                        );
                        if let Some(win) = window.upgrade() {
                            win.global::<LuaScriptsAdapter>().set_toast_message(
                                format!("Couldn't rename \"{old_name}\" to \"{new_name}\": {e}")
                                    .into(),
                            );
                        }
                        return;
                    }

                    rename_mod_in_config(&mods_dir, &old_name, &new_name);

                    item.name = new_name;
                    model.set_row_data(idx, item);
                }
            });
        }

        // Delete Script
        {
            let window = window.clone();
            let model = model.clone();
            adapter.on_delete_script(move |id| {
                let Some(win) = window.upgrade() else { return };

                if let Some((idx, item)) = find_by_id(&model, &id) {
                    *PENDING_DELETE.lock().unwrap() = Some(idx);

                    PopupSpec {
                        id: "lua-delete".to_owned(),
                        kind: "danger".to_owned(),
                        title: "Delete script?".to_owned(),
                        message: "This script will be permanently deleted. You cannot undo \
                                  this action."
                            .to_owned(),
                        subject: item.name.to_string(),
                        ..PopupSpec::default()
                    }
                    .apply(&win);
                }
            });
        }

        // Save Code
        {
            adapter.on_save_script_code(move |id, code| {
                if let Some((idx, mut item)) = find_by_id(&model, &id) {
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

    /// UE4SS Install Main
    fn start_install(window: Weak<MainWindow>, bin_dir: PathBuf) {
        crate::classes::pages::gbbrowser::runtime().spawn(async move {
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

    pub fn confirm_delete(window: &Weak<MainWindow>) {
        let Some(i) = PENDING_DELETE.lock().unwrap().take() else {
            return;
        };

        let mods_dir_guard = MODS_DIR.lock().unwrap();
        let Some(mods_dir) = mods_dir_guard.as_ref() else {
            return;
        };

        let Some(win) = window.upgrade() else { return };
        let adapter = win.global::<LuaScriptsAdapter>();
        let model = adapter.get_scripts();

        let Some(item) = model.row_data(i) else {
            return;
        };

        let script_dir = mods_dir.join(item.name.as_str());

        if let Err(e) = fs::remove_dir_all(&script_dir)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            error!(
                "Failed to delete script folder '{}': {e}",
                script_dir.display()
            );
        }

        unregister_mod_from_config(mods_dir, &item.name);
        drop(mods_dir_guard);

        if let Some(vec_model) = model.as_any().downcast_ref::<VecModel<LuaScriptItem>>() {
            vec_model.remove(i);
        }
    }

    async fn download_and_extract(bin_dir: &Path, window: &Weak<MainWindow>) -> Result<()> {
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

            let pct = 50 + i32::try_from((i + 1) * 50 / count).unwrap_or(50);
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

fn find_by_id(model: &Rc<VecModel<LuaScriptItem>>, id: &str) -> Option<(usize, LuaScriptItem)> {
    model.iter().enumerate().find(|(_, item)| item.id == id)
}

fn write_script(mods_dir: &Path, name: &str, code: &str) -> Result<()> {
    let scripts_dir = mods_dir.join(name).join("Scripts");
    fs::create_dir_all(&scripts_dir)?;
    fs::write(scripts_dir.join("main.lua"), code).map_err(|e| anyhow!(e))
}

fn create_script(mods_dir: &Path, name: &str, code: &str) -> Result<()> {
    let script_dir = mods_dir.join(name);
    if script_dir.exists() {
        return Err(anyhow::anyhow!(
            "a script folder named '{name}' already exists"
        ));
    }

    let lua_dir = script_dir.join("Scripts");
    fs::create_dir_all(&lua_dir)?;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lua_dir.join("main.lua"))?;
    file.write_all(code.as_bytes()).map_err(|e| anyhow!(e))
}

fn next_available_script_name(mods_dir: &Path) -> String {
    let mut index = 1u32;
    loop {
        let mut name = format!("lua_script_{index}");
        if is_blocklisted_name(&name) {
            name = format!("{name}_script");
        }

        if !mods_dir.join(&name).exists() {
            return name;
        }

        index += 1;
    }
}

fn rename_script_folder(mods_dir: &Path, old_name: &str, new_name: &str) -> Result<()> {
    if old_name == new_name {
        return Ok(());
    }

    let old_dir = mods_dir.join(old_name);
    let new_dir = mods_dir.join(new_name);

    if !old_name.eq_ignore_ascii_case(new_name) && new_dir.exists() {
        return Err(anyhow!("a script folder named '{new_name}' already exists"));
    }

    if !old_dir.exists() {
        return Ok(());
    }

    fs::rename(old_dir, new_dir).map_err(|e| anyhow!(e))
}

fn validate_script_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("A script name can't be empty.".to_string());
    }

    if name != name.trim() {
        return Err(format!("\"{name}\" can't start or end with spaces."));
    }

    if is_blocklisted_name(name) {
        return Err(format!("\"{name}\" is a reserved name and can't be used."));
    }

    if name
        .chars()
        .any(|c| INVALID_NAME_CHARS.contains(&c) || c.is_control())
    {
        return Err(format!(
            "\"{name}\" contains characters that can't be used in a script name."
        ));
    }

    if name.ends_with('.') || name == "." || name == ".." {
        return Err(format!("\"{name}\" isn't a valid script name."));
    }

    let stem = name.split('.').next().unwrap_or(name);
    if RESERVED_DEVICE_NAMES
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(stem))
    {
        return Err(format!("\"{name}\" is a reserved name and can't be used."));
    }

    let mut components = Path::new(name).components();
    let single_normal = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if !single_normal {
        return Err(format!("\"{name}\" isn't a valid script name."));
    }

    Ok(())
}

fn is_blocklisted_name(name: &str) -> bool {
    BLOCKLISTED_NAMES
        .iter()
        .any(|blocked| blocked.eq_ignore_ascii_case(name))
}

fn sanitize_mod_config_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_whitespace()).collect()
}

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

fn settings_ini_path(bin_dir: &Path) -> PathBuf {
    bin_dir.join("Lua").join("ue4ss").join("UE4SS-settings.ini")
}

fn read_debug_enabled(bin_dir: &Path) -> bool {
    let path = settings_ini_path(bin_dir);
    let Ok(contents) = fs::read_to_string(&path) else {
        return false;
    };

    let mut in_debug_section = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_debug_section = trimmed.eq_ignore_ascii_case("[Debug]");
            continue;
        }
        if !in_debug_section {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=')
            && key.trim().eq_ignore_ascii_case("GuiConsoleEnabled")
        {
            return value.trim() == "1";
        }
    }

    false
}

fn set_debug_enabled(bin_dir: &Path, enabled: bool) -> Result<()> {
    let path = settings_ini_path(bin_dir);
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) => {
            error!("Could not read {} to toggle debugging: {e}", path.display());
            return Err(anyhow!(e));
        }
    };

    let line_ending = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let value = if enabled { "1" } else { "0" };

    let mut updated = String::with_capacity(contents.len());
    let mut debug_section = false;

    for line in contents.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            debug_section = trimmed.eq_ignore_ascii_case("[Debug]");
            updated.push_str(line);
            updated.push_str(line_ending);
            continue;
        }

        if debug_section && let Some((key, _)) = trimmed.split_once('=') {
            let key_name = key.trim();
            if key_name.eq_ignore_ascii_case("GuiConsoleEnabled")
                || key_name.eq_ignore_ascii_case("GuiConsoleVisible")
            {
                let _ = write!(updated, "{key_name} = {value}");
                updated.push_str(line_ending);
                continue;
            }
        }

        updated.push_str(line);
        updated.push_str(line_ending);
    }

    if let Err(e) = fs::write(&path, updated) {
        error!("Failed to write {}: {e}", path.display());
        return Err(anyhow!(e));
    }

    Ok(())
}

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
    let json = serde_json::to_string_pretty(entries).unwrap_or_else(|_| "[]".to_string());
    fs::write(mods_json_path(mods_dir), json)
}

fn read_mods_txt(mods_dir: &Path) -> Vec<(String, bool)> {
    let path = mods_txt_path(mods_dir);
    let Ok(contents) = fs::read_to_string(&path) else {
        return Vec::new();
    };

    let parsed: Vec<(String, bool)> = contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (name, flag) = line.split_once(':')?;
            Some((name.trim().to_string(), flag.trim() == "1"))
        })
        .collect();

    let final_values: HashMap<String, bool> = parsed
        .iter()
        .map(|(name, enabled)| (name.clone(), *enabled))
        .collect();

    let mut seen = std::collections::HashSet::new();
    parsed
        .into_iter()
        .filter(|(name, _)| seen.insert(name.clone()))
        .map(|(name, _)| {
            let enabled = final_values[&name];
            (name, enabled)
        })
        .collect()
}

fn write_mods_txt(mods_dir: &Path, entries: &[(String, bool)]) -> std::io::Result<()> {
    let final_values: HashMap<&str, bool> = entries
        .iter()
        .map(|(name, enabled)| (name.as_str(), *enabled))
        .collect();

    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<(String, bool)> = entries
        .iter()
        .filter(|(name, _)| seen.insert(name.clone()))
        .map(|(name, _)| (name.clone(), final_values[name.as_str()]))
        .collect();

    let (builtin, user): (Vec<_>, Vec<_>) = deduped
        .into_iter()
        .partition(|(name, _)| is_blocklisted_name(name));

    let lines: Vec<String> = user
        .into_iter()
        .chain(builtin)
        .map(|(name, enabled)| format!("{name} : {}", i32::from(enabled)))
        .collect();

    fs::write(mods_txt_path(mods_dir), lines.join("\n"))
}

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

fn rename_mod_in_config(mods_dir: &Path, old_name: &str, new_name: &str) {
    let old_config = sanitize_mod_config_name(old_name);
    let new_config = sanitize_mod_config_name(new_name);

    let mut json_entries = read_mods_json(mods_dir);
    for entry in &mut json_entries {
        if entry.mod_name == old_config {
            entry.mod_name.clone_from(&new_config);
        }
    }
    if let Err(e) = write_mods_json(mods_dir, &json_entries) {
        error!("Failed to write mods.json: {e}");
    }

    let mut txt_entries = read_mods_txt(mods_dir);
    for (n, _) in &mut txt_entries {
        if *n == old_config {
            n.clone_from(&new_config);
        }
    }
    if let Err(e) = write_mods_txt(mods_dir, &txt_entries) {
        error!("Failed to write mods.txt: {e}");
    }
}

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
