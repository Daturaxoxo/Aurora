use super::INVALID_FILENAME_CHARS;
use super::gbbrowser;
use crate::classes::{characters, modicons};
use crate::{
    FilterOption, GroupOption, IconChoice, MainWindow, ModFilters, ModItem, ModStatusFilter, ModTag,
};

use anyhow::{Context, Result, anyhow};
use backend::handler::GAME_RUNNING;
use log::*;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shared::archive::{ARCHIVE_EXTENSIONS, extract_archive_with_progress};
use shared::classes::gamebanana::types::NteModFile;
use shared::config::{self, key};
use shared::utils::{get_cache_dir, get_mods_path, open_folder, read_dir_recursive};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

// prefixes & shit, done so we have an easy job later on when adding new game support (even tho we will probably have to change these to arrays later on...) -datura
const GROUP_PREFIX: &str = "AU GRP - ";
const MOD_EXTENSIONS: [&str; 3] = ["pak", "utoc", "ucas"];
const TOGGLE_EXTENSION: &str = "pak";

static TOGGLE_LOCK: Mutex<()> = Mutex::new(());
const DISABLED_SUFFIX: &str = ".disabled";
const STAGING_PREFIX: &str = ".aurora-installing-";
const ALREADY_INSTALLED: &str = "a mod with this name already exists";
const SOURCE_FILE: &str = ".aurora-source.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModSource {
    pub mod_id: u32,
    pub file_id: u32,
    pub file_name: String,
    pub md5: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub name: String,
}

impl ModSource {
    fn read(folder: &Path) -> Option<Self> {
        let raw = std::fs::read_to_string(folder.join(SOURCE_FILE)).ok()?;
        serde_json::from_str(&raw)
            .map_err(|e| warn!("'{}' has an unreadable source file: {e}", folder.display()))
            .ok()
    }

    fn write(&self, folder: &Path) {
        let raw = match serde_json::to_string(self) {
            Ok(raw) => raw,
            Err(e) => {
                warn!(
                    "could not serialize the source of '{}': {e}",
                    folder.display()
                );
                return;
            }
        };
        // Removing first breaks any hard link, so sibling instances of this
        // mod don't get the new source file written through their links
        let target = folder.join(SOURCE_FILE);
        let _ = std::fs::remove_file(&target);
        if let Err(e) = std::fs::write(&target, raw) {
            warn!("could not record the source of '{}': {e}", folder.display());
        }
    }
}

fn is_mod_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| MOD_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn is_disabled_mod_file(name: &str) -> bool {
    name.strip_suffix(DISABLED_SUFFIX).is_some_and(is_mod_file)
}

fn is_pak_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(TOGGLE_EXTENSION))
}

pub type InstallDoneCallback = Box<dyn FnOnce(&MainWindow) + Send>;

#[derive(Debug, Clone)]
pub struct Group {
    pub name: Option<String>,
    pub path: Option<PathBuf>,
    pub mods: Vec<Mod>,
}

impl Group {
    pub const fn new(name: Option<String>, path: Option<PathBuf>) -> Self {
        Self {
            name,
            path,
            mods: vec![],
        }
    }

    pub fn add_mod(&mut self, mod_: Mod) {
        self.mods.push(mod_);
    }
}

#[derive(Debug, Clone)]
pub struct Mod {
    pub folder_name: String,
    pub display_name: String,
    pub path: PathBuf,
    #[allow(dead_code)]
    pub group: Option<Group>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub support_link: Option<String>,
    pub icon: Option<String>,
    pub image_url: Option<String>,
    pub has_icon_png: bool,
    pub is_enabled: bool,
    pub has_json: bool,
    pub source: Option<ModSource>,
}

impl Default for Mod {
    fn default() -> Self {
        Self {
            folder_name: String::new(),
            display_name: String::new(),
            path: PathBuf::new(),
            group: None,
            version: Some("Unknown".to_string()),
            author: Some("Unknown".to_string()),
            support_link: None,
            icon: None,
            image_url: None,
            has_icon_png: false,
            is_enabled: false,
            has_json: false,
            source: None,
        }
    }
}

pub struct ModManager;

impl ModManager {
    fn find_mod_json(folder: &Path) -> Option<PathBuf> {
        let json_path = folder.join("mod.json");
        if json_path.exists() {
            return Some(json_path);
        }

        let entries = folder
            .read_dir()
            .map_err(|e| warn!("could not read '{}': {e}", folder.display()))
            .ok()?;

        for sub in entries.flatten() {
            let nested = sub.path().join("mod.json");
            if sub.file_type().is_ok_and(|t| t.is_dir()) && nested.exists() {
                return Some(nested);
            }
        }

        None
    }

    fn get_mod_data(folder: &PathBuf) -> Mod {
        let mod_name = folder
            .file_name()
            .unwrap_or(folder.as_os_str())
            .to_string_lossy()
            .into_owned();

        let files = read_dir_recursive(folder);
        let is_enabled = !files
            .iter()
            .any(|p| is_disabled_mod_file(&p.file_name().to_string_lossy()));

        let mut mod_data = Mod {
            folder_name: mod_name.clone(),
            display_name: mod_name.strip_suffix("_P").unwrap_or(&mod_name).to_string(),
            path: folder.clone(),
            is_enabled,
            has_icon_png: folder.join("icon.png").is_file(),
            source: ModSource::read(folder),
            ..Default::default()
        };

        if let Some(author) = mod_data
            .source
            .as_ref()
            .map(|source| source.author.clone())
            .filter(|author| !author.is_empty())
        {
            mod_data.author = Some(author);
        }

        let Some(json_path) = Self::find_mod_json(folder) else {
            return mod_data;
        };

        let raw = match std::fs::read_to_string(&json_path) {
            Ok(raw) => raw,
            Err(e) => {
                warn!(
                    "could not read '{}' ({e}); falling back to the folder name",
                    json_path.display()
                );
                return mod_data;
            }
        };

        let json: serde_json::Value = match serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        {
            Ok(json) => json,
            Err(e) => {
                warn!(
                    "'{}' is not valid JSON ({e}); falling back to the folder name",
                    json_path.display()
                );
                return mod_data;
            }
        };
        mod_data.has_json = true;

        let binding = serde_json::Map::new();
        let root = json.as_object().unwrap_or(&binding);

        // Case insensitive field lookup
        let field = |name: &str| {
            root.iter()
                .find(|(k, _)| k.to_lowercase() == name)
                .map(|(_, v)| v)
        };

        let optionals = field("optionals")
            .and_then(Value::as_object)
            .unwrap_or(&binding);

        let optional = |name: &str| {
            optionals
                .iter()
                .find(|(k, _)| k.to_lowercase() == name)
                .and_then(|(_, v)| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string)
        };

        let with_scheme = |value: String| {
            if value.starts_with("http://") || value.starts_with("https://") {
                value
            } else {
                format!("https://{value}")
            }
        };

        let support_link = optional("support link").map(&with_scheme);
        let image_url = optional("custom image url").map(with_scheme);

        let icon = field("icon")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|icon| !icon.is_empty())
            .map(ToString::to_string);

        let display_name = field("name")
            .and_then(Value::as_str)
            .unwrap_or(&mod_data.display_name)
            .to_string();

        Mod {
            display_name,
            version: field("version")
                .and_then(Value::as_str)
                .or(Some("1.0.0"))
                .map(ToString::to_string),
            author: field("author")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| mod_data.author.clone()),
            support_link,
            icon,
            image_url,
            ..mod_data
        }
    }

    fn contains_pak(folder: &PathBuf) -> bool {
        read_dir_recursive(folder).iter().any(|item| {
            let name = item.file_name().to_string_lossy().into_owned();
            is_mod_file(&name) || is_disabled_mod_file(&name)
        })
    }

    pub fn scan_mods() -> Result<Vec<Group>> {
        let mods_path =
            get_mods_path().ok_or_else(|| anyhow!("the game folder is not set in the settings"))?;

        if !mods_path.exists() {
            info!("mods folder '{}' does not exist yet", mods_path.display());
            return Ok(vec![]);
        }

        info!("scanning '{}'", mods_path.display());

        let entries = mods_path
            .read_dir()
            .with_context(|| format!("could not read '{}'", mods_path.display()))?;

        let mut groups: Vec<Group> = vec![];

        // Mods that don't have a group
        let mut root_group = Group::new(None, None);

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    warn!("skipping unreadable entry: {e}");
                    continue;
                }
            };

            match entry.file_type() {
                Ok(t) if t.is_dir() => {}
                Ok(_) => continue,
                Err(e) => {
                    warn!(
                        "skipping '{}': could not read its type: {e}",
                        entry.path().display()
                    );
                    continue;
                }
            }

            let name = entry.file_name().to_string_lossy().into_owned();

            if name.starts_with(STAGING_PREFIX) {
                continue;
            }

            if let Some(group_name) = name.strip_prefix(GROUP_PREFIX) {
                let mut group = Group::new(Some(group_name.to_string()), Some(entry.path()));
                match entry.path().read_dir() {
                    Ok(subs) => {
                        for sub in subs {
                            let sub = match sub {
                                Ok(sub) => sub,
                                Err(e) => {
                                    warn!("skipping unreadable entry in group '{group_name}': {e}");
                                    continue;
                                }
                            };
                            if sub.file_type().is_ok_and(|t| t.is_dir())
                                && !sub
                                    .file_name()
                                    .to_string_lossy()
                                    .starts_with(STAGING_PREFIX)
                                && Self::contains_pak(&sub.path())
                            {
                                group.add_mod(Self::get_mod_data(&sub.path()));
                            }
                        }
                    }
                    Err(e) => warn!("could not read group '{}': {e}", entry.path().display()),
                }
                if !group.mods.is_empty() {
                    group.mods.sort_by(|a, b| a.folder_name.cmp(&b.folder_name));
                }
                groups.push(group);
            } else if Self::contains_pak(&entry.path()) {
                root_group.add_mod(Self::get_mod_data(&entry.path()));
            } else {
                debug!("skipping '{name}': no mod files inside");
            }
        }

        if !root_group.mods.is_empty() {
            root_group
                .mods
                .sort_by(|a, b| a.folder_name.cmp(&b.folder_name));

            groups.insert(0, root_group);
        }

        let mod_count: usize = groups.iter().map(|g| g.mods.len()).sum();
        info!("found {mod_count} mod(s) in {} group(s)", groups.len());

        Ok(groups)
    }

    fn rename_all(pairs: &[(PathBuf, PathBuf)]) -> Result<()> {
        let mut done: Vec<&(PathBuf, PathBuf)> = Vec::with_capacity(pairs.len());

        for pair in pairs {
            let (old, new) = pair;
            if let Err(e) = std::fs::rename(old, new) {
                for (old, new) in done.iter().rev() {
                    if let Err(e) = std::fs::rename(new, old) {
                        error!(
                            "could not roll back '{}': {e}; the mod is now in a mixed state",
                            new.display()
                        );
                    }
                }
                return Err(anyhow!("could not rename '{}': {e}", old.display()));
            }
            done.push(pair);
        }

        Ok(())
    }

    pub fn toggle_mod(mod_: &Mod) -> Result<()> {
        let _guard = TOGGLE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);

        let folder = mod_.path.clone();
        if !folder.exists() {
            return Err(anyhow!(
                "Cannot toggle mod: folder not found for {}",
                mod_.folder_name
            ));
        }
        let files = read_dir_recursive(&folder);
        let disabled: Vec<_> = files
            .iter()
            .filter(|p| is_disabled_mod_file(&p.file_name().to_string_lossy()))
            .collect();

        if disabled.is_empty() {
            let targets = files
                .iter()
                .filter(|p| is_pak_file(&p.file_name().to_string_lossy()))
                .map(|pak| {
                    let old = pak.path();
                    let new = old.with_file_name(format!(
                        "{}{DISABLED_SUFFIX}",
                        pak.file_name().to_string_lossy()
                    ));
                    (old, new)
                })
                .collect::<Vec<_>>();

            if targets.is_empty() {
                return Err(anyhow!(
                    "Cannot toggle mod: {} has no .pak file to disable",
                    mod_.folder_name
                ));
            }

            Self::rename_all(&targets)?;
            trace!(
                "Mod disabled: renamed {} file(s) in {}",
                targets.len(),
                mod_.folder_name
            );
        } else {
            let targets = disabled
                .iter()
                .map(|pak| {
                    let old = pak.path();
                    let name = pak.file_name().to_string_lossy().into_owned();
                    let new =
                        old.with_file_name(name.strip_suffix(DISABLED_SUFFIX).unwrap_or(&name));
                    (old, new)
                })
                .collect::<Vec<_>>();

            Self::rename_all(&targets)?;
            trace!(
                "Mod enabled: renamed {} file(s) in {}",
                targets.len(),
                mod_.folder_name
            );
        }

        Ok(())
    }
}

