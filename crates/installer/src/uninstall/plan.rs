use std::path::{Path, PathBuf};

use serde_json::Value;
use shared::classes::info::paths::CLIENT_PAK_DIR;
use shared::config::{get_userdata_path, key};
const BUILD_DIRS: [&str; 2] = ["debug", "release"];
pub struct Plan {
    pub data_dir: PathBuf,
    pub app_dir: Option<PathBuf>,
    pub mods_dir: Option<PathBuf>,
    pub dev_build: bool,
}

impl Plan {
    pub fn resolve() -> Self {
        let config = read_config();

        let app_dir = path_value(&config, key::APP_LOCATION)
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(sibling_app_dir);

        let dev_build = app_dir.as_deref().is_some_and(is_build_dir);

        let mods_dir = path_value(&config, key::GAME_PATH)
            .map(|game| game.join(CLIENT_PAK_DIR))
            .filter(|mods| mods.is_dir());

        Self {
            data_dir: aurora_data_dir(),
            app_dir,
            mods_dir,
            dev_build,
        }
    }
}

fn aurora_data_dir() -> PathBuf {
    get_userdata_path()
        .parent()
        .map_or_else(|| PathBuf::from("Aurora"), Path::to_path_buf)
}

fn read_config() -> Value {
    std::fs::read_to_string(get_userdata_path().join("config.json"))
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or(Value::Null)
}

fn path_value(config: &Value, key: &str) -> Option<PathBuf> {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn sibling_app_dir() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    dir.join(ipc::AURORA_EXE).exists().then_some(dir)
}

fn is_build_dir(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| BUILD_DIRS.contains(&name.to_lowercase().as_str()))
}
