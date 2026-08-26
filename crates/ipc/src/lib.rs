pub mod lock;
pub mod manifest;
pub mod protocol;

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

pub const DOWNLOAD_BASE_PRIMARY: &str = "https://host.getaurora.moe/files/app/";
pub const DOWNLOAD_BASE_FALLBACK: &str =
    "https://github.com/Daturaxoxo/AuroraInstallation/releases/latest/download/";

#[cfg(windows)]
pub const MANIFEST_URL_PRIMARY: &str = "https://host.getaurora.moe/files/app/windows/manifest.json";
#[cfg(target_os = "linux")]
pub const MANIFEST_URL_PRIMARY: &str = "https://host.getaurora.moe/files/app/linux/manifest.json";
#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("unsupported target: no manifest URL");

#[cfg(windows)]
pub const MANIFEST_URL_FALLBACK: &str = concat!(
    "https://github.com/Daturaxoxo/AuroraInstallation/releases/latest/download/",
    "windows__manifest.json"
);
#[cfg(target_os = "linux")]
pub const MANIFEST_URL_FALLBACK: &str = concat!(
    "https://github.com/Daturaxoxo/AuroraInstallation/releases/latest/download/",
    "linux__manifest.json"
);

const MAIN_PIPE_BASE: &str = "aurora-updater";
const INIT_PIPE_BASE: &str = "aurora-updater-init";
const ONECLICK_PIPE_BASE: &str = "aurora-oneclick";

pub fn manifest_urls() -> impl Iterator<Item = &'static str> {
    [MANIFEST_URL_PRIMARY, MANIFEST_URL_FALLBACK]
        .into_iter()
        .filter(|url| !url.is_empty())
}

pub fn install_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        let root = instance_root();
        let mut key = root.to_string_lossy().into_owned();
        if cfg!(windows) {
            key = key.to_lowercase();
        }
        let digest = manifest::hash_bytes(key.as_bytes());
        digest[..8].to_owned()
    })
}

pub fn main_pipe_name() -> String {
    format!("{MAIN_PIPE_BASE}-{}", install_id())
}

pub const LEGACY_MAIN_PIPE_NAME: &str = MAIN_PIPE_BASE;
pub fn main_pipe_candidates() -> Vec<String> {
    vec![main_pipe_name(), LEGACY_MAIN_PIPE_NAME.to_owned()]
}
pub const PIPE_ARG: &str = "--pipe";

pub fn init_pipe_name() -> String {
    format!("{INIT_PIPE_BASE}-{}", install_id())
}

pub fn oneclick_pipe_name() -> String {
    format!("{ONECLICK_PIPE_BASE}-{}", install_id())
}

#[cfg(windows)]
pub const AURORA_EXE: &str = "Aurora.exe";
#[cfg(not(windows))]
pub const AURORA_EXE: &str = "Aurora";

#[cfg(windows)]
pub const UPDATER_EXE: &str = "updater.exe";
#[cfg(not(windows))]
pub const UPDATER_EXE: &str = "updater";

pub const LOCAL_MANIFEST_FILE: &str = ".aurora_manifest.json";

#[cfg(target_os = "linux")]
pub const APPIMAGE_NAME: &str = "Aurora-x86_64.AppImage";

pub const AURORA_LOCK_FILE: &str = "aurora.lock";
pub const UPDATER_LOCK_FILE: &str = "updater.lock";

/// Passed by the updater when relaunching Aurora after an exe swap
pub const POST_UPDATE_ARG: &str = "--post-update";
/// Passed by the updater when relaunching the old Aurora after a failed exe
/// swap, so that one run skips the startup update check and does not loop
pub const SKIP_UPDATE_CHECK_ARG: &str = "--skip-update-check";
/// Passed by Aurora when respawning itself after a silent update, so the new
/// instance retries the singleton lock while the old instance shuts down
pub const RELAUNCH_ARG: &str = "--relaunch";
/// Launches the game headlessly: no window, inject, wait for NTE to exit,
/// sanitize, then quit
pub const QUICK_START_ARG: &str = "--quick-start";

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);
pub const INIT_CONFIRM_TIMEOUT: Duration = Duration::from_secs(60);
pub const UPDATER_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
pub const UPDATER_CONNECT_ATTEMPTS: u32 = 10;
pub const UPDATER_CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);
pub const AURORA_EXIT_TIMEOUT: Duration = Duration::from_secs(30);
pub const RELAUNCH_LOCK_TIMEOUT: Duration = Duration::from_secs(60);

pub const HTTP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const HTTP_STALL_TIMEOUT: Duration = Duration::from_secs(20);
pub const HTTP_MANIFEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const HTTP_DOWNLOAD_TIMEOUT: Duration = Duration::from_mins(30);

pub fn user_agent(version: &str) -> String {
    format!("AuroraLauncher/{}", version.trim())
}

pub fn install_root() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn instance_root() -> PathBuf {
    #[cfg(target_os = "linux")]
    if is_appimage() {
        return state_root();
    }

    install_root()
}

#[cfg(target_os = "linux")]
pub fn state_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Aurora")
}

#[cfg(target_os = "linux")]
pub fn is_appimage() -> bool {
    appimage_path().is_some() || appimage_mount().is_some()
}

#[cfg(target_os = "linux")]
pub fn appimage_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE").map(PathBuf::from)
}

#[cfg(target_os = "linux")]
pub fn appimage_mount() -> Option<PathBuf> {
    std::env::var_os("APPDIR").map(PathBuf::from)
}
