use crate::classes::pages::modmanager::{profile_map, profile_map_set};
use crate::runtime::{self, GameContext, MutationGuard};
use crate::{MainWindow, ModItem, ModTag};

use anyhow::{Result, anyhow};
use log::*;
use once_cell::sync::Lazy;
use serde_json::Value;
use shared::classes::games::{
    Game,
    capabilities::{FileState, ModNaming},
};
use shared::config::{self, ProfileKey, ProfilePatch};
use slint::{Model, VecModel};

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
struct ModuleScope {
    game: &'static dyn Game,
    generation: u64,
    storage: PathBuf,
    naming: ModNaming,
}

impl ModuleScope {
    fn of(context: &GameContext) -> Result<Self> {
        let game = context.game();
        let modules = game.modules().ok_or_else(|| {
            anyhow!("{} does not support modules", game.descriptor().display_name)
        })?;
        let layout = modules.layout(&config::game_modules_path(context.id()));
        Ok(Self {
            game,
            generation: context.generation(),
            storage: layout.storage,
            naming: layout.naming,
        })
    }

    fn current() -> Result<Self> {
        let context = runtime::get()?
            .snapshot()
            .context
            .ok_or_else(|| anyhow!("no game is loaded yet"))?;
        Self::of(&context)
    }

    fn guarded() -> Result<(MutationGuard, Self)> {
        let guard = runtime::get()?.try_begin_mutation()?;
        let scope = Self::of(guard.context())?;
        let shown = STATE.lock().unwrap().scope.as_ref().map(|s| s.generation);
        if shown != Some(scope.generation) {
            return Err(anyhow!("the selected game changed, try again"));
        }
        Ok((guard, scope))
    }
}

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
    scope: Option<ModuleScope>,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

static GENERATION: AtomicU64 = AtomicU64::new(0);

static ADDONS_SYNC: Mutex<u64> = Mutex::new(0);

pub struct ModulesHandler;

