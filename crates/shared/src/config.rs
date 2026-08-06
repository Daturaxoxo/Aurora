use anyhow::{anyhow, Context, Result};
use log::*;
use serde_json::{json, Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

const LOCK_TIMEOUT: Duration = Duration::from_secs(2);
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(15);

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
    pub const COOLDOWN_TIMER: &str = "col_tim";
    pub const COLLECTIBLES: &str = "collectibles";
    pub const DISCORD_RPC: &str = "discord_rpc";
    pub const EXPORT_CONSOLE: &str = "export_console";
    pub const UI_SCALING: &str = "ui_scaling";
    pub const UI_MINIMIZATION: &str = "ui_min";
    pub const SHOW_NSFW_MODS: &str = "show_nsfw_mods";
    pub const APP_LOCATION: &str = "app_location";
    pub const CUSTOM_ADDONS: &str = "custom_addons";
    pub const CUSTOM_ADDONS_TOGGLED: &str = "custom_addons_toggled";
    pub const GB_NSFW: &str = "gb_show_nsfw";
    pub const MODMNG_NOTES: &str = "mod_notes";
    pub const MODMNG_DISPLAY_NAMES: &str = "mod_display_names";
    pub const MODMNG_VIEW_GRID: &str = "mod_view_grid";
    pub const MOD_NOTES: &str = "module_notes";
    pub const MOD_DISPLAY_NAMES: &str = "module_display_names";
    pub const SCREENSHOT_FAVORITES: &str = "screenshot_favorites";
    pub const SELECTED_GAME: &str = "selected_game";
    pub const PROTON_ARGS: &str = "proton_args";
    pub const QUICK_START_CREATED: &str = "quick_start_created";
    pub const DESKTOP_ENTRY: &str = "desktop_entry";
    pub const DESKTOP_ENTRY_PROMPTED: &str = "desktop_entry_prompted";
    pub const ERROR_TELEMETRY: &str = "error_telemetry";
}

pub fn default_value(k: &str) -> Value {
    match k {
        key::DISCORD_RPC | key::UI_MINIMIZATION | key::HIDE_UID | key::ERROR_TELEMETRY => {
            json!(true)
        }

        key::DEV_MODE
        | key::NO_DRIVE_LINE
        | key::HIDE_NOTIF_DOTS
        | key::SHOW_NSFW_MODS
        | key::CUSTOM_ADDONS_TOGGLED
        | key::CENSORSHIP_REMOVE
        | key::COLLECTIBLES
        | key::COOLDOWN_TIMER
        | key::GB_NSFW
        | key::QUICK_START_CREATED
        | key::DESKTOP_ENTRY
        | key::DESKTOP_ENTRY_PROMPTED => {
            json!(false)
        }

        key::GAME_PATH | key::APP_LOCATION | key::SELECTED_GAME | key::PROTON_ARGS => json!(""),

        key::CUSTOM_ADDONS
        | key::MODMNG_NOTES
        | key::MODMNG_DISPLAY_NAMES
        | key::MODMNG_VIEW_GRID
        | key::MOD_NOTES
        | key::MOD_DISPLAY_NAMES
        | key::SCREENSHOT_FAVORITES => json!([]),

        key::LANGUAGE => json!("en"),

        key::UI_SCALING => json!(1.0),

        // [0 = Default (version.dll)]
        // [1 = Alternate (dsound)]
        key::ENGINE_METHOD => json!(0),
        _ => Value::Null,
    }
}

pub fn get_userdata_path() -> PathBuf {
    dirs::config_dir()
        .expect("Could not resolve local data directory")
        .join("Aurora")
        .join("UserData")
}

pub fn config_file_path() -> PathBuf {
    let config_path = get_userdata_path().join("config.json");

    if !config_path.parent().unwrap().exists() {
        if let Err(e) = fs::create_dir_all(config_path.parent().unwrap()) {
            error!("Failed to create config directory: {e}");
        }
    }

    config_path
}

fn lock_file_path() -> PathBuf {
    config_file_path().with_extension("json.lock")
}

#[cfg(target_os = "windows")]
fn try_lock_file(path: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
}

#[cfg(target_os = "linux")]
fn try_lock_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::io::AsRawFd;

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)?;

    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(file)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn acquire_cross_process_lock() -> Option<File> {
    let path = lock_file_path();
    let started = Instant::now();

    loop {
        match try_lock_file(&path) {
            Ok(file) => return Some(file),
            Err(e) => {
                if started.elapsed() >= LOCK_TIMEOUT {
                    warn!(
                        "Gave up waiting for {} after {:?}: {e}",
                        path.display(),
                        LOCK_TIMEOUT
                    );
                    return None;
                }

                thread::sleep(LOCK_RETRY_DELAY);
            }
        }
    }
}

fn load_raw() -> Result<Map<String, Value>> {
    let path = config_file_path();

    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e).context(format!("Failed to read {}", path.display())),
    };

    if contents.trim().is_empty() {
        return Ok(Map::new());
    }

    let value: Value = serde_json::from_str(&contents)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    match value {
        Value::Object(map) => Ok(map),
        _ => Err(anyhow!("{} does not hold a JSON object", path.display())),
    }
}

fn save_raw(data: &Map<String, Value>) -> bool {
    let json_string = match serde_json::to_string_pretty(data) {
        Ok(json_string) => json_string,
        Err(e) => {
            error!("Failed to serialize the config: {e}");
            return false;
        }
    };

    let path = config_file_path();

    let tmp = path.with_extension("json.tmp");
    if let Err(e) = fs::write(&tmp, &json_string) {
        error!("Failed to write {}: {e}", tmp.display());
        return false;
    }

    if let Err(e) = fs::rename(&tmp, &path) {
        warn!("Could not replace {} atomically: {e}", path.display());
        let _ = fs::remove_file(&tmp);

        if let Err(e) = fs::write(&path, &json_string) {
            error!("Failed to write {}: {e}", path.display());
            return false;
        }
    }

    true
}

pub fn get(k: &str) -> Value {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let _cross_guard = acquire_cross_process_lock();

    match load_raw() {
        Ok(data) => data.get(k).cloned().unwrap_or_else(|| default_value(k)),
        Err(e) => {
            error!("Falling back to the default value of {k}: {e:#}");
            default_value(k)
        }
    }
}

pub fn modify(f: impl FnOnce(&mut Map<String, Value>)) -> bool {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let _cross_guard = acquire_cross_process_lock();

    let mut data = match load_raw() {
        Ok(data) => data,
        Err(e) => {
            error!("Refusing to overwrite the config, it could not be read: {e:#}");
            return false;
        }
    };

    f(&mut data);
    save_raw(&data)
}

pub fn set(k: &str, value: impl Into<Value>) {
    let value = value.into();

    modify(|data| {
        data.insert(k.to_string(), value);
    });
}

pub fn get_all_configs() -> Map<String, Value> {
    let _guard = CONFIG_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let _cross_guard = acquire_cross_process_lock();

    load_raw().unwrap_or_else(|e| {
        error!("Failed to load the config: {e:#}");
        Map::new()
    })
}
