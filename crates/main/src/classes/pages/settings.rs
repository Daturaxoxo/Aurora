use crate::classes::logwindow;
use crate::classes::pages::modmanager::ModManagerHandler;
use crate::classes::toast::ToastHandler;
use crate::{MainWindow, Tr, TrKey};
use backend::classes::rpc::RPC;
use backend::handler::{get_tx, EngineCommand, GAME_RUNNING};
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use shared::config::{self, key};
use shared::pathfind::resolve_selected_game_root;
use slint::{ComponentHandle as _, Model as _};
use std::sync::atomic::Ordering;

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct LangEntry {
    name: String,
    code: String,
}

static LANGUAGES: Lazy<Vec<LangEntry>> = Lazy::new(|| {
    serde_json::from_str(include_str!(
        "../../../../../production/Langs/lang-codes.json"
    ))
    .expect("lang-codes.json is missing or malformed!")
});

pub const IGNORE_CHECKSUM_POPUP_ID: &str = "ignore-checksum";

pub struct SettingsHandler;

impl SettingsHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("[Settings] setup() called");
        Self::load(window);
        Self::bind(window);
        info!("[Settings] setup() complete");
    }

    fn load(window: &slint::Weak<MainWindow>) {
        info!("[Settings] load() started - reading config values");

        let Some(w) = window.upgrade() else {
            error!("[Settings] load() failed - window handle is dead, cannot apply config to UI");
            return;
        };

        // General
        let raw_lang = config::get(key::LANGUAGE);
        let lang_code = raw_lang.as_str().unwrap_or("en").to_string();
        let lang_index = Self::code_to_index(&lang_code).unwrap_or(0);
        debug!("[Settings] language: raw={raw_lang:?} → code={lang_code:?} → index={lang_index}");
        w.set_language_index(lang_index);

        let raw_minimization = config::get(key::UI_MINIMIZATION);
        let minimization = raw_minimization.as_bool().unwrap_or(true);
        debug!("[Settings] interface_minimization: raw={raw_minimization:?} → {minimization}");
        w.set_interface_minimization(minimization);

        let raw_rpc = config::get(key::DISCORD_RPC);
        let discord_rpc = raw_rpc.as_bool().unwrap_or(true);
        debug!("[Settings] discord_rpc: raw={raw_rpc:?} → {discord_rpc}");
        w.set_discord_rpc(discord_rpc);
        if discord_rpc {
            if let Err(e) = RPC.set_idle() {
                error!("[Settings] could not set Discord RPC to idle: {e}");
            }
        }

        // Launcher
        let raw_path = config::get(key::GAME_PATH);
        let game_path = raw_path.as_str().unwrap_or("").to_string();
        debug!("[Settings] game_path: raw={raw_path:?} → {game_path:?}");
        if game_path.is_empty() {
            warn!("[Settings] game_path is empty - user has not set a game directory yet");
        }
        w.set_game_directory(game_path.into());

        let raw_engine = config::get(key::ENGINE_METHOD);
        let engine_method = raw_engine.as_i64().unwrap_or(0).try_into().unwrap_or(0);
        debug!("[Settings] engine_method: raw={raw_engine:?} → {engine_method}");
        w.set_engine_method_index(engine_method);

        let raw_ignore_checksum = config::get(key::IGNORE_CHECKSUM);
        let ignore_checksum = raw_ignore_checksum.as_bool().unwrap_or(false);
        debug!("[Settings] ignore_checksum: raw={raw_ignore_checksum:?} → {ignore_checksum}");
        w.set_ignore_checksum(ignore_checksum);

        // Linux only
        w.set_is_linux(cfg!(target_os = "linux"));
        if cfg!(target_os = "linux") {
            let raw_proton = config::get(key::PROTON_ARGS);
            let proton_args = raw_proton.as_str().unwrap_or("").to_string();
            debug!("[Settings] proton_args: raw={raw_proton:?} → {proton_args:?}");
            w.set_proton_launch_args(proton_args.into());

            Self::load_proton_versions(&w);

            let raw_entry = config::get(key::DESKTOP_ENTRY);
            let desktop_entry = raw_entry.as_bool().unwrap_or(false);
            debug!("[Settings] desktop_entry: raw={raw_entry:?} → {desktop_entry}");
            w.set_desktop_entry(desktop_entry);
        }

        // Developer
        let raw_dev = config::get(key::DEV_MODE);
        let dev_mode = raw_dev.as_bool().unwrap_or(false);
        debug!("[Settings] developer_mode: raw={raw_dev:?} → {dev_mode}");
        w.set_developer_mode(dev_mode);
        if dev_mode {
            debug!("[Settings] developer_mode was left on, reopening the log window");
            logwindow::set_visible(true, window);
        }

        info!("[Settings] load() complete shortcut all config values applied to UI");
    }

    #[cfg(not(target_os = "linux"))]
    const fn load_proton_versions(w: &MainWindow) {
        let _ = w;
    }

    #[cfg(target_os = "linux")]
    fn load_proton_versions(w: &MainWindow) {
        let builds = backend::classes::linux::installed_dwproton_builds();
        debug!("[Settings] installed DW-Proton builds: {builds:?}");

        let raw_version = config::get(key::PROTON_VERSION);
        let saved = raw_version.as_str().unwrap_or("").trim().to_string();

        // Entry 0 is "Automatic", so an installed build sits one slot later.
        let index = if saved.is_empty() {
            0
        } else {
            builds
                .iter()
                .position(|build| *build == saved)
                .and_then(|i| i32::try_from(i).ok())
                .map_or_else(
                    || {
                        warn!(
                            "[Settings] saved proton_version {saved:?} is not installed any \
                                 more, showing Automatic instead"
                        );
                        0
                    },
                    |i| i + 1,
                )
        };
        debug!("[Settings] proton_version: raw={raw_version:?} → index={index}");

        let mut options = Vec::with_capacity(builds.len() + 1);
        options.push(slint::SharedString::from(crate::translations::tr(
            "settings.proton-version.automatic",
        )));
        options.extend(builds.iter().map(slint::SharedString::from));

        w.set_proton_versions(slint::ModelRc::new(slint::VecModel::from(options)));
        w.set_proton_version_index(index);
    }

    fn bind(window: &slint::Weak<MainWindow>) {
        info!("[Settings] bind() started shortcut registering UI callbacks");
        let w = window.unwrap();

        // [GENERAL]

        let ww = window.clone();
        w.on_language_index_changed(move |index| {
            let code = Self::index_to_code(index);
            info!("[Settings] language changed → index={index}, code={code:?}");
            config::set(key::LANGUAGE, code);
            debug!("[Settings] language saved to config");

            if let Some(w) = ww.upgrade() {
                crate::translations::apply_language(&w, code);
                Self::load_proton_versions(&w);
            } else {
                error!("[Settings] window handle dead when applying language change");
            }
            crate::classes::logwindow::apply_language(code);
        });

        let ww = window.clone();
        w.on_interface_minimization_changed(move |enabled| {
            info!("[Settings] interface_minimization changed → {enabled}");
            config::set(key::UI_MINIMIZATION, enabled);
            debug!("[Settings] interface_minimization saved to config");
            if enabled {
                if GAME_RUNNING.load(Ordering::Relaxed) {
                    crate::classes::tray::activate(&ww, false);
                }
            } else {
                crate::classes::tray::deactivate(&ww);
            }
        });

        w.on_discord_rpc_changed(move |enabled| {
            info!("[Settings] discord_rpc changed → {enabled}");
            config::set(key::DISCORD_RPC, enabled);
            let res = if enabled { RPC.set_idle() } else { RPC.stop() };
            if let Err(e) = res {
                error!("[Settings] could not update Discord RPC state: {e}");
            }
            debug!("[Settings] discord_rpc saved to config");
        });

        // [LAUNCHER]

        let ww = window.clone();
        w.on_browse_game_directory(move || {
            info!("[Settings] browse_game_directory triggered shortcut opening folder picker");
            let ww = ww.clone();
            std::thread::spawn(move || {
                debug!("[Settings] file dialog thread spawned");
                let picked = rfd::FileDialog::new()
                    .set_title("Select Game Installation")
                    .pick_folder();

                match picked {
                    Some(path) => {
                        info!("[Settings] game directory selected -> {:?}", path.display());

                        let path = if let Some(root) = resolve_selected_game_root(&path) {
                            if root != path {
                                info!(
                                    "[Settings] resolved selection to install root -> {:?}",
                                    root.display()
                                );
                            }
                            root
                        } else {
                            warn!(
                                "[Settings] {:?} does not look like a game install (no launcher \
                                 or game files found); saving it anyway",
                                path.display()
                            );
                            path
                        };

                        let path_str: String = path.to_string_lossy().into_owned();
                        config::set(key::GAME_PATH, path_str.clone());
                        debug!("[Settings] game_path saved to config");

                        match get_tx() {
                            Ok(tx) => {
                                if let Err(e) = tx.send(EngineCommand::Update) {
                                    error!("[Settings] failed to notify engine of game_path change: {e}");
                                }
                            }
                            Err(e) => warn!("[Settings] engine not started yet, skipping live update: {e}"),
                        }

                        ModManagerHandler::reload(&ww);

                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = ww.upgrade() {
                                w.set_game_directory(path_str.into());
                                debug!("[Settings] game_directory UI property updated");
                            } else {
                                error!("[Settings] window handle dead when trying to update game_directory UI");
                            }
                        });
                    }
                    None => {
                        info!("[Settings] browse_game_directory cancelled shortcut no folder selected");
                    }
                }
            });
        });

        let ww = window.clone();
        w.on_ignore_checksum_changed(move |enabled| {
            info!("[Settings] ignore_checksum toggled → {enabled}");

            if !enabled {
                config::set(key::IGNORE_CHECKSUM, false);
                debug!("[Settings] ignore_checksum saved to config");
                return;
            }

            // Turning it on is only committed once the warning popup is confirmed.
            let Some(w) = ww.upgrade() else {
                error!("[Settings] window handle dead when opening the ignore_checksum warning");
                return;
            };

            let keys = w.global::<TrKey>();
            let title = Self::translation(&w, keys.get_popup_ignore_checksum_title());
            let message = Self::translation(&w, keys.get_popup_ignore_checksum_message());

            w.set_popup_id(IGNORE_CHECKSUM_POPUP_ID.into());
            w.set_popup_title(title);
            w.set_popup_message(message);
            w.set_popup_confirm_delay(0);
            w.set_popup_required_count(0);
            w.set_popup_checkboxes(slint::ModelRc::default());
            w.set_popup_active(true);
        });

        w.on_engine_method_index_changed(move |index| {
            info!("[Settings] engine_method changed -> {index}");
            config::set(key::ENGINE_METHOD, index);
            debug!("[Settings] engine_method saved to config");

            match get_tx() {
                Ok(tx) => {
                    if let Err(e) = tx.send(EngineCommand::Update) {
                        error!("[Settings] failed to notify engine of engine_method change: {e}");
                    }
                }
                Err(e) => warn!("[Settings] engine not started yet, skipping live update: {e}"),
            }
        });

        // [LINUX]

        w.on_proton_launch_args_changed(move |args| {
            info!("[Settings] proton_launch_args changed → {args:?}");
            config::set(key::PROTON_ARGS, args.as_str());
            debug!("[Settings] proton_args saved to config");
        });

        let ww = window.clone();
        w.on_proton_version_index_changed(move |index| {
            let name = if index <= 0 {
                String::new()
            } else {
                ww.upgrade()
                    .zip(usize::try_from(index).ok())
                    .and_then(|(w, index)| {
                        use slint::Model;
                        w.get_proton_versions().row_data(index)
                    })
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            };

            info!("[Settings] proton_version changed → index={index}, build={name:?}");
            config::set(key::PROTON_VERSION, name);
            debug!("[Settings] proton_version saved to config");
        });

        w.on_desktop_entry_changed(move |enabled| {
            info!("[Settings] desktop_entry changed → {enabled}");

            #[cfg(target_os = "linux")]
            {
                crate::classes::desktop::apply(enabled);
                crate::classes::desktop::mark_prompted();
            }

            #[cfg(not(target_os = "linux"))]
            config::set(key::DESKTOP_ENTRY, enabled);

            debug!("[Settings] desktop_entry saved to config");
        });

        // [DEVELOPER]

        let ww = window.clone();
        w.on_developer_mode_changed(move |enabled| {
            info!("[Settings] developer_mode changed → {enabled}");
            config::set(key::DEV_MODE, enabled);
            debug!("[Settings] developer_mode saved to config");
            logwindow::set_visible(enabled, &ww);
        });

        w.on_export_telemetry({
            let ww = window.clone();
            move || {
                info!("[Settings] export_telemetry triggered");
                let ww = ww.clone();
                std::thread::spawn(move || {
                    debug!("[Settings] telemetry export thread spawned");
                    match shared::telemetry::export_telemetry() {
                        Ok(()) => {
                            info!("[Settings] telemetry export complete");
                            ToastHandler::show(
                                &ww,
                                "Exported debugging logs to the logs folder.",
                                "success",
                            );

                            let logs_dir = shared::logger::logs_directory();
                            if let Err(e) = open::that(&logs_dir) {
                                error!("[Settings] failed to open Logs directory: {e}");
                                ToastHandler::show(&ww, "Failed to open logs folder.", "error");
                            }
                        }
                        Err(e) => {
                            error!("[Settings] telemetry export failed: {e}");
                            ToastHandler::show(
                                &ww,
                                format!("Telemetry export failed: {e}"),
                                "error",
                            );
                        }
                    }
                });
            }
        });

        info!("[Settings] bind() complete shortcut all callbacks registered");
    }

    fn translation(w: &MainWindow, index: i32) -> slint::SharedString {
        w.global::<Tr>()
            .get_values()
            .row_data(index.try_into().unwrap_or(0))
            .unwrap_or_default()
    }

    /// The user accepted the warning, so "Ignore Checksum Matching" stays on.
    pub fn confirm_ignore_checksum() {
        info!("[Settings] ignore_checksum warning confirmed");
        config::set(key::IGNORE_CHECKSUM, true);
        debug!("[Settings] ignore_checksum saved to config");
    }

    /// The user backed out of the warning, so flip the switch back off without
    /// touching the config.
    pub fn cancel_ignore_checksum(window: &slint::Weak<MainWindow>) {
        info!("[Settings] ignore_checksum warning cancelled, reverting the toggle");
        if let Some(w) = window.upgrade() {
            w.set_ignore_checksum(false);
        } else {
            error!("[Settings] window handle dead when reverting ignore_checksum");
        }
    }

    pub fn index_to_code(index: i32) -> &'static str {
        let result = LANGUAGES
            .get(index.try_into().unwrap_or(0))
            .map_or("en", |l| l.code.as_str());

        if result == "en" && index != 0 {
            warn!("[Settings] index_to_code: index={index} is out of range ({} langs loaded), falling back to \"en\"", LANGUAGES.len());
        }

        result
    }

    pub fn code_to_index(code: &str) -> Option<i32> {
        let result = LANGUAGES
            .iter()
            .position(|l| l.code == code)
            .map(|i| i.try_into().unwrap_or(0));

        if result.is_none() {
            warn!("[Settings] code_to_index: unknown language code {code:?} shortcut will default to index 0");
        }

        result
    }
}
