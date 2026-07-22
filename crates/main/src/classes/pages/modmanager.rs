use crate::classes::pages::addons::ARCHIVE_EXTENSIONS;
use crate::{MainWindow, ModItem};

use anyhow::{anyhow, Context, Result};
use archive::{ArchiveExtractor, ArchiveFormat};
use log::*;
use once_cell::sync::Lazy;
use serde_json::Value;
use shared::config;
use shared::utils::{get_mods_path, read_dir_recursive};
use slint::{Model, ModelRc, VecModel};
use unrar::Archive as RarArchive;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

const GROUP_PREFIX: &str = "AU GRP - ";
const NOTES_KEY: &str = "mod_notes";
const DISPLAY_NAMES_KEY: &str = "mod_display_names";
const VIEW_GRID_KEY: &str = "mod_view_grid";

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
    // TODO: used once mod icons are loaded into the cards
    #[allow(dead_code)]
    pub icon: Option<String>,
    pub is_enabled: bool,
    pub has_json: bool,
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
            is_enabled: false,
            has_json: false,
        }
    }
}

pub struct ModManager;

impl ModManager {
    fn get_mod_data(folder: &PathBuf) -> Option<Mod> {
        let mod_name = folder.file_name()?.to_str()?;

        let files = read_dir_recursive(folder);
        let is_enabled = files
            .iter()
            .filter(|p| {
                Path::new(p.file_name().to_str().unwrap())
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
            })
            .count()
            > 0;

        let mut mod_data = Mod {
            folder_name: mod_name.to_string(),
            display_name: mod_name.to_string().replace("_P", ""),
            path: folder.clone(),
            is_enabled,
            ..Default::default()
        };

        let mut json_path = folder.join("mod.json");
        if !json_path.exists() {
            for sub in folder.read_dir().ok()? {
                let sub = sub.ok()?;
                if sub.file_type().ok()?.is_dir() && sub.path().join("mod.json").exists() {
                    json_path = sub.path().join("mod.json");
                    break;
                }
            }

            if !json_path.exists() {
                return Some(mod_data);
            }
        }

        let json = std::fs::read_to_string(json_path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&json).ok()?;
        mod_data.has_json = true;

        let binding = serde_json::Map::new();
        let optionals = json["Optionals"].as_object().unwrap_or(&binding);

        let mut support_link = optionals
            .iter()
            .find(|(k, _)| *k.to_lowercase() == *"support link")
            .and_then(|(_, v)| v.as_str().map(ToString::to_string));
        if let Some(link) = &mut support_link {
            if !link.starts_with("http://") && !link.starts_with("https://") {
                support_link = format!("https://{link}").into();
            }
        }

        let mut icon = json["Icon"].as_str().map(ToString::to_string);
        let mut custom_image_url = optionals
            .iter()
            .find(|(k, _)| *k.to_lowercase() == *"custom image url")
            .and_then(|(_, v)| v.as_str().map(ToString::to_string));
        if custom_image_url.is_some() {
            let url = custom_image_url.as_mut().unwrap();
            if !url.starts_with("http://") && !url.starts_with("https://") {
                custom_image_url = format!("https://{url}").into();
            }
            icon = custom_image_url;
        }
        Some(Mod {
            display_name: json["Name"]
                .as_str()
                .or(Some(&mod_data.display_name))
                .map(ToString::to_string)?,
            version: json["Version"]
                .as_str()
                .or(Some("1.0.0"))
                .map(ToString::to_string),
            author: json["Author"]
                .as_str()
                .or(Some("Unknown"))
                .map(ToString::to_string),
            support_link,
            icon,
            ..mod_data
        })
    }

    fn contains_pak(folder: &PathBuf) -> bool {
        read_dir_recursive(folder).iter().any(|item| {
            item.file_name().to_str().is_some_and(|name| {
                Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
                    || name.ends_with(".pak.disabled")
            })
        })
    }

