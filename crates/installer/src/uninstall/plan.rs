use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;
use shared::classes::info::paths::CLIENT_PAK_DIR;
use shared::config::{get_userdata_path, key};
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

/// Folder the user picked by hand after automatic detection failed.
static APP_DIR_OVERRIDE: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn set_app_dir_override(dir: PathBuf) {
    *APP_DIR_OVERRIDE.lock().unwrap() = Some(dir);
}

fn app_dir_override() -> Option<PathBuf> {
    APP_DIR_OVERRIDE.lock().unwrap().clone()
}

pub struct Plan {
    pub data_dir: PathBuf,
    pub app_dir: Option<PathBuf>,
    pub app_dir_error: Option<String>,
    pub mods_dir: Option<PathBuf>,
    pub backup_dir: PathBuf,
}

impl Plan {
    pub fn resolve() -> Self {
        let config = read_config();

        let mods_dir = path_value(&config, key::GAME_PATH).map(|game| game.join(CLIENT_PAK_DIR));

        Self::from_candidates(
            aurora_data_dir(),
            candidate_app_dirs(&config),
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
        Self::from_candidates(
            data_dir,
            app_dir.into_iter().collect(),
            mods_dir,
            backup_dir,
        )
    }

    fn from_candidates(
        data_dir: PathBuf,
        candidates: Vec<PathBuf>,
        mods_dir: Option<PathBuf>,
        backup_dir: PathBuf,
    ) -> Self {
        let (app_dir, app_dir_error) = select_app_dir(candidates);

        Self {
            data_dir,
            app_dir,
            app_dir_error,
            mods_dir: mods_dir.filter(|mods| mods.is_dir()),
            backup_dir,
        }
    }
}

/// Takes the first candidate that verifies. If none do, reports why the best
/// guess was rejected.
fn select_app_dir(candidates: Vec<PathBuf>) -> (Option<PathBuf>, Option<String>) {
    let mut first_error = None;

    for dir in candidates {
        match verify_app_dir(&dir) {
            Ok(()) => return (Some(dir), None),
            Err(e) => first_error.get_or_insert(e),
        };
    }

    (None, first_error)
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

    for name in [ipc::AURORA_EXE, ipc::UPDATER_EXE] {
        if !dir.join(name).is_file() {
            return Err(format!(
                "refusing to touch {}: it does not contain {name}",
                dir.display()
            ));
        }
    }

    Ok(())
}

fn candidate_app_dirs(config: &Value) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    candidates.extend(app_dir_override());
    candidates.extend(
        path_value(config, key::APP_LOCATION)
            .as_deref()
            .and_then(Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
            .map(Path::to_path_buf),
    );
    candidates.extend(sibling_app_dir());

    candidates.dedup();
    candidates
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
    Some(std::env::current_exe().ok()?.parent()?.to_path_buf())
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