// [UI HANDLER]

#[derive(Debug, Clone)]
struct ScannedGroup {
    id: String,
    name: String,
    mods: Vec<Mod>,
}

const UNGROUPED: &str = "\u{1}ungrouped";

/// `None` means the mod is up-to-date, `Some` means it needs to be updated
type UpdateCheck = Option<NteModFile>;

struct AddExistingEntry {
    id: String,
    name: String,
}

struct AddExisting {
    pool: Vec<AddExistingEntry>,
    selected: HashSet<String>,
    search: String,
}

#[derive(Default)]
struct State {
    scanned: Vec<ScannedGroup>,
    displayed: Vec<Mod>,
    rows: Vec<(bool, String)>,
    selected: HashSet<String>,
    selected_groups: HashSet<String>,
    collapsed: HashSet<String>,
    restart_required: HashSet<String>,
    updates: HashMap<String, UpdateCheck>,
    pending_edit_group: Option<String>,
    search: String,
    filter: ModStatusFilter,
    filter_characters: HashSet<String>,
    filter_authors: HashSet<String>,
    /// A group id, [`UNGROUPED`], or empty for every group
    filter_group: String,
    add_existing: Option<AddExisting>,
}

impl State {
    fn filtering(&self) -> bool {
        self.filter != ModStatusFilter::All
            || !self.filter_characters.is_empty()
            || !self.filter_authors.is_empty()
            || !self.filter_group.is_empty()
    }

    fn active_filter_count(&self) -> i32 {
        i32::from(self.filter != ModStatusFilter::All)
            + i32::from(!self.filter_group.is_empty())
            + i32::try_from(self.filter_characters.len() + self.filter_authors.len()).unwrap_or(0)
    }
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn config_map(key: &str) -> serde_json::Map<String, Value> {
    config::get(key).as_object().cloned().unwrap_or_default()
}

pub fn config_map_set(key: &str, entry: &str, value: Option<&str>) {
    config::modify(|data| {
        let mut map = data
            .get(key)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();

        match value {
            Some(v) => {
                map.insert(entry.to_string(), Value::from(v));
            }
            None => {
                map.remove(entry);
            }
        }

        data.insert(key.to_string(), Value::Object(map));
    });
}

fn mod_id(mod_: &Mod) -> String {
    mod_.path.to_string_lossy().into_owned()
}

fn note_missing(action: &str, mod_: &Mod) {
    warn!(
        "{action} '{}': already gone from '{}', refreshing",
        mod_.folder_name,
        mod_.path.display()
    );
}

fn expected_install_failure(error: &anyhow::Error) -> Option<&'static str> {
    let chain = format!("{error:#}");

    if chain.contains(ALREADY_INSTALLED) {
        return Some("is already installed");
    }

    if chain.contains("Password required") {
        return Some("is password protected, so Aurora cannot extract it");
    }

    None
}

fn rekey_mod_config(old: &str, new: &str) {
    if old == new {
        return;
    }

    for map_key in [key::MODMNG_NOTES, key::MODMNG_DISPLAY_NAMES] {
        config::modify(|data| {
            let Some(mut map) = data.get(map_key).and_then(Value::as_object).cloned() else {
                return;
            };

            let moved: Vec<String> = map
                .keys()
                .filter(|k| {
                    k.as_str() == old
                        || k.strip_prefix(old)
                            .is_some_and(|rest| rest.starts_with(std::path::MAIN_SEPARATOR))
                })
                .cloned()
                .collect();

            if moved.is_empty() {
                return;
            }

            for k in moved {
                let Some(value) = map.remove(&k) else {
                    continue;
                };
                map.insert(format!("{new}{}", &k[old.len()..]), value);
            }

            data.insert(map_key.to_string(), Value::Object(map));
        });
    }
}

fn mod_icon(mod_: &Mod, shown_name: &str) -> slint::Image {
    if mod_.has_icon_png
        && let Ok(image) = slint::Image::load_from_path(&mod_.path.join("icon.png"))
    {
        return image;
    }

    mod_.image_url
        .as_deref()
        .and_then(modicons::cached)
        .or_else(|| mod_.icon.as_deref().and_then(characters::icon_for))
        .or_else(|| characters::icon_for(shown_name))
        .or_else(|| characters::icon_for(&mod_.folder_name))
        .unwrap_or_default()
}

fn mod_character(mod_: &Mod, shown_name: &str) -> Option<&'static str> {
    mod_.icon
        .as_deref()
        .and_then(characters::character_for)
        .or_else(|| characters::character_for(shown_name))
        .or_else(|| characters::character_for(&mod_.folder_name))
}

fn mod_author(mod_: &Mod) -> Option<&str> {
    mod_.author
        .as_deref()
        .map(str::trim)
        .filter(|author| !author.is_empty() && *author != "Unknown")
}

fn encode_icon_png(bytes: &[u8]) -> Result<Vec<u8>> {
    let img = image::load_from_memory(bytes)?.to_rgba8();
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

fn icon_backup_path(mod_path: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    mod_path.hash(&mut hasher);

    get_cache_dir()
        .join("ModIconBackups")
        .join(format!("{:016x}", hasher.finish()))
}

fn backup_existing_icon(mod_: &Mod) {
    let current = mod_.path.join("icon.png");
    if !current.is_file() {
        return;
    }

    let backup = icon_backup_path(&mod_.path);
    if backup.exists() {
        return;
    }

    if let Some(parent) = backup.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!("could not create '{}': {e}", parent.display());
        return;
    }

    if let Err(e) = std::fs::copy(&current, &backup) {
        warn!(
            "could not back up the icon of '{}': {e}",
            mod_.path.display()
        );
    }
}

fn purge_icon_backup(mod_path: &Path) {
    match std::fs::remove_file(icon_backup_path(mod_path)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => warn!(
            "could not remove the icon backup of '{}': {e}",
            mod_path.display()
        ),
    }
}

fn rekey_icon_backup(old: &Path, new: &Path) {
    let old_backup = icon_backup_path(old);
    if !old_backup.exists() {
        return;
    }

    let new_backup = icon_backup_path(new);
    if let Some(parent) = new_backup.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        warn!("could not create '{}': {e}", parent.display());
        return;
    }

    if std::fs::rename(&old_backup, &new_backup).is_ok() {
        return;
    }

    match std::fs::copy(&old_backup, &new_backup) {
        Ok(_) => {
            let _ = std::fs::remove_file(&old_backup);
        }
        Err(e) => warn!(
            "could not move the icon backup '{}' to '{}': {e}",
            old_backup.display(),
            new_backup.display()
        ),
    }
}

fn write_icon(mod_: &Mod, png: &[u8]) -> Result<()> {
    backup_existing_icon(mod_);
    // Removing first breaks any hard link, so sibling instances of this mod
    // keep their old icon instead of getting the new one written through
    let target = mod_.path.join("icon.png");
    let _ = std::fs::remove_file(&target);
    std::fs::write(&target, png)
        .with_context(|| format!("could not write the icon of '{}'", mod_.path.display()))
}

fn report_icon_error(window: &slint::Weak<MainWindow>, mod_: &Mod, error: &anyhow::Error) {
    error!("{error:#}");
    let name = mod_.folder_name.clone();
    let text = format!("Could not change the icon of {name} - {error:#}");
    let ww = window.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(win) = ww.upgrade() {
            ModManagerHandler::show_toast(&win, "error", text);
        }
    });
}

pub struct ModManagerHandler;