    pub fn scan_mods() -> Option<Vec<Group>> {
        let mods_path = get_mods_path()?;

        if !mods_path.exists() {
            return Some(vec![]);
        }

        let mut groups: Vec<Group> = vec![];

        // Mods that don't have a group
        let mut root_group = Group::new(None, None);

        for entry in mods_path.read_dir().ok()? {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_dir() {
                continue;
            }

            if entry.file_name().to_str()?.starts_with(GROUP_PREFIX) {
                let group_name = entry.file_name().to_str()?.replace(GROUP_PREFIX, "");
                let mut group = Group::new(Some(group_name), Some(entry.path()));
                for sub in entry.path().read_dir().ok()? {
                    let sub = sub.ok()?;
                    if sub.file_type().ok()?.is_dir() && Self::contains_pak(&sub.path()) {
                        group.add_mod(Self::get_mod_data(&sub.path())?);
                    }
                }
                if !group.mods.is_empty() {
                    group.mods.sort_by(|a, b| a.folder_name.cmp(&b.folder_name));
                }
                groups.push(group);
            } else if Self::contains_pak(&entry.path()) {
                root_group.add_mod(Self::get_mod_data(&entry.path())?);
            }
        }

        if !root_group.mods.is_empty() {
            root_group
                .mods
                .sort_by(|a, b| a.folder_name.cmp(&b.folder_name));

            groups.insert(0, root_group);
        }

        Some(groups)
    }