impl ModulesHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("[Modules] setup() called");

        Self::bind(window);
        Self::reload(window);
        info!("[Modules] setup() complete");
    }

    fn feature_enabled(game: &dyn Game) -> bool {
        config::read_profile(&game.descriptor().id)
            .map(|profile| profile.value(ProfileKey::CustomAddonsToggled, game))
            .ok()
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
    }

    fn refuse(window: &slint::Weak<MainWindow>, action: &str, error: &anyhow::Error) {
        warn!("[Modules] refused to {action}: {error:#}");
        let text = format!("Could not {action} - {error:#}");
        let ww = window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(win) = ww.upgrade() {
                Self::show_toast(&win, "error", text);
            }
        });
    }

    fn begin(window: &slint::Weak<MainWindow>, action: &str) -> Option<(MutationGuard, ModuleScope)> {
        ModuleScope::guarded()
            .map_err(|e| Self::refuse(window, action, &e))
            .ok()
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

    fn scan(scope: &ModuleScope) -> Vec<Module> {
        let dir = &scope.storage;
        let mut modules = Vec::new();

        let Ok(entries) = std::fs::read_dir(dir) else {
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

            let (id, enabled) = match scope.naming.classify(name) {
                FileState::Enabled => (name.to_string(), true),
                FileState::Disabled => match scope.naming.toggled(name, true) {
                    Some(base) => (base, false),
                    None => continue,
                },
                FileState::Other => {
                    warn!("[Modules] scan: skipping unknown file '{name}'");
                    continue;
                }
            };

            modules.push(Module { id, path, enabled });
        }

        modules.sort_by_key(|m| m.id.to_lowercase());
        modules
    }

    fn sync_custom_addons(generation: u64, scope: &ModuleScope, modules: &[Module]) {
        let mut synced = ADDONS_SYNC.lock().unwrap();
        if generation < *synced {
            return;
        }
        *synced = generation;
        drop(synced);

        let guard = match runtime::get().and_then(|r| r.try_begin_mutation()) {
            Ok(guard) if guard.context().generation() == scope.generation => guard,
            Ok(_) => return,
            Err(e) => {
                debug!("[Modules] not saving the module list now: {e}");
                return;
            }
        };

        let paths: Vec<Value> = modules
            .iter()
            .filter(|m| m.enabled)
            .map(|m| Value::from(m.path.to_string_lossy().into_owned()))
            .collect();

        let current = config::read_profile(&scope.game.descriptor().id)
            .ok()
            .and_then(|profile| profile.get(ProfileKey::CustomAddons).cloned());
        if current.as_ref() != Some(&Value::Array(paths.clone()))
            && let Err(e) = config::update_profile(
                &scope.game.descriptor().id,
                &ProfilePatch::new().set(ProfileKey::CustomAddons, Value::Array(paths)),
            )
        {
            error!("[Modules] could not save the custom addon list: {e:#}");
        }
        drop(guard);
    }

    pub(crate) fn reload(window: &slint::Weak<MainWindow>) {
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let ww = window.clone();
        std::thread::spawn(move || {
            let scope = ModuleScope::current()
                .inspect_err(|e| debug!("[Modules] nothing to scan: {e}"))
                .ok();
            let modules = scope.as_ref().map_or_default(Self::scan);
            if let Some(scope) = &scope {
                Self::sync_custom_addons(generation, scope, &modules);
            }
            let enabled = scope.as_ref().is_some_and(|s| Self::feature_enabled(s.game));

            let _ = slint::invoke_from_event_loop(move || {
                if GENERATION.load(Ordering::SeqCst) != generation
                    || scope
                        .as_ref()
                        .is_some_and(|s| !runtime::is_current_generation(s.generation))
                {
                    return;
                }
                let Some(w) = ww.upgrade() else {
                    error!("[Modules] could not load: window handle is dead");
                    return;
                };

                {
                    let mut state = STATE.lock().unwrap();
                    let previous = state.scope.as_ref().map(|s| s.game.descriptor().id.clone());
                    if previous != scope.as_ref().map(|s| s.game.descriptor().id.clone()) {
                        state.selected.clear();
                        state.search.clear();
                    }
                    state.scope = scope;
                    state.scanned = modules;
                }
                w.set_modules_feature_enabled(enabled);
                Self::rebuild(&w);
            });
        });
    }

    fn rebuild(w: &MainWindow) {
        let game = STATE.lock().unwrap().scope.as_ref().map(|s| s.game);
        let display_names = game.map_or_default(|g| profile_map(g, ProfileKey::ModDisplayNames));
        let notes = game.map_or_default(|g| profile_map(g, ProfileKey::ModNotes));

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
                    restart_required: false,
                    tag: ModTag::None,
                    has_icon_png: false,
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

    fn install_path(path: &Path, scope: &ModuleScope) -> Result<String> {
        let dir = &scope.storage;
        std::fs::create_dir_all(dir)?;

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .ok_or_else(|| anyhow!("invalid file name"))?;

        if scope.naming.classify(file_name) != FileState::Enabled {
            return Err(anyhow!(
                "unsupported file type, expected .{}",
                scope.naming.extensions.join(", .")
            ));
        }

        let target = dir.join(file_name);
        let disabled = scope
            .naming
            .toggled(file_name, false)
            .map(|name| dir.join(name));
        if target.exists() || disabled.is_some_and(|d| d.exists()) {
            return Err(anyhow!("a module with this name already exists"));
        }

        std::fs::copy(path, &target)?;
        Ok(file_name.to_string())
    }

    pub(crate) fn install_paths(window: &slint::Weak<MainWindow>, paths: Vec<PathBuf>) {
        let Some((guard, scope)) = Self::begin(window, "install the modules") else {
            return;
        };
        let ww = window.clone();
        std::thread::spawn(move || {
            let mut installed: Vec<String> = Vec::new();
            let mut failed: Vec<String> = Vec::new();

            for path in &paths {
                match Self::install_path(path, &scope) {
                    Ok(name) => {
                        info!("[Modules] installed '{name}' from '{}'", path.display());
                        installed.push(name);
                    }
                    Err(e) => {
                        error!("[Modules] could not install '{}': {e}", path.display());
                        failed.push(format!(
                            "{}: {e}",
                            path.file_name()
                                .map_or_default(|n| n.to_string_lossy().into_owned())
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

            drop(guard);
            Self::reload(&ww);
        });
    }

    fn toggle_module(module: &Module, naming: &ModNaming) -> Result<()> {
        let name = module
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let new = naming
            .toggled(&name, !module.enabled)
            .map(|name| module.path.with_file_name(name))
            .ok_or_else(|| anyhow!("'{name}' cannot be toggled"))?;
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
            let Some(extensions) = STATE
                .lock()
                .unwrap()
                .scope
                .as_ref()
                .map(|s| s.naming.extensions)
            else {
                return;
            };
            let ww = ww.clone();
            std::thread::spawn(move || {
                let picked = rfd::FileDialog::new()
                    .set_title("Select Modules")
                    .add_filter("Modules", extensions)
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
            let Some((_guard, scope)) = Self::begin(&ww, "rename the module") else {
                return;
            };
            let name = new_name.trim();

            profile_map_set(
                scope.game,
                ProfileKey::ModDisplayNames,
                &m.id,
                (!name.is_empty()).then_some(name),
            );

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
            let Some((_guard, scope)) = Self::begin(&ww, "save the notes") else {
                return;
            };
            let notes = notes.trim().to_string();

            profile_map_set(
                scope.game,
                ProfileKey::ModNotes,
                &m.id,
                (!notes.is_empty()).then_some(&notes),
            );
            Self::update_row(&win, &id, |row| row.notes = notes.as_str().into());
        });

        let ww = window.clone();
        w.on_module_delete(move |id, name| {
            let Some((guard, scope)) = Self::begin(&ww, "delete the module") else {
                return;
            };
            let ww = ww.clone();
            let name = name.to_string();
            let id = id.to_string();
            std::thread::spawn(move || {
                let _guard = guard;
                if let Some(m) = Self::module_by_id(&id) {
                    match std::fs::remove_file(&m.path) {
                        Ok(()) => {
                            info!("[Modules] deleted '{}'", m.path.display());
                            // Drop leftover per-module config entries
                            profile_map_set(scope.game, ProfileKey::ModNotes, &m.id, None);
                            profile_map_set(scope.game, ProfileKey::ModDisplayNames, &m.id, None);

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
            let Some((guard, scope)) = Self::begin(&ww, "toggle the module") else {
                return;
            };
            let ww = ww.clone();
            std::thread::spawn(move || {
                let _guard = guard;
                if let Some(m) = Self::module_by_id(&id)
                    && let Err(e) = Self::toggle_module(&m, &scope.naming)
                {
                    error!("[Modules] could not toggle '{}': {e}", m.id);
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
            let Some((guard, scope)) = Self::begin(&ww, "toggle the modules") else {
                return;
            };
            let ww = ww.clone();
            std::thread::spawn(move || {
                let _guard = guard;
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
                    if let Err(e) = Self::toggle_module(m, &scope.naming) {
                        error!("[Modules] could not toggle '{}': {e}", m.id);
                    }
                }
                Self::reload(&ww);
            });
        });

        let ww = window.clone();
        w.on_modules_delete_selected(move || {
            let Some((guard, scope)) = Self::begin(&ww, "delete the modules") else {
                return;
            };
            let ww = ww.clone();
            std::thread::spawn(move || {
                let _guard = guard;
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
                            profile_map_set(scope.game, ProfileKey::ModNotes, &m.id, None);
                            profile_map_set(scope.game, ProfileKey::ModDisplayNames, &m.id, None);
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

        let ww = window.clone();
        w.on_modules_feature_toggled(move |enabled| {
            info!("[Modules] feature toggled -> {enabled}");
            let saved = Self::begin(&ww, "change the modules setting").is_some_and(|(_guard, scope)| {
                config::update_profile(
                    &scope.game.descriptor().id,
                    &ProfilePatch::new().set(ProfileKey::CustomAddonsToggled, enabled),
                )
                .inspect_err(|e| error!("[Modules] could not save the modules setting: {e:#}"))
                .is_ok()
            });
            if !saved && let Some(w) = ww.upgrade() {
                w.set_modules_feature_enabled(!enabled);
            }
        });

        info!("[Modules] bind() complete");
    }

    fn update_row(w: &MainWindow, id: &str, change: impl Fn(&mut ModItem)) {
        let model = w.get_modules_list();
        for i in 0..model.row_count() {
            if let Some(mut row) = model.row_data(i)
                && row.id == id
            {
                change(&mut row);
                model.set_row_data(i, row);
                break;
            }
        }
    }
}
