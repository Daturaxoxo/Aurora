use std::fs;
use std::path::PathBuf;

use log::*;

use serde_json::{json, Map, Value};

pub const LANGS: &[(&str, &str)] = &[
    ("English", "en"),
    ("Türkçe", "tr"),
    ("简体中文", "cn"),
    ("繁體中文", "zh-TW"),
    ("日本語", "jp"),
    ("Español", "es"),
    ("Português (Brasil)", "pt-br"),
    ("Deutsch", "de"),
    ("Tiếng Việt", "vi"),
    ("Nederlands", "nl"),
    ("Pусский", "ru"),
    ("Bahasa Indonesia", "id"),
    ("Italiano", "it"),
    ("French", "fr"),
];

pub fn get_lang_code(name: &str) -> Option<&'static str> {
    LANGS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

pub fn get_lang_name(code: &str) -> Option<&'static str> {
    LANGS.iter().find(|(_, c)| *c == code).map(|(n, _)| *n)
}

pub mod key {
    pub const GAME_PATH: &str = "game_path";
    pub const ENGINE_METHOD: &str = "engine_method";
    pub const LANGUAGE: &str = "language";
    pub const DEV_MODE: &str = "dev_mode";
    pub const CENSORSHIP_REMOVE: &str = "csn_rem";
    pub const NO_DRIVE_LINE: &str = "drv_lin";
    pub const HIDE_UID: &str = "uid_rem";
    pub const HIDE_NOTIF_DOTS: &str = "nor_rem";
    pub const DISCORD_RPC: &str = "discord_rpc";
    pub const EXTENSIVE_LOGGING: &str = "extensive_logging";
    pub const EXPORT_CONSOLE: &str = "export_console";
    pub const UI_SCALING: &str = "ui_scaling";
    pub const UI_MINIMIZATION: &str = "ui_min";
    pub const SHOW_NSFW_MODS: &str = "show_nsfw_mods";
    pub const APP_LOCATION: &str = "app_location";
    pub const CUSTOM_ADDONS: &str = "custom_addons";
    pub const CUSTOM_ADDONS_TOGGLED: &str = "custom_addons_toggled";
}

pub fn default_value(k: &str) -> Value {
    match k {
        key::CENSORSHIP_REMOVE | key::HIDE_UID | key::DISCORD_RPC | key::UI_MINIMIZATION => {
            json!(true)
        }

        key::DEV_MODE
        | key::NO_DRIVE_LINE
        | key::HIDE_NOTIF_DOTS
        | key::EXTENSIVE_LOGGING
        | key::SHOW_NSFW_MODS
        | key::CUSTOM_ADDONS_TOGGLED => {
            json!(false)
        }

        key::GAME_PATH | key::APP_LOCATION => json!(""),

        key::CUSTOM_ADDONS => json!([]),

        key::LANGUAGE => json!("en"),

        key::UI_SCALING => json!(1.0),

        // [0 = Default (dsound only)]
        // [1 = Alternate (dsound + version.dll)]
        // [2 = Alternate 2 (dsound + dinput8.dll)]
        key::ENGINE_METHOD => json!(0),
        _ => Value::Null,
    }
}

pub fn get_userdata_path() -> PathBuf {
    dirs::data_local_dir()
        .expect("Could not resolve local data directory")
        .join("Aurora")
        .join("UserData")
}

fn config_file_path() -> PathBuf {
    let config_path = get_userdata_path().join("config.json");

    if !config_path.parent().unwrap().exists() {
        if let Err(e) = fs::create_dir_all(config_path.parent().unwrap()) {
            error!("Failed to create config directory: {e}");
        }
    }

    config_path
}

fn load_raw() -> Map<String, Value> {
    fs::read_to_string(config_file_path())
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .and_then(|val| match val {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default() // Falls back to an empty Map automatically
}

fn save_raw(data: &Map<String, Value>) {
    if let Ok(json_string) = serde_json::to_string_pretty(data) {
        let _ = fs::write(config_file_path(), json_string);
    }
}

pub fn get(k: &str) -> Value {
    load_raw()
        .get(k)
        .cloned()
        .unwrap_or_else(|| default_value(k))
}

pub fn set(k: &str, value: impl Into<Value>) {
    let mut data = load_raw();
    data.insert(k.to_string(), value.into());
    save_raw(&data);
}