    pub fn toggle_mod(mod_: &Mod) -> Result<()> {
        let folder = mod_.path.clone();
        if !folder.exists() {
            return Err(anyhow!(
                "Cannot toggle mod: folder not found for {}",
                mod_.folder_name
            ));
        }
        let files = read_dir_recursive(&folder);

        if mod_.is_enabled {
            let targets = files
                .iter()
                .filter(|p| {
                    Path::new(p.file_name().to_str().unwrap())
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
                })
                .collect::<Vec<_>>();

            if targets.is_empty() {
                return Err(anyhow!(
                    "Cannot toggle mod: no .pak files found for {}",
                    mod_.folder_name
                ));
            }

            for pak in &targets {
                let old = pak.path();
                let new = old.with_file_name(format!(
                    "{}.disabled",
                    pak.file_name()
                        .to_str()
                        .ok_or_else(|| anyhow!("Could not get file name"))?
                ));
                std::fs::rename(&old, &new)?;
            }
            trace!(
                "Mod disabled: renamed {} file(s) in {}",
                targets.len(),
                mod_.folder_name
            );
        } else {
            let targets = files
                .iter()
                .filter(|p| p.file_name().to_str().unwrap().ends_with(".pak.disabled"))
                .collect::<Vec<_>>();

            if targets.is_empty() {
                return Err(anyhow!(
                    "Cannot toggle mod: no .pak.disabled files found for {}",
                    mod_.folder_name
                ));
            }

            for pak in &targets {
                let old = pak.path();
                let new = old.with_file_name(
                    old.file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .replace(".disabled", ""),
                );
                std::fs::rename(&old, &new)?;
            }
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

#[derive(Default)]
struct State {
    scanned: Vec<ScannedGroup>,
    displayed: Vec<Mod>,
    rows: Vec<(bool, String)>,
    selected: HashSet<String>,
    collapsed: HashSet<String>,
    search: String,
    // TODO: Make this an enum
    filter: i32, // 0 = all, 1 = enabled only, 2 = disabled only
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

pub fn config_map(key: &str) -> serde_json::Map<String, Value> {
    config::get(key).as_object().cloned().unwrap_or_default()
}

pub fn config_map_set(key: &str, entry: &str, value: Option<&str>) {
    let mut map = config_map(key);
    match value {
        Some(v) => {
            map.insert(entry.to_string(), Value::from(v));
        }
        None => {
            map.remove(entry);
        }
    }
    config::set(key, Value::Object(map));
}

fn mod_id(mod_: &Mod) -> String {
    mod_.path.to_string_lossy().into_owned()
}

pub struct ModManagerHandler;

impl ModManagerHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("[ModManager] setup() called");
        Self::bind(window);
        Self::setup_file_drop(window);
        Self::reload(window);
        info!("[ModManager] setup() complete");
    }

    // On Windows the app runs elevated and UIPI blocks OLE drag & drop
    // from the non-elevated Explorer, so winit's HoveredFile/DroppedFile
    // events never fire. This works around that with the legacy WM_DROPFILES
    // protocol instead.
    #[cfg(target_os = "windows")]
    fn setup_file_drop(window: &slint::Weak<MainWindow>) {
        crate::classes::filedrop::setup(window);
    }

    // TODO: This is not yet tested on linux
    #[cfg(not(target_os = "windows"))]
    fn setup_file_drop(window: &slint::Weak<MainWindow>) {
        use i_slint_backend_winit::winit::event::WindowEvent;
        use i_slint_backend_winit::WinitWindowAccessor;
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

    fn show_toast(w: &MainWindow, kind: &str, text: String) {
        w.set_toast_text(text.into());
        w.set_toast_kind(kind.into());
        w.set_toast_active(true);
    }

    pub(crate) fn install_paths(window: &slint::Weak<MainWindow>, paths: Vec<PathBuf>) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let mut installed: Vec<String> = Vec::new();
            let mut failed: Vec<String> = Vec::new();

            for path in &paths {
                match Self::install_path(path) {
                    Ok(name) => {
                        info!("[ModManager] installed '{name}' from '{}'", path.display());
                        installed.push(name);
                    }
                    Err(e) => {
                        error!("[ModManager] could not install '{}': {e}", path.display());
                        failed.push(format!(
                            "{}: {e}",
                            path.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        ));
                    }
                }
            }

            let ww2 = ww.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(win) = ww2.upgrade() else { return };
                if let Some(err) = failed.first() {
                    Self::show_toast(&win, "error", format!("Install failed - {err}"));
                } else if installed.len() == 1 {
                    Self::show_toast(&win, "success", format!("Installed {}", installed[0]));
                } else if !installed.is_empty() {
                    Self::show_toast(
                        &win,
                        "success",
                        format!("Installed {} mods", installed.len()),
                    );
                }
            });

            Self::reload(&ww);
        });
    }

    fn install_path(path: &Path) -> Result<String> {
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
            if target.exists() {
                return Err(anyhow!("a mod with this name already exists"));
            }
            Self::copy_dir_recursive(path, &target)?;
            return Ok(name);
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let target = mods_path.join(&name);
        if target.exists() {
            return Err(anyhow!("a mod with this name already exists"));
        }

        if ext == "pak" || ext == "utoc" || ext == "ucas" {
            std::fs::create_dir_all(&target)?;
            std::fs::copy(
                path,
                target.join(path.file_name().with_context(|| "invalid file name")?),
            )?;
        } else if ext == "rar" {
            std::fs::create_dir_all(&target)?;
            let archive = RarArchive::new(path).open_for_processing()?;
            archive.extract_all(&target)?;
        } else if ARCHIVE_EXTENSIONS.contains(&ext.as_str()) {
            let data = std::fs::read(path)?;
            // Max file size is 200MB
            let extractor = ArchiveExtractor::new().with_max_file_size(200 * 1024 * 1024);
            let files = match ext.as_str() {
                "zip" => extractor.extract(&data, ArchiveFormat::Zip)?,
                "7z" => extractor.extract(&data, ArchiveFormat::SevenZ)?,
                "tar" => extractor.extract(&data, ArchiveFormat::Tar)?,
                "gz" => extractor.extract(&data, ArchiveFormat::Gz)?,
                "bz2" => extractor.extract(&data, ArchiveFormat::Bz2)?,
                "xz" => extractor.extract(&data, ArchiveFormat::Xz)?,
                "zst" => extractor.extract(&data, ArchiveFormat::Zst)?,
                "lz4" => extractor.extract(&data, ArchiveFormat::Lz4)?,
                _ => return Err(anyhow!("unsupported archive format")),
            };

            std::fs::create_dir_all(&target)?;
            for file in files {
                let dest = target.join(&file.path);
                if file.is_directory {
                    std::fs::create_dir_all(&dest)?;
                } else {
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    match std::fs::write(&dest, file.data) {
                        Ok(()) => (),
                        Err(e) => return Err(anyhow!("could not write file: {e}")),
                    }
                }
            }
        } else {
            return Err(anyhow!("unsupported file type '.{ext}'"));
        }

        Ok(name)
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

    fn reload(window: &slint::Weak<MainWindow>) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let groups = ModManager::scan_mods().unwrap_or_default();

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
                let Some(w) = ww.upgrade() else {
                    error!("[ModManager] could not load: window handle is dead");
                    return;
                };

                STATE.lock().unwrap().scanned = scanned;
                Self::rebuild(&w);
            });
        });
    }

    fn rebuild(w: &MainWindow) {
        let display_names = config_map(DISPLAY_NAMES_KEY);
        let notes = config_map(NOTES_KEY);

        let shown_name = |m: &Mod| -> String {
            display_names
                .get(&m.folder_name)
                .and_then(|v| v.as_str())
                .unwrap_or(&m.display_name)
                .to_string()
        };

        let (items, grid_sections, selected_count, all_selected) = {
            let mut state = STATE.lock().unwrap();
            let mut items: Vec<ModItem> = Vec::new();
            let mut grid_sections: Vec<Vec<ModItem>> = Vec::new();
            let mut displayed: Vec<Mod> = Vec::new();
            let mut rows: Vec<(bool, String)> = Vec::new();
            let searching = !state.search.is_empty();

            for group in state.scanned.clone() {
                let is_root = group.id.is_empty();
                // A search hit on the group name shows the whole group
                let group_matches =
                    !is_root && searching && group.name.to_lowercase().contains(&state.search);

                let visible: Vec<&Mod> = group
                    .mods
                    .iter()
                    .filter(|m| {
                        let matches_search = !searching
                            || group_matches
                            || shown_name(m).to_lowercase().contains(&state.search)
                            || m.folder_name.to_lowercase().contains(&state.search);
                        let matches_filter = match state.filter {
                            1 => m.is_enabled,
                            2 => !m.is_enabled,
                            _ => true,
                        };
                        matches_search && matches_filter
                    })
                    .collect();

                let collapsed = !is_root && state.collapsed.contains(&group.id);
                let mut section: Vec<ModItem> = Vec::new();

                if !is_root {
                    if (searching || state.filter != 0) && visible.is_empty() && !group_matches {
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
                        // Checked when every visible mod in the group is selected
                        selected: !visible.is_empty()
                            && visible.iter().all(|m| state.selected.contains(&mod_id(m))),
                        has_json: false,
                        is_editing: false,
                        group_id: "".into(),
                        support_link: "".into(),
                        is_group_header: true,
                        collapsed,
                    };
                    items.push(header.clone());
                    section.push(header);
                    rows.push((true, group.id.clone()));
                }

                for m in visible {
                    displayed.push(m.clone());
                    if !collapsed {
                        // Hide the "Unknown" placeholder so mods without a
                        // mod.json don't show a meaningless version chip
                        let version = m
                            .version
                            .clone()
                            .filter(|v| v != "Unknown")
                            .unwrap_or_default();
                        let item = ModItem {
                            id: mod_id(m).into(),
                            name: shown_name(m).into(),
                            author: m.author.clone().unwrap_or_default().into(),
                            version: version.into(),
                            icon: slint::Image::default(),
                            notes: notes
                                .get(&m.folder_name)
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

            let existing: HashSet<String> = displayed.iter().map(mod_id).collect();
            state.selected.retain(|id| existing.contains(id));
            state.displayed = displayed;
            state.rows = rows;

            let count = state.selected.len();
            let all = !state.displayed.is_empty() && count == state.displayed.len();
            drop(state);

            (items, grid_sections, count, all)
        };

        w.set_mods(Rc::new(VecModel::from(items)).into());
        let sections: Vec<ModelRc<ModItem>> = grid_sections
            .into_iter()
            .map(|s| ModelRc::from(Rc::new(VecModel::from(s))))
            .collect();
        w.set_mods_grid(Rc::new(VecModel::from(sections)).into());
        w.set_mods_selected_count(i32::try_from(selected_count).unwrap_or(0));
        w.set_mods_all_selected(all_selected);
    }

    fn update_selection_props(w: &MainWindow) {
        let state = STATE.lock().unwrap();
        let count = state.selected.len();
        let all = !state.displayed.is_empty() && count == state.displayed.len();
        drop(state);
        w.set_mods_selected_count(i32::try_from(count).unwrap_or(0));
        w.set_mods_all_selected(all);
    }

    /// Creates a unique group folder path for `name` inside the mods dir
    fn group_path(name: &str) -> Option<PathBuf> {
        get_mods_path().map(|p| p.join(format!("{GROUP_PREFIX}{name}")))
    }

    // Must match the list layout in modmanager.slint
    const LIST_PADDING_TOP: f32 = 12.0;
    const LIST_SPACING: f32 = 8.0;
    // 40px header + 12px top margin separating groups (see modmanager.slint)
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

    // Dragging a selected mod moves the whole selection, otherwise just that mod
    fn drop_mods_on_zone(window: &slint::Weak<MainWindow>, id: String, zone: String) {
        // Dropping onto the group the mod is already in moves it back to root
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
                    warn!(
                        "[ModManager] not moving '{}': target already exists",
                        m.folder_name
                    );
                } else if let Err(e) = std::fs::rename(&m.path, &target) {
                    error!("[ModManager] could not move '{}': {e}", m.folder_name);
                } else {
                    info!(
                        "[ModManager] moved '{}' → '{}'",
                        m.folder_name,
                        target_dir.display()
                    );
                }
            }
            Self::reload(&ww);
        });
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

        w.set_mods_view_grid(config::get(VIEW_GRID_KEY).as_bool().unwrap_or(false));
        w.on_mods_view_changed(|grid| {
            config::set(VIEW_GRID_KEY, Value::from(grid));
        });

        let ww = window.clone();
        w.on_mod_toggle(move |id| {
            let id = id.to_string();
            let ww = ww.clone();
            std::thread::spawn(move || {
                if let Some(m) = Self::mod_by_id(&id) {
                    if let Err(e) = ModManager::toggle_mod(&m) {
                        error!("[ModManager] could not toggle '{}': {e}", m.folder_name);
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_mods_toggle_all(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let mods = STATE.lock().unwrap().displayed.clone();
                for m in &mods {
                    if let Err(e) = ModManager::toggle_mod(m) {
                        error!("[ModManager] could not toggle '{}': {e}", m.folder_name);
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
                            info!("[ModManager] deleted '{}'", m.path.display());
                            // Drop leftover per-mod config entries
                            config_map_set(NOTES_KEY, &m.folder_name, None);
                            config_map_set(DISPLAY_NAMES_KEY, &m.folder_name, None);
                        }
                        Err(e) => {
                            error!("[ModManager] could not delete '{}': {e}", m.path.display());
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
                DISPLAY_NAMES_KEY,
                &m.folder_name,
                (!name.is_empty()).then_some(name),
            );

            let shown = if name.is_empty() {
                &m.display_name
            } else {
                name
            };
            Self::update_row(&win, &id, |row| row.name = shown.into());
        });

        let ww = window.clone();
        w.on_mod_set_notes(move |id, notes| {
            let Some(win) = ww.upgrade() else { return };
            let Some(m) = Self::mod_by_id(&id) else {
                return;
            };
            let notes = notes.trim().to_string();

            config_map_set(
                NOTES_KEY,
                &m.folder_name,
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
                    if let Err(e) = ModManager::toggle_mod(m) {
                        error!("[ModManager] could not toggle '{}': {e}", m.folder_name);
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
                            info!("[ModManager] deleted '{}'", m.path.display());
                            config_map_set(NOTES_KEY, &m.folder_name, None);
                            config_map_set(DISPLAY_NAMES_KEY, &m.folder_name, None);
                        }
                        Err(e) => {
                            error!("[ModManager] could not delete '{}': {e}", m.path.display());
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
        w.on_mods_filter_changed(move |index| {
            let Some(win) = ww.upgrade() else { return };
            STATE.lock().unwrap().filter = index;
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
                            error!("[ModManager] could not create group '{name}': {e}");
                        } else {
                            info!("[ModManager] created group '{name}'");
                        }
                    }
                    None => error!("[ModManager] could not create group: no mods path"),
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
                warn!("[ModManager] invalid group name '{name}', ignoring");
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
                    warn!("[ModManager] group '{name}' already exists, ignoring rename");
                } else if let Err(e) = std::fs::rename(&old_path, &new_path) {
                    error!(
                        "[ModManager] could not rename group '{}': {e}",
                        old_path.display()
                    );
                } else {
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
                // Move the mods out to the root mods folder before deleting
                let Some(mods_path) = get_mods_path() else {
                    return;
                };
                if let Ok(entries) = group_path.read_dir() {
                    for entry in entries.flatten() {
                        let target = mods_path.join(entry.file_name());
                        if target.exists() {
                            warn!(
                                "[ModManager] not moving '{}' out of group: target exists",
                                entry.path().display()
                            );
                            continue;
                        }
                        if let Err(e) = std::fs::rename(entry.path(), &target) {
                            error!(
                                "[ModManager] could not move '{}' out of group: {e}",
                                entry.path().display()
                            );
                        }
                    }
                }

                // Only removes the folder if everything was moved out
                if let Err(e) = std::fs::remove_dir(&group_path) {
                    error!(
                        "[ModManager] could not delete group '{}': {e}",
                        group_path.display()
                    );
                } else {
                    info!("[ModManager] deleted group '{}'", group_path.display());
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
                        if let Err(e) = ModManager::toggle_mod(m) {
                            error!("[ModManager] could not toggle '{}': {e}", m.folder_name);
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
        w.on_mod_drag_moved(move |id, content_y| {
            let Some(win) = ww.upgrade() else { return };

            let target = Self::zone_at(content_y).map_or_else(String::new, |zone| {
                let source = Self::mod_by_id(&id)
                    .map(|m| Self::current_zone(&m))
                    .unwrap_or_default();
                if zone == source {
                    String::new()
                } else {
                    zone
                }
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
                error!("[ModManager] could not open support link '{link}': {e}");
            }
        });

        let ww = window.clone();
        w.on_mods_refresh(move || {
            Self::reload(&ww);
        });

        w.on_open_mods_folder(move || {
            let Some(folder) = get_mods_path() else {
                return;
            };
            let _ = std::fs::create_dir_all(&folder);
            if let Err(e) = open::that(&folder) {
                error!(
                    "[ModManager] could not open mods folder '{}': {e}",
                    folder.display()
                );
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

        info!("[ModManager] bind() complete");
    }

    fn update_row(w: &MainWindow, id: &str, change: impl Fn(&mut ModItem)) {
        let model = w.get_mods();
        for i in 0..model.row_count() {
            if let Some(mut row) = model.row_data(i) {
                if row.id == id {
                    change(&mut row);
                    model.set_row_data(i, row);
                    break;
                }
            }
        }

        let sections = w.get_mods_grid();
        for s in 0..sections.row_count() {
            let Some(section) = sections.row_data(s) else {
                continue;
            };
            for i in 0..section.row_count() {
                if let Some(mut row) = section.row_data(i) {
                    if row.id == id {
                        change(&mut row);
                        section.set_row_data(i, row);
                        return;
                    }
                }
            }
        }
    }
}