impl ModManagerHandler {
    pub(crate) fn is_source_installed(mod_id: u32, file_id: u32) -> bool {
        ModManager::scan_mods().is_ok_and(|groups| {
            groups.into_iter().flat_map(|group| group.mods).any(|mod_| {
                mod_.source
                    .is_some_and(|source| source.mod_id == mod_id && source.file_id == file_id)
            })
        })
    }

    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("setup() called");
        Self::bind(window);
        Self::setup_file_drop(window);
        Self::setup_icon_choices(window);
        Self::reload(window);
        info!("setup() complete");
    }

    fn setup_icon_choices(window: &slint::Weak<MainWindow>) {
        // zero.png and mc.png are the same art
        const HIDDEN: &[&str] = &["zero"];
        const RENAMED: &[(&str, &str)] = &[("mc", "Zero")];

        let choices: Vec<IconChoice> = characters::slugs()
            .filter(|slug| !HIDDEN.contains(slug))
            .map(|slug| {
                let name = RENAMED
                    .iter()
                    .find(|(candidate, _)| *candidate == slug)
                    .map_or_else(
                        || characters::display_name(slug),
                        |(_, name)| (*name).to_string(),
                    );

                IconChoice {
                    slug: slug.into(),
                    name: name.as_str().into(),
                    icon: characters::icon(slug).unwrap_or_default(),
                }
            })
            .collect();

        window
            .unwrap()
            .set_mods_icon_choices(Rc::new(VecModel::from(choices)).into());
    }

    #[cfg(target_os = "windows")]
    fn setup_file_drop(window: &slint::Weak<MainWindow>) {
        crate::classes::filedrop::setup(window);
    }

    // TODO: This is not yet tested on linux
    #[cfg(not(target_os = "windows"))]
    fn setup_file_drop(window: &slint::Weak<MainWindow>) {
        use i_slint_backend_winit::WinitWindowAccessor;
        use i_slint_backend_winit::winit::event::WindowEvent;
        use slint::ComponentHandle;

        let ww = window.clone();
        window
            .unwrap()
            .window()
            .on_winit_window_event(move |_w, event| {
                if let Some(win) = ww.upgrade() {
                    match event {
                        WindowEvent::HoveredFile(_) => {
                            if win.get_show_mod_manager() {
                                win.set_mods_file_hover(true);
                            }
                        }
                        WindowEvent::HoveredFileCancelled => {
                            win.set_mods_file_hover(false);
                        }
                        WindowEvent::DroppedFile(path) => {
                            win.set_mods_file_hover(false);
                            if win.get_show_mod_manager() {
                                Self::install_paths(&ww, vec![path.clone()]);
                            }
                        }
                        _ => {}
                    }
                }
                i_slint_backend_winit::EventResult::Propagate
            });
    }

    fn note_toggle(id: &str) {
        if !GAME_RUNNING.load(Ordering::SeqCst) {
            return;
        }
        let mut state = STATE.lock().unwrap();
        if !state.restart_required.remove(id) {
            state.restart_required.insert(id.to_string());
        }
    }

    pub fn game_closed(w: &MainWindow) {
        let pending = {
            let mut state = STATE.lock().unwrap();
            let pending = !state.restart_required.is_empty();
            state.restart_required.clear();
            pending
        };

        if pending {
            Self::rebuild(w);
        }
    }

    fn show_toast(w: &MainWindow, kind: &str, text: String) {
        w.set_toast_text(text.into());
        w.set_toast_kind(kind.into());
        w.set_toast_active(true);
    }

    pub(crate) fn install_paths(window: &slint::Weak<MainWindow>, paths: Vec<PathBuf>) {
        Self::install_paths_with_done(window, paths, None, None, None);
    }

    fn set_progress(window: &slint::Weak<MainWindow>, progress: f32, text: String) {
        let ww = window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = ww.upgrade() {
                w.set_progress_overlay_progress(progress);
                w.set_progress_overlay_text(text.into());
            }
        });
    }

    pub(crate) fn install_paths_with_done(
        window: &slint::Weak<MainWindow>,
        paths: Vec<PathBuf>,
        source: Option<ModSource>,
        on_done: Option<InstallDoneCallback>,
        icon_png: Option<Vec<u8>>,
    ) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let units = Self::group_siblings(paths);
            let mut installed: Vec<String> = Vec::new();
            let mut failed: Vec<String> = Vec::new();

            let count = units.len();
            #[allow(clippy::cast_precision_loss)]
            let total = count as f32;
            let _ = slint::invoke_from_event_loop({
                let ww = ww.clone();
                move || {
                    if let Some(w) = ww.upgrade() {
                        w.set_progress_overlay_title(
                            if count > 1 {
                                "Installing Mods"
                            } else {
                                "Installing Mod"
                            }
                            .into(),
                        );
                        w.set_progress_overlay_progress(0.0);
                        w.set_progress_overlay_text("Installing...".into());
                        // Extraction can't be aborted halfway through
                        w.set_progress_overlay_cancellable(false);
                        w.set_progress_overlay_active(true);
                    }
                }
            });

            for (index, unit) in units.iter().enumerate() {
                let path = unit.first();
                let Some(path) = path else { continue };
                let label = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();

                #[allow(clippy::cast_precision_loss)]
                let base = index as f32;
                Self::set_progress(&ww, base / total, format!("Extracting {label}..."));

                let mut last_percent = 0u64;
                let mut on_progress = |frac: f32| {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let percent = (frac * 100.0) as u64;
                    if percent > last_percent {
                        last_percent = percent;
                        Self::set_progress(
                            &ww,
                            (base + frac) / total,
                            format!("Extracting {label}..."),
                        );
                    }
                };

                match Self::install_path(unit, &mut on_progress) {
                    Ok(name) => {
                        info!("installed '{name}' from '{}'", path.display());
                        if let Some(folder) = get_mods_path().map(|mods| mods.join(&name)) {
                            if let Some(source) = &source {
                                source.write(&folder);
                                if !source.name.is_empty() {
                                    config_map_set(
                                        key::MODMNG_NOTES,
                                        &folder.to_string_lossy(),
                                        Some(&source.name),
                                    );
                                }
                            }
                            if let Some(png) = &icon_png
                                && let Err(e) = std::fs::write(folder.join("icon.png"), png)
                            {
                                warn!("could not write the icon of '{}': {e}", folder.display());
                            }
                        }
                        installed.push(name);
                    }
                    Err(e) => {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();

                        if let Some(reason) = expected_install_failure(&e) {
                            warn!("skipped '{}': {reason}", path.display());
                            failed.push(format!("{name} {reason}"));
                        } else {
                            error!("could not install '{}': {e}", path.display());
                            failed.push(format!("{name}: {e}"));
                        }
                    }
                }
            }

            let ww2 = ww.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(win) = ww2.upgrade() else { return };
                win.set_progress_overlay_active(false);

                let ok = match installed.len() {
                    0 => String::new(),
                    1 => format!("Installed {}", installed[0]),
                    n => format!("Installed {n} mods"),
                };

                if failed.is_empty() {
                    if !ok.is_empty() {
                        Self::show_toast(&win, "success", ok);
                    }
                } else {
                    let errors = failed.join("; ");
                    let text = if ok.is_empty() {
                        format!("Install failed - {errors}")
                    } else {
                        format!("{ok}, {} failed - {errors}", failed.len())
                    };
                    Self::show_toast(&win, "error", text);
                }
                if let Some(on_done) = on_done {
                    on_done(&win);
                }
            });

            Self::reload(&ww);
        });
    }

    fn group_siblings(paths: Vec<PathBuf>) -> Vec<Vec<PathBuf>> {
        let mut units: Vec<Vec<PathBuf>> = Vec::new();
        let mut index: HashMap<(PathBuf, String), usize> = HashMap::new();

        for path in paths {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if MOD_EXTENSIONS.contains(&ext.as_str()) && path.is_file() {
                let key = (
                    path.parent().map(Path::to_path_buf).unwrap_or_default(),
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase(),
                );
                if let Some(&at) = index.get(&key) {
                    units[at].push(path);
                    continue;
                }
                index.insert(key, units.len());
            }

            units.push(vec![path]);
        }

        units
    }

    fn install_into(target: &Path, fill: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
        if target.exists() {
            return Err(anyhow!(ALREADY_INSTALLED));
        }

        let parent = target
            .parent()
            .ok_or_else(|| anyhow!("mods folder could not be resolved"))?;
        let name = target
            .file_name()
            .ok_or_else(|| anyhow!("invalid mod name"))?
            .to_string_lossy()
            .into_owned();
        let staging = parent.join(format!("{STAGING_PREFIX}{name}"));

        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;

        let finish = |staging: &Path| -> Result<()> {
            fill(staging)?;
            if target.exists() {
                return Err(anyhow!(ALREADY_INSTALLED));
            }
            std::fs::rename(staging, target)?;
            Ok(())
        };

        match finish(&staging) {
            Ok(()) => Ok(()),
            Err(e) => {
                if let Err(cleanup) = std::fs::remove_dir_all(&staging) {
                    warn!("could not clean up '{}': {cleanup}", staging.display());
                }
                Err(e)
            }
        }
    }

    fn install_path(unit: &[PathBuf], progress: &mut dyn FnMut(f32)) -> Result<String> {
        let path = unit.first().ok_or_else(|| anyhow!("nothing to install"))?;
        let mods_path =
            get_mods_path().ok_or_else(|| anyhow!("mods folder could not be resolved"))?;
        std::fs::create_dir_all(&mods_path)?;

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("invalid file name"))?;

        if path.is_dir() {
            let target = mods_path.join(
                path.file_name()
                    .ok_or_else(|| anyhow!("invalid folder name"))?,
            );
            Self::install_into(&target, |staging| Self::copy_dir_recursive(path, staging))?;
            return Ok(name);
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let target = mods_path.join(&name);

        if MOD_EXTENSIONS.contains(&ext.as_str()) {
            Self::install_into(&target, |staging| {
                for file in unit {
                    std::fs::copy(
                        file,
                        staging.join(file.file_name().with_context(|| "invalid file name")?),
                    )?;
                }
                Ok(())
            })?;
        } else if ARCHIVE_EXTENSIONS.contains(&ext.as_str()) {
            Self::install_into(&target, |staging| {
                extract_archive_with_progress(path.as_path(), staging, &mut |done, total| {
                    if total > 0 {
                        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                        progress((done as f64 / total as f64).min(1.0) as f32);
                    }
                })?;
                if !ModManager::contains_pak(&staging.to_path_buf()) {
                    return Err(anyhow!(
                        "archive contains no .pak, .utoc, or .ucas mod files"
                    ));
                }
                Ok(())
            })?;
        } else {
            return Err(anyhow!("unsupported file type '.{ext}'"));
        }

        Ok(name)
    }

    pub(crate) fn apply_update(
        window: &slint::Weak<MainWindow>,
        folder: PathBuf,
        downloaded: PathBuf,
        source: ModSource,
        on_done: Option<InstallDoneCallback>,
    ) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let name = folder
                .file_name()
                .unwrap_or(folder.as_os_str())
                .to_string_lossy()
                .into_owned();

            let was_enabled = !read_dir_recursive(&folder)
                .iter()
                .any(|p| is_disabled_mod_file(&p.file_name().to_string_lossy()));

            let outcome = Self::replace_folder(&folder, &downloaded, &mut |frac| {
                Self::set_progress(&ww, frac, format!("Updating {name}..."));
            });

            let failure = match outcome {
                Ok(()) => {
                    info!("updated '{}'", folder.display());
                    source.write(&folder);

                    if !was_enabled {
                        let updated = Mod {
                            folder_name: name.clone(),
                            path: folder.clone(),
                            ..Default::default()
                        };
                        if let Err(e) = ModManager::toggle_mod(&updated) {
                            warn!("could not disable '{name}' again after updating: {e}");
                        }
                    }

                    STATE
                        .lock()
                        .unwrap()
                        .updates
                        .insert(folder.to_string_lossy().into_owned(), None);
                    None
                }
                Err(e) => {
                    error!("could not update '{}': {e:#}", folder.display());
                    Some(format!("{e:#}"))
                }
            };

            let ww2 = ww.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(win) = ww2.upgrade() else { return };
                win.set_progress_overlay_active(false);
                match failure {
                    None => Self::show_toast(&win, "success", format!("Updated {name}")),
                    Some(e) => {
                        Self::show_toast(&win, "error", format!("Could not update {name} - {e}"));
                    }
                }
                if let Some(on_done) = on_done {
                    on_done(&win);
                }
            });

            Self::reload(&ww);
        });
    }

    fn replace_folder(
        folder: &Path,
        downloaded: &Path,
        progress: &mut dyn FnMut(f32),
    ) -> Result<()> {
        let parent = folder
            .parent()
            .ok_or_else(|| anyhow!("the mod folder could not be resolved"))?;
        let name = folder
            .file_name()
            .ok_or_else(|| anyhow!("invalid mod name"))?
            .to_string_lossy()
            .into_owned();
        let staging = parent.join(format!("{STAGING_PREFIX}{name}"));

        if staging.exists() {
            std::fs::remove_dir_all(&staging)?;
        }
        std::fs::create_dir_all(&staging)?;

        let ext = downloaded
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Updates keep whatever icon.png the mod already had
        let kept_icon = std::fs::read(folder.join("icon.png")).ok();

        let filled = if ARCHIVE_EXTENSIONS.contains(&ext.as_str()) {
            extract_archive_with_progress(downloaded, &staging, &mut |done, total| {
                if total > 0 {
                    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
                    progress((done as f64 / total as f64).min(1.0) as f32);
                }
            })
        } else if MOD_EXTENSIONS.contains(&ext.as_str()) {
            downloaded
                .file_name()
                .ok_or_else(|| anyhow!("invalid file name"))
                .and_then(|file_name| {
                    std::fs::copy(downloaded, staging.join(file_name))?;
                    Ok(())
                })
        } else {
            Err(anyhow!("unsupported file type '.{ext}'"))
        };

        let clean_up = |staging: &Path| {
            if let Err(e) = std::fs::remove_dir_all(staging) {
                warn!("could not clean up '{}': {e}", staging.display());
            }
        };

        if let Err(e) = filled {
            clean_up(&staging);
            return Err(e);
        }

        if let Err(e) = std::fs::remove_dir_all(folder) {
            clean_up(&staging);
            return Err(anyhow!("could not clear '{}': {e}", folder.display()));
        }

        std::fs::rename(&staging, folder)
            .with_context(|| format!("could not move the new files into '{}'", folder.display()))?;

        if let Some(png) = kept_icon
            && let Err(e) = std::fs::write(folder.join("icon.png"), &png)
        {
            warn!("could not restore the icon of '{}': {e}", folder.display());
        }

        Ok(())
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in src.read_dir()? {
            let entry = entry?;
            let target = dst.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                Self::copy_dir_recursive(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), &target)?;
            }
        }
        Ok(())
    }

    pub(crate) fn reload(window: &slint::Weak<MainWindow>) {
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let ww = window.clone();
        std::thread::spawn(move || {
            let (groups, error) = match ModManager::scan_mods() {
                Ok(groups) => (groups, None),
                Err(e) => {
                    error!("could not scan the mods folder: {e}");
                    (vec![], Some(format!("{e}")))
                }
            };

            let scanned: Vec<ScannedGroup> = groups
                .into_iter()
                .map(|g| ScannedGroup {
                    id: g
                        .path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    name: g.name.clone().unwrap_or_default(),
                    mods: g.mods,
                })
                .collect();

            let _ = slint::invoke_from_event_loop(move || {
                if GENERATION.load(Ordering::SeqCst) != generation {
                    return;
                }
                let Some(w) = ww.upgrade() else {
                    error!("could not load: window handle is dead");
                    return;
                };

                STATE.lock().unwrap().scanned = scanned;
                Self::rebuild(&w);
                Self::check_updates(&ww);

                if let Some(error) = error {
                    Self::show_toast(&w, "error", format!("Could not read mods folder - {error}"));
                }
            });
        });
    }

    fn tag_for(m: &Mod, updates: &HashMap<String, UpdateCheck>) -> ModTag {
        if m.source.is_none() {
            return ModTag::Local;
        }

        match updates.get(&mod_id(m)) {
            Some(Some(_)) => ModTag::UpdateAvailable,
            Some(None) => ModTag::UpToDate,
            None => ModTag::None,
        }
    }

    fn check_updates(window: &slint::Weak<MainWindow>) {
        let pending: Vec<(String, ModSource)> = {
            let state = STATE.lock().unwrap();
            state
                .scanned
                .iter()
                .flat_map(|g| g.mods.iter())
                .filter(|m| !state.updates.contains_key(&mod_id(m)))
                .filter_map(|m| m.source.clone().map(|source| (mod_id(m), source)))
                .collect()
        };

        if pending.is_empty() {
            return;
        }

        let ww = window.clone();
        gbbrowser::runtime().spawn(async move {
            let mod_ids: HashSet<u32> = pending.iter().map(|(_, s)| s.mod_id).collect();
            let mut fetched: HashMap<u32, Vec<NteModFile>> = HashMap::new();
            for mod_id in mod_ids {
                match gbbrowser::mod_files(mod_id).await {
                    Some(files) => {
                        fetched.insert(mod_id, files);
                    }
                    None => warn!("could not check mod {mod_id} for updates"),
                }
            }

            let mut results: Vec<(String, UpdateCheck)> = Vec::new();
            for (id, source) in pending {
                let Some(files) = fetched.get(&source.mod_id) else {
                    continue;
                };

                let newest = files
                    .iter()
                    .find(|f| f.id == source.file_id)
                    .or_else(|| files.iter().find(|f| f.name == source.file_name));
                let Some(newest) = newest else {
                    warn!("'{}' is no longer on GameBanana", source.file_name);
                    continue;
                };

                let outdated =
                    !newest.md5.is_empty() && !newest.md5.eq_ignore_ascii_case(&source.md5);
                results.push((id, outdated.then(|| newest.clone())));
            }

            let _ = slint::invoke_from_event_loop(move || {
                let Some(w) = ww.upgrade() else { return };
                STATE.lock().unwrap().updates.extend(results);
                Self::rebuild(&w);
            });
        });
    }

    fn rebuild(w: &MainWindow) {
        let display_names = config_map(key::MODMNG_DISPLAY_NAMES);
        let notes = config_map(key::MODMNG_NOTES);

        let shown_name = |m: &Mod| -> String {
            display_names
                .get(&mod_id(m))
                .and_then(|v| v.as_str())
                .unwrap_or(&m.display_name)
                .to_string()
        };

        let (
            items,
            grid_sections,
            groups,
            selected_count,
            selected_group_count,
            all_selected,
            wanted_images,
        ) = {
            let mut state = STATE.lock().unwrap();
            let mut items: Vec<ModItem> = Vec::new();
            let mut grid_sections: Vec<Vec<ModItem>> = Vec::new();
            let mut displayed: Vec<Mod> = Vec::new();
            let mut rows: Vec<(bool, String)> = Vec::new();
            let mut wanted_images: Vec<(String, String)> = Vec::new();
            let searching = !state.search.is_empty();

            // Anything toggled during a session is live again once the game is
            // gone, whether or not we saw the close event
            if !GAME_RUNNING.load(Ordering::SeqCst) {
                state.restart_required.clear();
            }

            let filtering = state.filtering();

            for group in state.scanned.clone() {
                let is_root = group.id.is_empty();

                let in_filtered_group = match state.filter_group.as_str() {
                    "" => true,
                    UNGROUPED => is_root,
                    id => group.id == id,
                };
                if !in_filtered_group {
                    continue;
                }

                let group_matches =
                    !is_root && searching && group.name.to_lowercase().contains(&state.search);

                let visible: Vec<&Mod> = group
                    .mods
                    .iter()
                    .filter(|m| {
                        let wrong_status = match state.filter {
                            ModStatusFilter::Enabled => !m.is_enabled,
                            ModStatusFilter::Disabled => m.is_enabled,
                            ModStatusFilter::All => false,
                        };
                        if wrong_status {
                            return false;
                        }

                        if !state.filter_authors.is_empty()
                            && !mod_author(m)
                                .is_some_and(|author| state.filter_authors.contains(author))
                        {
                            return false;
                        }

                        if searching
                            && !group_matches
                            && !shown_name(m).to_lowercase().contains(&state.search)
                            && !m.folder_name.to_lowercase().contains(&state.search)
                        {
                            return false;
                        }

                        if !state.filter_characters.is_empty()
                            && !mod_character(m, &shown_name(m))
                                .is_some_and(|slug| state.filter_characters.contains(slug))
                        {
                            return false;
                        }

                        true
                    })
                    .collect();

                let collapsed = !is_root && state.collapsed.contains(&group.id);
                let mut section: Vec<ModItem> = Vec::new();

                if !is_root {
                    if (searching || filtering) && visible.is_empty() && !group_matches {
                        continue;
                    }
                    let header = ModItem {
                        id: group.id.clone().into(),
                        name: group.name.clone().into(),
                        author: "".into(),
                        version: "".into(),
                        icon: slint::Image::default(),
                        notes: "".into(),
                        enabled: !group.mods.is_empty() && group.mods.iter().all(|m| m.is_enabled),
                        selected: if group.mods.is_empty() {
                            state.selected_groups.contains(&group.id)
                        } else {
                            !visible.is_empty()
                                && visible.iter().all(|m| state.selected.contains(&mod_id(m)))
                        },
                        has_json: false,
                        is_editing: false,
                        group_id: "".into(),
                        support_link: "".into(),
                        is_group_header: true,
                        collapsed,
                        restart_required: false,
                        tag: ModTag::None,
                        has_icon_png: false,
                    };
                    items.push(header.clone());
                    section.push(header);
                    rows.push((true, group.id.clone()));
                }

                for m in visible {
                    displayed.push(m.clone());
                    if !collapsed {
                        let version = m
                            .version
                            .clone()
                            .filter(|v| v != "Unknown")
                            .unwrap_or_default();
                        let name = shown_name(m);
                        if let Some(url) = m.image_url.clone() {
                            wanted_images.push((mod_id(m), url));
                        }
                        let item = ModItem {
                            id: mod_id(m).into(),
                            icon: mod_icon(m, &name),
                            name: name.into(),
                            author: m.author.clone().unwrap_or_default().into(),
                            version: version.into(),
                            notes: notes
                                .get(&mod_id(m))
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .into(),
                            enabled: m.is_enabled,
                            selected: state.selected.contains(&mod_id(m)),
                            has_json: m.has_json,
                            is_editing: false,
                            group_id: group.id.clone().into(),
                            support_link: m.support_link.clone().unwrap_or_default().into(),
                            is_group_header: false,
                            collapsed: false,
                            restart_required: state.restart_required.contains(&mod_id(m)),
                            tag: Self::tag_for(m, &state.updates),
                            has_icon_png: m.has_icon_png,
                        };
                        items.push(item.clone());
                        section.push(item);
                        rows.push((false, group.id.clone()));
                    }
                }

                if !section.is_empty() {
                    grid_sections.push(section);
                }
            }

            let groups: Vec<GroupOption> = state
                .scanned
                .iter()
                .filter(|g| !g.id.is_empty())
                .map(|g| GroupOption {
                    id: g.id.as_str().into(),
                    name: g.name.as_str().into(),
                })
                .collect();

            let empty_groups: HashSet<String> = state
                .scanned
                .iter()
                .filter(|g| !g.id.is_empty() && g.mods.is_empty())
                .map(|g| g.id.clone())
                .collect();
            state.selected_groups.retain(|id| empty_groups.contains(id));

            let existing: HashSet<String> = displayed.iter().map(mod_id).collect();
            state.selected.retain(|id| existing.contains(id));
            let installed: HashSet<String> = state
                .scanned
                .iter()
                .flat_map(|g| g.mods.iter().map(mod_id))
                .collect();
            state.restart_required.retain(|id| installed.contains(id));
            state.updates.retain(|id, _| installed.contains(id));
            state.displayed = displayed;
            state.rows = rows;

            let count = state.selected.len();
            let group_count = state.selected_groups.len();
            let all = !state.displayed.is_empty() && count == state.displayed.len();
            drop(state);

            (
                items,
                grid_sections,
                groups,
                count,
                group_count,
                all,
                wanted_images,
            )
        };

        w.set_mods(Rc::new(VecModel::from(items)).into());
        let sections: Vec<ModelRc<ModItem>> = grid_sections
            .into_iter()
            .map(|s| ModelRc::from(Rc::new(VecModel::from(s))))
            .collect();
        w.set_mods_grid(Rc::new(VecModel::from(sections)).into());
        w.set_mods_groups(Rc::new(VecModel::from(groups)).into());
        w.set_mods_selected_count(i32::try_from(selected_count).unwrap_or(0));
        w.set_mods_selected_group_count(i32::try_from(selected_group_count).unwrap_or(0));
        w.set_mods_all_selected(all_selected);

        let pending_edit = STATE.lock().unwrap().pending_edit_group.take();
        w.set_mods_editing_group_id(pending_edit.unwrap_or_default().into());

        Self::refresh_filter_options(w, &shown_name);

        modicons::load(w.as_weak(), wanted_images, Self::apply_icon);
    }

    fn refresh_filter_options(w: &MainWindow, shown_name: &dyn Fn(&Mod) -> String) {
        let state = STATE.lock().unwrap();

        let mut characters: BTreeMap<&'static str, i32> = BTreeMap::new();
        let mut authors: BTreeMap<String, i32> = BTreeMap::new();
        let mut group_rows: Vec<FilterOption> = Vec::new();
        let mut ungrouped = 0i32;
        let mut total = 0i32;

        for group in &state.scanned {
            let count = i32::try_from(group.mods.len()).unwrap_or(0);
            total += count;

            if group.id.is_empty() {
                ungrouped = count;
            } else {
                group_rows.push(FilterOption {
                    id: group.id.as_str().into(),
                    name: group.name.as_str().into(),
                    icon: slint::Image::default(),
                    count,
                    selected: state.filter_group == group.id,
                });
            }

            for m in &group.mods {
                if let Some(slug) = mod_character(m, &shown_name(m)) {
                    *characters.entry(slug).or_default() += 1;
                }
                if let Some(author) = mod_author(m) {
                    *authors.entry(author.to_string()).or_default() += 1;
                }
            }
        }

        let character_rows: Vec<FilterOption> = characters
            .into_iter()
            .map(|(slug, count)| FilterOption {
                id: slug.into(),
                name: characters::display_name(slug).as_str().into(),
                icon: characters::icon(slug).unwrap_or_default(),
                count,
                selected: state.filter_characters.contains(slug),
            })
            .collect();

        let author_rows: Vec<FilterOption> = authors
            .into_iter()
            .map(|(author, count)| FilterOption {
                id: author.as_str().into(),
                name: author.as_str().into(),
                icon: slint::Image::default(),
                count,
                selected: state.filter_authors.contains(&author),
            })
            .collect();

        let mut groups: Vec<FilterOption> = vec![FilterOption {
            id: "".into(),
            name: crate::translations::tr("global.filter.all").as_str().into(),
            icon: slint::Image::default(),
            count: total,
            selected: state.filter_group.is_empty(),
        }];

        if ungrouped > 0 {
            groups.push(FilterOption {
                id: UNGROUPED.into(),
                name: crate::translations::tr("modmanager.filter.ungrouped")
                    .as_str()
                    .into(),
                icon: slint::Image::default(),
                count: ungrouped,
                selected: state.filter_group == UNGROUPED,
            });
        }
        groups.append(&mut group_rows);

        let label = groups
            .iter()
            .find(|option| option.selected)
            .or_else(|| groups.first())
            .map(|option| option.name.clone())
            .unwrap_or_default();

        let count = state.active_filter_count();
        drop(state);

        w.set_mods_filter_characters(Rc::new(VecModel::from(character_rows)).into());
        w.set_mods_filter_authors(Rc::new(VecModel::from(author_rows)).into());
        w.set_mods_filter_groups(Rc::new(VecModel::from(groups)).into());
        w.set_mods_filter_group_label(label);
        w.set_mods_active_filter_count(count);
    }

    /// Drops a finished custom image onto its card.
    fn apply_icon(w: &MainWindow, id: &str, image: &slint::Image) {
        Self::update_row(w, id, |row| row.icon = image.clone());
    }

    fn update_selection_props(w: &MainWindow) {
        let state = STATE.lock().unwrap();
        let count = state.selected.len();
        let all = !state.displayed.is_empty() && count == state.displayed.len();
        drop(state);
        w.set_mods_selected_count(i32::try_from(count).unwrap_or(0));
        w.set_mods_all_selected(all);
    }

    fn group_path(name: &str) -> Option<PathBuf> {
        get_mods_path().map(|p| p.join(format!("{GROUP_PREFIX}{name}")))
    }

    // [ADD EXISTING MODS]

    /// Creates `dst` as a real folder whose files are hard links into `src`
    fn hard_link_dir(src: &Path, dst: &Path) -> Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in src.read_dir()? {
            let entry = entry?;
            let target = dst.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                Self::hard_link_dir(&entry.path(), &target)?;
            } else {
                std::fs::hard_link(entry.path(), &target).with_context(|| {
                    format!(
                        "could not link '{}' into '{}'",
                        entry.path().display(),
                        target.display()
                    )
                })?;
            }
        }
        Ok(())
    }

    fn copy_mod_config(old: &str, new: &str) {
        if old == new {
            return;
        }

        for map_key in [key::MODMNG_NOTES, key::MODMNG_DISPLAY_NAMES] {
            config::modify(|data| {
                let Some(mut map) = data.get(map_key).and_then(Value::as_object).cloned() else {
                    return;
                };

                let Some(value) = map.get(old).cloned() else {
                    return;
                };

                map.insert(new.to_string(), value);
                data.insert(map_key.to_string(), Value::Object(map));
            });
        }
    }

    fn open_add_existing(w: &MainWindow, group_id: &str) {
        let display_names = config_map(key::MODMNG_DISPLAY_NAMES);
        let shown_name = |m: &Mod| -> String {
            display_names
                .get(&mod_id(m))
                .and_then(|v| v.as_str())
                .unwrap_or(&m.display_name)
                .to_string()
        };

        let group_name = {
            let state = STATE.lock().unwrap();
            let Some(group) = state.scanned.iter().find(|g| g.id == group_id) else {
                return;
            };
            let inside: HashSet<String> = group.mods.iter().map(mod_id).collect();
            let pool: Vec<AddExistingEntry> = state
                .scanned
                .iter()
                .flat_map(|g| g.mods.iter())
                .filter(|m| !inside.contains(&mod_id(m)))
                .map(|m| AddExistingEntry {
                    id: mod_id(m),
                    name: shown_name(m),
                })
                .collect();
            let name = group.name.clone();
            drop(state);
            (name, pool)
        };

        {
            let mut state = STATE.lock().unwrap();
            state.add_existing = Some(AddExisting {
                pool: group_name.1,
                selected: HashSet::new(),
                search: String::new(),
            });
        }

        w.set_mods_add_existing_group_id(group_id.into());
        w.set_mods_add_existing_group_name(group_name.0.as_str().into());
        w.set_mods_add_existing_search("".into());
        Self::rebuild_add_existing_model(w);
        w.set_mods_add_existing_open(true);
    }

    fn rebuild_add_existing_model(w: &MainWindow) {
        let display_names = config_map(key::MODMNG_DISPLAY_NAMES);

        let options: Vec<FilterOption> = {
            let state = STATE.lock().unwrap();
            let Some(picker) = &state.add_existing else {
                return;
            };

            let filtered: Vec<FilterOption> = picker
                .pool
                .iter()
                .filter(|entry| {
                    picker.search.is_empty() || entry.name.to_lowercase().contains(&picker.search)
                })
                .filter_map(|entry| {
                    let m = state
                        .scanned
                        .iter()
                        .flat_map(|g| g.mods.iter())
                        .find(|m| mod_id(m) == entry.id)?;
                    let shown = display_names
                        .get(&mod_id(m))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&m.display_name);
                    Some(FilterOption {
                        id: entry.id.as_str().into(),
                        name: entry.name.as_str().into(),
                        icon: mod_icon(m, shown),
                        count: 0,
                        selected: picker.selected.contains(&entry.id),
                    })
                })
                .collect();
            drop(state);
            filtered
        };

        w.set_mods_add_existing_options(Rc::new(VecModel::from(options)).into());
    }

    // Items below must match the list layout + margins of modmanager.slint
    const LIST_PADDING_TOP: f32 = 12.0;
    const LIST_SPACING: f32 = 8.0;
    const HEADER_ROW_H: f32 = 52.0;
    const CARD_ROW_H: f32 = 64.0;

    fn zone_at(content_y: f32) -> Option<String> {
        if content_y < 0.0 {
            return None;
        }

        let rows = STATE.lock().unwrap().rows.clone();
        let mut y0 = Self::LIST_PADDING_TOP;
        let mut prev_zone = String::new();
        for (is_header, zone) in rows {
            if content_y < y0 {
                return Some(prev_zone);
            }
            let h = if is_header {
                Self::HEADER_ROW_H
            } else {
                Self::CARD_ROW_H
            };
            if content_y < y0 + h {
                return Some(zone);
            }
            prev_zone = zone;
            y0 += h + Self::LIST_SPACING;
        }

        Some(String::new())
    }

    fn drop_mods_on_zone(window: &slint::Weak<MainWindow>, id: String, zone: String) {
        let zone = if !zone.is_empty()
            && Self::mod_by_id(&id).is_some_and(|m| Self::current_zone(&m) == zone)
        {
            String::new()
        } else {
            zone
        };

        let ids: Vec<String> = {
            let state = STATE.lock().unwrap();
            if state.selected.contains(&id) && state.selected.len() > 1 {
                state
                    .displayed
                    .iter()
                    .map(mod_id)
                    .filter(|i| state.selected.contains(i))
                    .collect()
            } else {
                vec![id]
            }
        };

        Self::move_mods_to_zone(window, ids, zone);
    }

    fn current_zone(m: &Mod) -> String {
        let parent = m.path.parent().map(Path::to_path_buf).unwrap_or_default();
        if get_mods_path().is_some_and(|mp| mp == parent) {
            String::new()
        } else {
            parent.to_string_lossy().into_owned()
        }
    }

    fn move_mods_to_zone(window: &slint::Weak<MainWindow>, ids: Vec<String>, group_id: String) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let target_dir = if group_id.is_empty() {
                get_mods_path()
            } else {
                Some(PathBuf::from(&group_id))
            };
            let Some(target_dir) = target_dir else { return };

            for id in &ids {
                let Some(m) = Self::mod_by_id(id) else {
                    continue;
                };
                if Self::current_zone(&m) == group_id {
                    continue;
                }

                let target = target_dir.join(&m.folder_name);
                if target.exists() {
                    warn!("not moving '{}': target already exists", m.folder_name);
                    let ww2 = ww.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = ww2.upgrade() {
                            Self::show_toast(
                                &win,
                                "warning",
                                format!("Not moving '{}'. This mod already exists.", m.folder_name),
                            );
                        }
                    });
                } else if let Err(e) = std::fs::rename(&m.path, &target) {
                    warn!("could not move '{}': {e}", m.folder_name);
                    let ww2 = ww.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(win) = ww2.upgrade() {
                            Self::show_toast(
                                &win,
                                "error",
                                format!(
                                    "Could not move '{}'. Game needs to be closed before moving mods.",
                                    m.folder_name
                                ),
                            );
                        }
                    });
                } else {
                    rekey_mod_config(id, &target.to_string_lossy());
                    rekey_icon_backup(&m.path, &target);
                    let mut state = STATE.lock().unwrap();
                    if state.selected.remove(id) {
                        state.selected.insert(target.to_string_lossy().into_owned());
                    }
                    drop(state);
                    info!("moved '{}' → '{}'", m.folder_name, target_dir.display());
                }
            }
            Self::reload(&ww);
        });
    }

    fn add_existing_mods(window: &slint::Weak<MainWindow>, group_id: String, ids: Vec<String>) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let group_dir = PathBuf::from(&group_id);
            if !group_dir.is_dir() {
                error!(
                    "cannot add mods: '{}' is not a group folder",
                    group_dir.display()
                );
                return;
            }

            let mut added: Vec<String> = Vec::new();
            let mut failed: Vec<String> = Vec::new();

            for id in &ids {
                let Some(m) = Self::mod_by_id(id) else {
                    continue;
                };

                // Already living in this group
                if m.path.parent().is_some_and(|p| p == group_dir) {
                    continue;
                }

                let base = m.folder_name.clone();
                let mut name = base.clone();
                let mut counter = 1;
                while group_dir.join(&name).exists() {
                    counter += 1;
                    name = format!("{base} ({counter})");
                }
                let target = group_dir.join(&name);

                match Self::hard_link_dir(&m.path, &target) {
                    Ok(()) => {
                        let new_id = target.to_string_lossy().into_owned();
                        Self::copy_mod_config(id, &new_id);
                        added.push(name);
                        info!("linked '{}' into '{}'", m.path.display(), target.display());
                    }
                    Err(e) => {
                        error!("could not link '{}': {e}", m.path.display());
                        failed.push(format!("{}: {e}", m.folder_name));
                        if let Err(cleanup) = std::fs::remove_dir_all(&target)
                            && cleanup.kind() != std::io::ErrorKind::NotFound
                        {
                            warn!("could not clean up '{}': {cleanup}", target.display());
                        }
                    }
                }
            }

            let ww2 = ww.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(win) = ww2.upgrade() else { return };
                let ok = match added.len() {
                    0 => String::new(),
                    1 => format!("Added {}", added[0]),
                    n => format!("Added {n} mods"),
                };
                if failed.is_empty() {
                    if !ok.is_empty() {
                        Self::show_toast(&win, "success", ok);
                    }
                } else {
                    let errors = failed.join("; ");
                    let text = if ok.is_empty() {
                        format!("Could not add mods - {errors}")
                    } else {
                        format!("{ok}, {} failed - {errors}", failed.len())
                    };
                    Self::show_toast(&win, "error", text);
                }
            });

            Self::reload(&ww);
        });
    }

    fn delete_group_folder(group_path: &Path) {
        let Some(mods_path) = get_mods_path() else {
            return;
        };

        if let Ok(entries) = group_path.read_dir() {
            for entry in entries.flatten() {
                let target = mods_path.join(entry.file_name());
                if target.exists() {
                    warn!(
                        "not moving '{}' out of group: target exists",
                        entry.path().display()
                    );
                    continue;
                }
                if let Err(e) = std::fs::rename(entry.path(), &target) {
                    error!(
                        "could not move '{}' out of group: {e}",
                        entry.path().display()
                    );
                } else {
                    rekey_mod_config(&entry.path().to_string_lossy(), &target.to_string_lossy());
                    rekey_icon_backup(&entry.path(), &target);
                }
            }
        }

        if let Err(e) = std::fs::remove_dir(group_path) {
            error!("could not delete group '{}': {e}", group_path.display());
        } else {
            info!("deleted group '{}'", group_path.display());
        }
    }

    fn selected_ids() -> Vec<String> {
        let state = STATE.lock().unwrap();
        state
            .displayed
            .iter()
            .map(mod_id)
            .filter(|id| state.selected.contains(id))
            .collect()
    }

    fn checked_ids(options: &ModelRc<FilterOption>) -> HashSet<String> {
        options
            .iter()
            .filter(|option| option.selected)
            .map(|option| option.id.to_string())
            .collect()
    }

    fn mod_by_id(id: &str) -> Option<Mod> {
        STATE
            .lock()
            .unwrap()
            .displayed
            .iter()
            .find(|m| mod_id(m) == id)
            .cloned()
    }

    // [CALLBACKS]

    fn bind(window: &slint::Weak<MainWindow>) {
        let w = window.unwrap();

        w.set_mods_view_grid(
            config::get(key::MODMNG_VIEW_GRID)
                .as_bool()
                .unwrap_or(false),
        );
        w.on_mods_view_changed(|grid| {
            config::set(key::MODMNG_VIEW_GRID, Value::from(grid));
        });

        let ww = window.clone();
        w.on_mod_toggle(move |id| {
            let id = id.to_string();
            let ww = ww.clone();
            std::thread::spawn(move || {
                if let Some(m) = Self::mod_by_id(&id) {
                    match ModManager::toggle_mod(&m) {
                        Ok(()) => Self::note_toggle(&id),
                        Err(e) => {
                            if m.path.exists() {
                                error!("could not toggle '{}': {e}", m.folder_name);
                            } else {
                                note_missing("could not toggle", &m);
                            }
                            let ww = ww.clone();
                            let name = m.folder_name.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(win) = ww.upgrade() {
                                    Self::show_toast(
                                        &win,
                                        "error",
                                        format!("Could not toggle {name} - {e}"),
                                    );
                                }
                            });
                        }
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mod_update(move |id| {
            let id = id.to_string();
            let file = STATE.lock().unwrap().updates.get(&id).cloned().flatten();
            let (Some(file), Some(m)) = (file, Self::mod_by_id(&id)) else {
                return;
            };
            let Some(source) = m.source else { return };

            gbbrowser::GbBrowserHandler::download_update(&ww, source, file, m.path);
        });

        let ww = window.clone();
        w.on_mods_toggle_all(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let mods = STATE.lock().unwrap().displayed.clone();
                for m in &mods {
                    match ModManager::toggle_mod(m) {
                        Ok(()) => Self::note_toggle(&mod_id(m)),
                        Err(e) => {
                            if m.path.exists() {
                                error!("could not toggle '{}': {e}", m.folder_name);
                            } else {
                                note_missing("could not toggle", m);
                            }
                        }
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mod_delete(move |id| {
            let id = id.to_string();
            let ww = ww.clone();
            std::thread::spawn(move || {
                if let Some(m) = Self::mod_by_id(&id) {
                    match std::fs::remove_dir_all(&m.path) {
                        Ok(()) => {
                            info!("deleted '{}'", m.path.display());
                            config_map_set(key::MODMNG_NOTES, &id, None);
                            config_map_set(key::MODMNG_DISPLAY_NAMES, &id, None);
                            purge_icon_backup(&m.path);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            note_missing("could not delete", &m);
                            config_map_set(key::MODMNG_NOTES, &mod_id(&m), None);
                            config_map_set(key::MODMNG_DISPLAY_NAMES, &mod_id(&m), None);
                            purge_icon_backup(&m.path);
                        }
                        Err(e) => {
                            error!("could not delete '{}': {e}", m.path.display());
                        }
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mod_rename(move |id, new_name| {
            let Some(win) = ww.upgrade() else { return };
            let Some(m) = Self::mod_by_id(&id) else {
                return;
            };
            let name = new_name.trim();

            config_map_set(
                key::MODMNG_DISPLAY_NAMES,
                &id,
                (!name.is_empty()).then_some(name),
            );

            let shown = if name.is_empty() {
                &m.display_name
            } else {
                name
            };

            let icon = mod_icon(&m, shown);
            Self::update_row(&win, &id, |row| {
                row.name = shown.into();
                row.icon = icon.clone();
            });

            let display_names = config_map(key::MODMNG_DISPLAY_NAMES);
            let shown_name = |m: &Mod| -> String {
                display_names
                    .get(&mod_id(m))
                    .and_then(|v| v.as_str())
                    .unwrap_or(&m.display_name)
                    .to_string()
            };
            Self::refresh_filter_options(&win, &shown_name);
        });

        let ww = window.clone();
        w.on_mod_set_notes(move |id, notes| {
            let Some(win) = ww.upgrade() else { return };
            if Self::mod_by_id(&id).is_none() {
                return;
            }
            let notes = notes.trim().to_string();

            config_map_set(
                key::MODMNG_NOTES,
                &id,
                (!notes.is_empty()).then_some(&notes),
            );
            Self::update_row(&win, &id, |row| row.notes = notes.as_str().into());
        });

        let ww = window.clone();
        w.on_mod_select(move |id| {
            let Some(win) = ww.upgrade() else { return };
            let id = id.to_string();

            let (selected, group_state) = {
                let mut state = STATE.lock().unwrap();
                let selected = if state.selected.remove(&id) {
                    false
                } else {
                    state.selected.insert(id.clone());
                    true
                };

                let group_state = state
                    .displayed
                    .iter()
                    .find(|m| mod_id(m) == id)
                    .map(Self::current_zone)
                    .filter(|zone| !zone.is_empty())
                    .map(|zone| {
                        let all = state
                            .displayed
                            .iter()
                            .filter(|m| Self::current_zone(m) == zone)
                            .all(|m| state.selected.contains(&mod_id(m)));
                        (zone, all)
                    });
                drop(state);
                (selected, group_state)
            };

            Self::update_row(&win, &id, |row| row.selected = selected);
            if let Some((zone, all)) = group_state {
                Self::update_row(&win, &zone, |row| row.selected = all);
            }
            Self::update_selection_props(&win);
        });

        let ww = window.clone();
        w.on_mods_select_all(move || {
            let Some(win) = ww.upgrade() else { return };

            {
                let mut state = STATE.lock().unwrap();
                let all =
                    !state.displayed.is_empty() && state.selected.len() == state.displayed.len();
                if all {
                    state.selected.clear();
                } else {
                    state.selected = state.displayed.iter().map(mod_id).collect();
                }
            }

            Self::rebuild(&win);
        });

        let ww = window.clone();
        w.on_mods_toggle_selected(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let mods: Vec<Mod> = {
                    let state = STATE.lock().unwrap();
                    state
                        .displayed
                        .iter()
                        .filter(|m| state.selected.contains(&mod_id(m)))
                        .cloned()
                        .collect()
                };
                for m in &mods {
                    match ModManager::toggle_mod(m) {
                        Ok(()) => Self::note_toggle(&mod_id(m)),
                        Err(e) => {
                            if m.path.exists() {
                                error!("could not toggle '{}': {e}", m.folder_name);
                            } else {
                                note_missing("could not toggle", m);
                            }
                        }
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mods_delete_selected(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let mods: Vec<Mod> = {
                    let state = STATE.lock().unwrap();
                    state
                        .displayed
                        .iter()
                        .filter(|m| state.selected.contains(&mod_id(m)))
                        .cloned()
                        .collect()
                };
                for m in &mods {
                    match std::fs::remove_dir_all(&m.path) {
                        Ok(()) => {
                            info!("deleted '{}'", m.path.display());
                            config_map_set(key::MODMNG_NOTES, &mod_id(m), None);
                            config_map_set(key::MODMNG_DISPLAY_NAMES, &mod_id(m), None);
                            purge_icon_backup(&m.path);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            note_missing("could not delete", m);
                            config_map_set(key::MODMNG_NOTES, &mod_id(m), None);
                            config_map_set(key::MODMNG_DISPLAY_NAMES, &mod_id(m), None);
                            purge_icon_backup(&m.path);
                        }
                        Err(e) => {
                            error!("could not delete '{}': {e}", m.path.display());
                        }
                    }
                }
                STATE.lock().unwrap().selected.clear();
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mods_search_changed(move |text| {
            let Some(win) = ww.upgrade() else { return };
            STATE.lock().unwrap().search = text.trim().to_lowercase();
            Self::rebuild(&win);
        });

        let ww = window.clone();
        w.on_mods_filters_changed(move |filters| {
            let Some(win) = ww.upgrade() else { return };

            let characters = Self::checked_ids(&win.get_mods_filter_characters());
            let authors = Self::checked_ids(&win.get_mods_filter_authors());

            {
                let mut state = STATE.lock().unwrap();
                state.filter = filters.status;
                state.filter_group = filters.group_id.to_string();
                state.filter_characters = characters;
                state.filter_authors = authors;
            }

            Self::rebuild(&win);
        });

        let ww = window.clone();
        w.on_mods_clear_filters(move || {
            let Some(win) = ww.upgrade() else { return };

            {
                let mut state = STATE.lock().unwrap();
                state.filter = ModStatusFilter::All;
                state.filter_group = String::new();
                state.filter_characters.clear();
                state.filter_authors.clear();
            }

            win.set_mods_filters(ModFilters {
                status: ModStatusFilter::All,
                group_id: "".into(),
            });
            Self::rebuild(&win);
        });

        let ww = window.clone();
        w.on_mod_group_create(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let mut name = "New Group".to_string();
                let mut counter = 1;
                while Self::group_path(&name).is_some_and(|p| p.exists()) {
                    counter += 1;
                    name = format!("New Group {counter}");
                }

                match Self::group_path(&name) {
                    Some(path) => {
                        if let Err(e) = std::fs::create_dir_all(&path) {
                            error!("could not create group '{name}': {e}");
                        } else {
                            info!("created group '{name}'");
                            STATE.lock().unwrap().pending_edit_group =
                                Some(path.to_string_lossy().into_owned());
                        }
                    }
                    None => error!("could not create group: no mods path"),
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mod_group_rename(move |id, new_name| {
            let ww = ww.clone();
            let old_path = PathBuf::from(id.to_string());
            let name = new_name.trim().to_string();

            if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
                warn!("invalid group name '{name}', ignoring");
                return;
            }

            std::thread::spawn(move || {
                let Some(new_path) = Self::group_path(&name) else {
                    return;
                };
                if new_path == old_path {
                    return;
                }
                if new_path.exists() {
                    warn!("group '{name}' already exists, ignoring rename");
                } else if let Err(e) = std::fs::rename(&old_path, &new_path) {
                    error!("could not rename group '{}': {e}", old_path.display());
                } else {
                    rekey_mod_config(&old_path.to_string_lossy(), &new_path.to_string_lossy());

                    if let Ok(entries) = new_path.read_dir() {
                        for entry in entries.flatten() {
                            rekey_icon_backup(&old_path.join(entry.file_name()), &entry.path());
                        }
                    }

                    let mut state = STATE.lock().unwrap();
                    if state
                        .collapsed
                        .remove(&old_path.to_string_lossy().into_owned())
                    {
                        state
                            .collapsed
                            .insert(new_path.to_string_lossy().into_owned());
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mod_group_delete(move |id| {
            let ww = ww.clone();
            let group_path = PathBuf::from(id.to_string());
            std::thread::spawn(move || {
                Self::delete_group_folder(&group_path);
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mods_delete_selected_groups(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let ids: Vec<String> = {
                    let mut state = STATE.lock().unwrap();
                    state.selected_groups.drain().collect()
                };
                for id in &ids {
                    Self::delete_group_folder(Path::new(id));
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mod_group_toggle(move |id| {
            let ww = ww.clone();
            let id = id.to_string();
            std::thread::spawn(move || {
                let mods: Vec<Mod> = {
                    let state = STATE.lock().unwrap();
                    state
                        .scanned
                        .iter()
                        .find(|g| g.id == id)
                        .map(|g| g.mods.clone())
                        .unwrap_or_default()
                };

                let all_enabled = !mods.is_empty() && mods.iter().all(|m| m.is_enabled);
                for m in &mods {
                    if m.is_enabled == all_enabled {
                        match ModManager::toggle_mod(m) {
                            Ok(()) => Self::note_toggle(&mod_id(m)),
                            Err(e) => {
                                if m.path.exists() {
                                    error!("could not toggle '{}': {e}", m.folder_name);
                                } else {
                                    note_missing("could not toggle", m);
                                }
                            }
                        }
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mod_group_select(move |id| {
            let Some(win) = ww.upgrade() else { return };
            let id = id.to_string();

            {
                let mut state = STATE.lock().unwrap();

                if state
                    .scanned
                    .iter()
                    .any(|g| g.id == id && g.mods.is_empty())
                {
                    if !state.selected_groups.remove(&id) {
                        state.selected_groups.insert(id);
                    }
                    drop(state);
                    Self::rebuild(&win);
                    return;
                }

                let group_ids: Vec<String> = state
                    .displayed
                    .iter()
                    .filter(|m| Self::current_zone(m) == id)
                    .map(mod_id)
                    .collect();
                if group_ids.is_empty() {
                    return;
                }

                let all_selected = group_ids.iter().all(|i| state.selected.contains(i));
                if all_selected {
                    for i in &group_ids {
                        state.selected.remove(i);
                    }
                } else {
                    state.selected.extend(group_ids);
                }
            }

            Self::rebuild(&win);
        });

        let ww = window.clone();
        w.on_mod_group_collapse(move |id| {
            let Some(win) = ww.upgrade() else { return };
            let id = id.to_string();
            {
                let mut state = STATE.lock().unwrap();
                if !state.collapsed.remove(&id) {
                    state.collapsed.insert(id);
                }
            }
            Self::rebuild(&win);
        });

        let ww = window.clone();
        w.on_mod_move_to_group(move |id, group_id| {
            Self::move_mods_to_zone(&ww, vec![id.to_string()], group_id.to_string());
        });

        let ww = window.clone();
        w.on_mods_move_selected_to_group(move |group_id| {
            let ids = Self::selected_ids();
            if ids.is_empty() {
                return;
            }
            Self::move_mods_to_zone(&ww, ids, group_id.to_string());
        });

        let ww = window.clone();
        w.on_mods_create_group_with_selected(move |name| {
            let Some(win) = ww.upgrade() else { return };
            let name = name.trim().to_string();

            if name.is_empty() || INVALID_FILENAME_CHARS.iter().any(|c| name.contains(*c)) {
                Self::show_toast(&win, "error", format!("'{name}' is not a valid group name"));
                return;
            }

            let Some(path) = Self::group_path(&name) else {
                Self::show_toast(
                    &win,
                    "error",
                    "Could not create group - no mods folder".into(),
                );
                return;
            };
            if path.exists() {
                Self::show_toast(
                    &win,
                    "error",
                    format!("A group named '{name}' already exists"),
                );
                return;
            }

            let ids = Self::selected_ids();
            if ids.is_empty() {
                return;
            }

            if let Err(e) = std::fs::create_dir_all(&path) {
                error!("could not create group '{name}': {e}");
                Self::show_toast(
                    &win,
                    "error",
                    format!("Could not create group '{name}' - {e}"),
                );
                return;
            }
            info!("created group '{name}'");

            Self::move_mods_to_zone(&ww, ids, path.to_string_lossy().into_owned());
        });

        let ww = window.clone();
        w.on_mod_open_add_existing(move |group_id| {
            if let Some(win) = ww.upgrade() {
                Self::open_add_existing(&win, &group_id);
            }
        });

        let ww = window.clone();
        w.on_mods_add_existing_search_changed(move |text| {
            let Some(win) = ww.upgrade() else { return };
            {
                let mut state = STATE.lock().unwrap();
                let Some(picker) = state.add_existing.as_mut() else {
                    return;
                };
                picker.search = text.trim().to_lowercase();

                for option in win.get_mods_add_existing_options().iter() {
                    if option.selected {
                        picker.selected.insert(option.id.to_string());
                    } else {
                        picker.selected.remove(option.id.as_str());
                    }
                }
                drop(state);
            }
            Self::rebuild_add_existing_model(&win);
        });

        let ww = window.clone();
        w.on_mods_add_existing_confirm(move |group_id| {
            let Some(win) = ww.upgrade() else { return };
            let ids: Vec<String> = win
                .get_mods_add_existing_options()
                .iter()
                .filter(|option| option.selected)
                .map(|option| option.id.to_string())
                .collect();
            win.set_mods_add_existing_open(false);
            if ids.is_empty() {
                return;
            }
            STATE.lock().unwrap().add_existing = None;
            Self::add_existing_mods(&ww, group_id.to_string(), ids);
        });

        let ww = window.clone();
        w.on_mod_drag_moved(move |id, content_y| {
            let Some(win) = ww.upgrade() else { return };

            let target = Self::zone_at(content_y).map_or_else(String::new, |zone| {
                let source = Self::mod_by_id(&id)
                    .map(|m| Self::current_zone(&m))
                    .unwrap_or_default();
                if zone == source { String::new() } else { zone }
            });

            if win.get_mods_drag_target() != target.as_str() {
                win.set_mods_drag_target(target.into());
            }
        });

        let ww = window.clone();
        w.on_mod_drag_dropped(move |id, content_y| {
            if let Some(win) = ww.upgrade() {
                win.set_mods_drag_target("".into());
            }

            let Some(zone) = Self::zone_at(content_y) else {
                return;
            };

            Self::drop_mods_on_zone(&ww, id.to_string(), zone);
        });

        let ww = window.clone();
        w.on_mod_drop_on_zone(move |id, zone| {
            Self::drop_mods_on_zone(&ww, id.to_string(), zone.to_string());
        });

        w.on_mod_open_link(move |id| {
            let Some(m) = Self::mod_by_id(&id) else {
                return;
            };
            let Some(link) = m.support_link else { return };
            if link.is_empty() {
                return;
            }
            if let Err(e) = open::that(&link) {
                error!("could not open support link '{link}': {e}");
            }
        });

        let ww = window.clone();
        w.on_mods_refresh(move || {
            {
                STATE.lock().unwrap().updates.clear();
            }
            Self::reload(&ww);
        });

        // [ICON PICKER]

        let ww = window.clone();
        w.on_mods_pick_icon_character(move |id, slug| {
            let id = id.to_string();
            let slug = slug.to_string();
            let ww = ww.clone();
            std::thread::spawn(move || {
                let Some(m) = Self::mod_by_id(&id) else {
                    return;
                };

                let outcome = characters::icon_bytes(&slug)
                    .ok_or_else(|| anyhow!("no character icon matches '{slug}'"))
                    .and_then(|bytes| write_icon(&m, bytes));

                if let Err(e) = outcome {
                    report_icon_error(&ww, &m, &e);
                } else {
                    info!("set the icon of '{}' to '{slug}'", m.folder_name);
                }

                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mods_browse_icon(move |id| {
            let id = id.to_string();
            let ww = ww.clone();
            std::thread::spawn(move || {
                let Some(m) = Self::mod_by_id(&id) else {
                    return;
                };

                let Some(picked) = rfd::FileDialog::new()
                    .set_title("Choose an Icon")
                    .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
                    .pick_file()
                else {
                    return;
                };

                let outcome = std::fs::read(&picked)
                    .with_context(|| format!("could not read '{}'", picked.display()))
                    .and_then(|bytes| encode_icon_png(&bytes))
                    .and_then(|png| write_icon(&m, &png));

                if let Err(e) = outcome {
                    report_icon_error(&ww, &m, &e);
                } else {
                    info!(
                        "set the icon of '{}' from '{}'",
                        m.folder_name,
                        picked.display()
                    );
                }

                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mods_remove_icon(move |id| {
            let id = id.to_string();
            let ww = ww.clone();
            std::thread::spawn(move || {
                let Some(m) = Self::mod_by_id(&id) else {
                    return;
                };
                let icon = m.path.join("icon.png");
                let backup = icon_backup_path(&m.path);

                let outcome = if backup.is_file() {
                    std::fs::copy(&backup, &icon)
                        .map(|_| ())
                        .and_then(|()| std::fs::remove_file(&backup))
                        .with_context(|| {
                            format!("could not restore the icon of '{}'", m.path.display())
                        })
                } else {
                    match std::fs::remove_file(&icon) {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                        other => other,
                    }
                    .with_context(|| format!("could not remove '{}'", icon.display()))
                };

                if let Err(e) = outcome {
                    report_icon_error(&ww, &m, &e);
                } else {
                    info!("removed the custom icon of '{}'", m.folder_name);
                }
                Self::reload(&ww);
            });
        });

        w.on_open_mods_folder(move || {
            let Some(folder) = get_mods_path() else {
                return;
            };
            let _ = std::fs::create_dir_all(&folder);
            if let Err(e) = open_folder(&folder) {
                error!("could not open mods folder '{}': {e}", folder.display());
            }
        });

        let ww = window.clone();
        w.on_mods_install_archive(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let picked = rfd::FileDialog::new()
                    .set_title("Select Mod Archives")
                    .add_filter("Archives", &ARCHIVE_EXTENSIONS)
                    .pick_files();

                if let Some(files) = picked {
                    Self::install_paths(&ww, files);
                }
            });
        });

        let ww = window.clone();
        w.on_mods_install_folder(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let picked = rfd::FileDialog::new()
                    .set_title("Select Mod Folders")
                    .pick_folders();

                if let Some(folders) = picked {
                    Self::install_paths(&ww, folders);
                }
            });
        });

        info!("bind() complete");
    }

    fn update_row(w: &MainWindow, id: &str, change: impl Fn(&mut ModItem)) {
        let model = w.get_mods();
        for i in 0..model.row_count() {
            if let Some(mut row) = model.row_data(i)
                && row.id == id
            {
                change(&mut row);
                model.set_row_data(i, row);
                break;
            }
        }

        let sections = w.get_mods_grid();
        for s in 0..sections.row_count() {
            let Some(section) = sections.row_data(s) else {
                continue;
            };
            for i in 0..section.row_count() {
                if let Some(mut row) = section.row_data(i)
                    && row.id == id
                {
                    change(&mut row);
                    section.set_row_data(i, row);
                    return;
                }
            }
        }
    }
}
