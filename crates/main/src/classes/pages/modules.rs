use crate::classes::pages::modmanager::{config_map, config_map_set};
use crate::{MainWindow, ModItem};

use anyhow::{anyhow, Result};
use log::*;
use once_cell::sync::Lazy;
use serde_json::Value;
use shared::config::{self, key};
use slint::{Model, VecModel};

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

const NOTES_KEY: &str = "module_notes";
const DISPLAY_NAMES_KEY: &str = "module_display_names";
const DISABLED_SUFFIX: &str = ".disabled";
const MODULE_EXTENSIONS: [&str; 2] = ["asi", "dll"];

#[derive(Debug, Clone)]
struct Module {
    id: String,
    path: PathBuf,
    enabled: bool,
}

#[derive(Default)]
struct State {
    scanned: Vec<Module>,
    displayed: Vec<String>,
    selected: HashSet<String>,
    search: String,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

pub struct ModulesHandler;

impl ModulesHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("[Modules] setup() called");

        let w = window.unwrap();
        w.set_modules_feature_enabled(
            config::get(key::CUSTOM_ADDONS_TOGGLED)
                .as_bool()
                .unwrap_or(false),
        );

        Self::bind(window);
        Self::reload(window);
        info!("[Modules] setup() complete");
    }

    fn modules_path() -> PathBuf {
        config::get_userdata_path()
            .join("ThirdParty")
            .join("Modules")
    }

    fn default_name(id: &str) -> String {
        Path::new(id)
            .file_stem()
            .map_or_else(|| id.to_string(), |s| s.to_string_lossy().into_owned())
    }

    fn module_by_id(id: &str) -> Option<Module> {
        STATE
            .lock()
            .unwrap()
            .scanned
            .iter()
            .find(|m| m.id == id)
            .cloned()
    }

    fn scan() -> Vec<Module> {
        let dir = Self::modules_path();
        let mut modules = Vec::new();

        let Ok(entries) = std::fs::read_dir(&dir) else {
            return modules;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };

            let (id, enabled) = name.strip_suffix(DISABLED_SUFFIX).map_or_else(
                || (name.to_string(), true),
                |base| (base.to_string(), false),
            );

            let ext = Path::new(&id)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !MODULE_EXTENSIONS.contains(&ext.as_str()) {
                warn!("[Modules] scan: skipping unknown file '{name}'");
                continue;
            }

            modules.push(Module { id, path, enabled });
        }

        modules.sort_by_key(|m| m.id.to_lowercase());
        modules
    }

    fn sync_custom_addons(modules: &[Module]) {
        let paths: Vec<Value> = modules
            .iter()
            .filter(|m| m.enabled)
            .map(|m| Value::from(m.path.to_string_lossy().into_owned()))
            .collect();
        config::set(key::CUSTOM_ADDONS, Value::Array(paths));
    }

    fn reload(window: &slint::Weak<MainWindow>) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let modules = Self::scan();
            Self::sync_custom_addons(&modules);

            let _ = slint::invoke_from_event_loop(move || {
                let Some(w) = ww.upgrade() else {
                    error!("[Modules] could not load: window handle is dead");
                    return;
                };

                STATE.lock().unwrap().scanned = modules;
                Self::rebuild(&w);
            });
        });
    }

    fn rebuild(w: &MainWindow) {
        let display_names = config_map(DISPLAY_NAMES_KEY);
        let notes = config_map(NOTES_KEY);

        let shown_name = |m: &Module| -> String {
            display_names
                .get(&m.id)
                .and_then(|v| v.as_str())
                .map_or_else(|| Self::default_name(&m.id), ToString::to_string)
        };

        let (items, selected_count, all_selected) = {
            let mut state = STATE.lock().unwrap();
            let visible: Vec<Module> = state
                .scanned
                .iter()
                .filter(|m| {
                    state.search.is_empty()
                        || shown_name(m).to_lowercase().contains(&state.search)
                        || m.id.to_lowercase().contains(&state.search)
                })
                .cloned()
                .collect();

            state.displayed = visible.iter().map(|m| m.id.clone()).collect();
            let existing: HashSet<String> = state.scanned.iter().map(|m| m.id.clone()).collect();
            state.selected.retain(|id| existing.contains(id));

            let items: Vec<ModItem> = visible
                .iter()
                .map(|m| ModItem {
                    id: m.id.clone().into(),
                    name: shown_name(m).into(),
                    author: "".into(),
                    version: "".into(),
                    icon: slint::Image::default(),
                    notes: notes
                        .get(&m.id)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .into(),
                    enabled: m.enabled,
                    selected: state.selected.contains(&m.id),
                    has_json: false,
                    is_editing: false,
                    group_id: "".into(),
                    support_link: "".into(),
                    is_group_header: false,
                    collapsed: false,
                })
                .collect();

            let count = state.selected.len();
            let all = !state.displayed.is_empty()
                && state.displayed.iter().all(|id| state.selected.contains(id));
            drop(state);

            (items, count, all)
        };

        w.set_modules_list(Rc::new(VecModel::from(items)).into());
        w.set_modules_selected_count(i32::try_from(selected_count).unwrap_or(0));
        w.set_modules_all_selected(all_selected);
    }

    fn update_selection_props(w: &MainWindow) {
        let state = STATE.lock().unwrap();
        let count = state.selected.len();
        let all = !state.displayed.is_empty()
            && state.displayed.iter().all(|id| state.selected.contains(id));
        drop(state);
        w.set_modules_selected_count(i32::try_from(count).unwrap_or(0));
        w.set_modules_all_selected(all);
    }

    fn show_toast(w: &MainWindow, kind: &str, text: String) {
        w.set_toast_text(text.into());
        w.set_toast_kind(kind.into());
        w.set_toast_active(true);
    }

    fn install_path(path: &Path) -> Result<String> {
        let dir = Self::modules_path();
        std::fs::create_dir_all(&dir)?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| anyhow!("invalid file name"))?;

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !MODULE_EXTENSIONS.contains(&ext.as_str()) {
            return Err(anyhow!("unsupported file type '.{ext}'"));
        }

        let target = dir.join(file_name);
        if target.exists() || dir.join(format!("{file_name}{DISABLED_SUFFIX}")).exists() {
            return Err(anyhow!("a module with this name already exists"));
        }

        std::fs::copy(path, &target)?;
        Ok(file_name.to_string())
    }

    fn install_paths(window: &slint::Weak<MainWindow>, paths: Vec<PathBuf>) {
        let ww = window.clone();
        std::thread::spawn(move || {
            let mut installed: Vec<String> = Vec::new();
            let mut failed: Vec<String> = Vec::new();

            for path in &paths {
                match Self::install_path(path) {
                    Ok(name) => {
                        info!("[Modules] installed '{name}' from '{}'", path.display());
                        installed.push(name);
                    }
                    Err(e) => {
                        error!("[Modules] could not install '{}': {e}", path.display());
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
                        format!("Installed {} modules", installed.len()),
                    );
                }
            });

            Self::reload(&ww);
        });
    }

    fn toggle_module(module: &Module) -> Result<()> {
        let new = if module.enabled {
            module
                .path
                .with_file_name(format!("{}{DISABLED_SUFFIX}", module.id))
        } else {
            module.path.with_file_name(module.id.clone())
        };
        std::fs::rename(&module.path, &new)?;
        trace!(
            "[Modules] renamed '{}' → '{}'",
            module.path.display(),
            new.display()
        );
        Ok(())
    }

    // [CALLBACKS]

    fn bind(window: &slint::Weak<MainWindow>) {
        let w = window.unwrap();

        let ww = window.clone();
        w.on_module_choose(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let picked = rfd::FileDialog::new()
                    .set_title("Select Modules")
                    .add_filter("Modules", &MODULE_EXTENSIONS)
                    .pick_files();

                if let Some(files) = picked {
                    Self::install_paths(&ww, files);
                }
            });
        });

        let ww = window.clone();
        w.on_module_drop_files(move |path| {
            Self::install_paths(&ww, vec![PathBuf::from(path.to_string())]);
        });

        let ww = window.clone();
        w.on_module_rename(move |id, new_name| {
            let Some(win) = ww.upgrade() else { return };
            let Some(m) = Self::module_by_id(&id) else {
                return;
            };
            let name = new_name.trim();

            config_map_set(DISPLAY_NAMES_KEY, &m.id, (!name.is_empty()).then_some(name));

            let shown = if name.is_empty() {
                Self::default_name(&m.id)
            } else {
                name.to_string()
            };
            Self::update_row(&win, &id, |row| row.name = shown.as_str().into());
        });

        let ww = window.clone();
        w.on_module_set_notes(move |id, notes| {
            let Some(win) = ww.upgrade() else { return };
            let Some(m) = Self::module_by_id(&id) else {
                return;
            };
            let notes = notes.trim().to_string();

            config_map_set(NOTES_KEY, &m.id, (!notes.is_empty()).then_some(&notes));
            Self::update_row(&win, &id, |row| row.notes = notes.as_str().into());
        });

        let ww = window.clone();
        w.on_module_delete(move |id, name| {
            let ww = ww.clone();
            let name = name.to_string();
            let id = id.to_string();
            std::thread::spawn(move || {
                if let Some(m) = Self::module_by_id(&id) {
                    match std::fs::remove_file(&m.path) {
                        Ok(()) => {
                            info!("[Modules] deleted '{}'", m.path.display());
                            // Drop leftover per-module config entries
                            config_map_set(NOTES_KEY, &m.id, None);
                            config_map_set(DISPLAY_NAMES_KEY, &m.id, None);

                            let ww2 = ww.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(win) = ww2.upgrade() {
                                    Self::show_toast(&win, "success", format!("Deleted {name}"));
                                }
                            });
                        }
                        Err(e) => {
                            error!("[Modules] could not delete '{}': {e}", m.path.display());
                        }
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_module_toggle(move |id| {
            let id = id.to_string();
            let ww = ww.clone();
            std::thread::spawn(move || {
                if let Some(m) = Self::module_by_id(&id) {
                    if let Err(e) = Self::toggle_module(&m) {
                        error!("[Modules] could not toggle '{}': {e}", m.id);
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_module_select(move |id| {
            let Some(win) = ww.upgrade() else { return };
            let id = id.to_string();

            let selected = {
                let mut state = STATE.lock().unwrap();
                if state.selected.remove(&id) {
                    false
                } else {
                    state.selected.insert(id.clone());
                    true
                }
            };

            Self::update_row(&win, &id, |row| row.selected = selected);
            Self::update_selection_props(&win);
        });

        let ww = window.clone();
        w.on_modules_select_all(move || {
            let Some(win) = ww.upgrade() else { return };

            {
                let mut state = STATE.lock().unwrap();
                let all = !state.displayed.is_empty()
                    && state.displayed.iter().all(|id| state.selected.contains(id));
                if all {
                    state.selected.clear();
                } else {
                    state.selected = state.displayed.iter().cloned().collect();
                }
            }

            Self::rebuild(&win);
        });

        let ww = window.clone();
        w.on_modules_toggle_selected(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let modules: Vec<Module> = {
                    let state = STATE.lock().unwrap();
                    state
                        .scanned
                        .iter()
                        .filter(|m| state.selected.contains(&m.id))
                        .cloned()
                        .collect()
                };
                for m in &modules {
                    if let Err(e) = Self::toggle_module(m) {
                        error!("[Modules] could not toggle '{}': {e}", m.id);
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_modules_delete_selected(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let modules: Vec<Module> = {
                    let state = STATE.lock().unwrap();
                    state
                        .scanned
                        .iter()
                        .filter(|m| state.selected.contains(&m.id))
                        .cloned()
                        .collect()
                };
                for m in &modules {
                    match std::fs::remove_file(&m.path) {
                        Ok(()) => {
                            info!("[Modules] deleted '{}'", m.path.display());
                            config_map_set(NOTES_KEY, &m.id, None);
                            config_map_set(DISPLAY_NAMES_KEY, &m.id, None);
                        }
                        Err(e) => {
                            error!("[Modules] could not delete '{}': {e}", m.path.display());
                        }
                    }
                }
                STATE.lock().unwrap().selected.clear();
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_module_search_changed(move |text| {
            let Some(win) = ww.upgrade() else { return };
            STATE.lock().unwrap().search = text.trim().to_lowercase();
            Self::rebuild(&win);
        });

        w.on_modules_feature_toggled(move |enabled| {
            info!("[Modules] feature toggled -> {enabled}");
            config::set(key::CUSTOM_ADDONS_TOGGLED, enabled);
        });

        info!("[Modules] bind() complete");
    }

    fn update_row(w: &MainWindow, id: &str, change: impl Fn(&mut ModItem)) {
        let model = w.get_modules_list();
        for i in 0..model.row_count() {
            if let Some(mut row) = model.row_data(i) {
                if row.id == id {
                    change(&mut row);
                    model.set_row_data(i, row);
                    break;
                }
            }
        }
    }
}
