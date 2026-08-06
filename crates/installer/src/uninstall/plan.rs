use std::path::{Path, PathBuf};

use serde_json::Value;
use shared::classes::info::paths::CLIENT_PAK_DIR;
use shared::config::{get_userdata_path, key};
const BUILD_DIRS: [&str; 2] = ["debug", "release"];
const PROTECTED_ENV_VARS: [&str; 9] = [
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "ProgramData",
    "SystemRoot",
    "windir",
    "PUBLIC",
    "USERPROFILE",
    "LOCALAPPDATA",
];
pub struct Plan {
    pub data_dir: PathBuf,
    pub app_dir: Option<PathBuf>,
    pub app_dir_error: Option<String>,
    pub mods_dir: Option<PathBuf>,
    pub backup_dir: PathBuf,
    pub dev_build: bool,
}

impl Plan {
    pub fn resolve() -> Self {
        let config = read_config();

        let mods_dir = path_value(&config, key::GAME_PATH)
            .map(|game| game.join(CLIENT_PAK_DIR))
            .filter(|mods| mods.is_dir());

        Self::from_paths(
            aurora_data_dir(),
            candidate_app_dir(&config),
            mods_dir,
            default_backup_dir(),
        )
    }

    pub fn from_paths(
        data_dir: PathBuf,
        app_dir: Option<PathBuf>,
        mods_dir: Option<PathBuf>,
        backup_dir: PathBuf,
    ) -> Self {
        let (app_dir, app_dir_error) = match app_dir {
            Some(dir) => match verify_app_dir(&dir) {
                Ok(()) => (Some(dir), None),
                Err(e) => (None, Some(e)),
            },
            None => (None, None),
        };

        let dev_build = app_dir.as_deref().is_some_and(is_build_dir);

        Self {
            data_dir,
            app_dir,
            app_dir_error,
            mods_dir: mods_dir.filter(|mods| mods.is_dir()),
            backup_dir,
            dev_build,
        }
    }
}

pub fn default_backup_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Aurora Mods Backup")
}

pub fn verify_app_dir(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a directory", dir.display()));
    }

    if is_protected_dir(dir) {
        return Err(format!(
            "refusing to touch {}: it is a system or user folder, not an Aurora installation",
            dir.display()
        ));
    }

    if !dir.join(ipc::AURORA_EXE).is_file() {
        return Err(format!(
            "refusing to touch {}: it does not contain {}",
            dir.display(),
            ipc::AURORA_EXE
        ));
    }

    if !dir.join(ipc::LOCAL_MANIFEST_FILE).is_file() && !is_build_dir(dir) {
        return Err(format!(
            "refusing to touch {}: it does not contain {}",
            dir.display(),
            ipc::LOCAL_MANIFEST_FILE
        ));
    }

    Ok(())
}

fn candidate_app_dir(config: &Value) -> Option<PathBuf> {
    path_value(config, key::APP_LOCATION)
        .as_deref()
        .and_then(Path::parent)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(sibling_app_dir)
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

fn is_protected_dir(dir: &Path) -> bool {
    let target = normalize(dir);
    if target.parent().is_none() {
        return true;
    }
    protected_dirs()
        .iter()
        .any(|protected| normalize(protected) == target)
}

fn normalize(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn protected_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        dirs::home_dir(),
        dirs::desktop_dir(),
        dirs::download_dir(),
        dirs::document_dir(),
        dirs::picture_dir(),
        dirs::video_dir(),
        dirs::audio_dir(),
        dirs::data_dir(),
        dirs::data_local_dir(),
        dirs::config_dir(),
        dirs::executable_dir(),
    ]
    .into_iter()
    .flatten()
    .collect();

    for var in PROTECTED_ENV_VARS {
        if let Some(value) = std::env::var_os(var) {
            let path = PathBuf::from(value);
            dirs.push(path.join("System32"));
            dirs.push(path);
        }
    }

    dirs
}
