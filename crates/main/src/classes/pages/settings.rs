#[allow(dead_code)]
use crate::MainWindow;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use shared::config::{self, key};

#[derive(serde::Deserialize)]
struct LangEntry {
    name: String,
    code: String,
}

static LANGUAGES: Lazy<Vec<LangEntry>> = Lazy::new(|| {
    serde_json::from_str(include_str!("../../../../../production/Langs/lang-codes.json"))
        .expect("lang-codes.json is missing or malformed!")
});

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

        // Launcher
        let raw_path = config::get(key::GAME_PATH);
        let game_path = raw_path.as_str().unwrap_or("").to_string();
        debug!("[Settings] game_path: raw={raw_path:?} → {game_path:?}");
        if game_path.is_empty() {
            warn!("[Settings] game_path is empty - user has not set a game directory yet");
        }
        w.set_game_directory(game_path.into());

        let raw_engine = config::get(key::ENGINE_METHOD);
        let engine_method = raw_engine.as_i64().unwrap_or(0) as i32;
        debug!("[Settings] engine_method: raw={raw_engine:?} → {engine_method}");
        w.set_engine_method_index(engine_method);

        // Developer
        let raw_dev = config::get(key::DEV_MODE);
        let dev_mode = raw_dev.as_bool().unwrap_or(false);
        debug!("[Settings] developer_mode: raw={raw_dev:?} → {dev_mode}");
        w.set_developer_mode(dev_mode);

        let raw_logging = config::get(key::EXTENSIVE_LOGGING);
        let extensive_logging = raw_logging.as_bool().unwrap_or(false);
        debug!("[Settings] extensive_logging: raw={raw_logging:?} → {extensive_logging}");
        w.set_extensive_logging(extensive_logging);

        info!("[Settings] load() complete shortcut all config values applied to UI");
    }

    fn bind(window: &slint::Weak<MainWindow>) {
        info!("[Settings] bind() started shortcut registering UI callbacks");
        let w = window.unwrap();

        // [GENERAL]

        w.on_language_index_changed(move |index| {
            let code = Self::index_to_code(index);
            info!("[Settings] language changed → index={index}, code={code:?}");
            config::set(key::LANGUAGE, code);
            debug!("[Settings] language saved to config");
        });

        w.on_interface_minimization_changed(move |enabled| {
            info!("[Settings] interface_minimization changed → {enabled}");
            config::set(key::UI_MINIMIZATION, enabled);
            debug!("[Settings] interface_minimization saved to config");
        });

        w.on_discord_rpc_changed(move |enabled| {
            info!("[Settings] discord_rpc changed → {enabled}");
            config::set(key::DISCORD_RPC, enabled);
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
                        let path_str: String = path.to_string_lossy().into_owned();
                        info!("[Settings] game directory selected → {path_str:?}");
                        config::set(key::GAME_PATH, path_str.clone());
                        debug!("[Settings] game_path saved to config");
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

        w.on_engine_method_index_changed(move |index| {
            info!("[Settings] engine_method changed → {index}");
            config::set(key::ENGINE_METHOD, index);
            debug!("[Settings] engine_method saved to config");
        });

        // [DEVELOPER]

        w.on_developer_mode_changed(move |enabled| {
            info!("[Settings] developer_mode changed → {enabled}");
            // w.set_show_dev_console(enabled); // TODO: add show-dev-console to MainWindow
            config::set(key::DEV_MODE, enabled);
            debug!("[Settings] developer_mode saved to config");
        });

        w.on_extensive_logging_changed(move |enabled| {
            info!("[Settings] extensive_logging changed → {enabled}");
            config::set(key::EXTENSIVE_LOGGING, enabled);
            debug!("[Settings] extensive_logging saved to config");
        });

        w.on_export_telemetry(move || {
            info!("[Settings] export_telemetry triggered");
            std::thread::spawn(|| {
                debug!("[Settings] telemetry export thread spawned");
                // TODO: shared::telemetry::export()
                info!("[Settings] telemetry export complete (stub)");
            });
        });

        info!("[Settings] bind() complete shortcut all callbacks registered");
    }

    // ------------------------------------------------------------------ //
    // Helpers
    // ------------------------------------------------------------------ //

    /// `0 → "en"`, `1 → "tr"`, etc. Falls back to `"en"` for out-of-range.
    pub fn index_to_code(index: i32) -> &'static str {
        let result = LANGUAGES
            .get(index as usize)
            .map(|l| l.code.as_str())
            .unwrap_or("en");

        if result == "en" && index != 0 {
            warn!("[Settings] index_to_code: index={index} is out of range ({} langs loaded), falling back to \"en\"", LANGUAGES.len());
        }

        result
    }

    /// `"tr" → 1`, returns `None` if the code is unknown.
    pub fn code_to_index(code: &str) -> Option<i32> {
        let result = LANGUAGES
            .iter()
            .position(|l| l.code == code)
            .map(|i| i as i32);

        if result.is_none() {
            warn!("[Settings] code_to_index: unknown language code {code:?} shortcut will default to index 0");
        }

        result
    }
}