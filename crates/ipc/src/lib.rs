pub mod lock;
pub mod manifest;
pub mod protocol;

use std::path::PathBuf;
use std::time::Duration;

#[cfg(windows)]
pub const MANIFEST_URL_PRIMARY: &str = "https://host.getaurora.moe/files/app/windows/manifest.json";
#[cfg(target_os = "linux")]
pub const MANIFEST_URL_PRIMARY: &str = "https://host.getaurora.moe/files/app/linux/manifest.json";
#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("unsupported target: no manifest URL");

// TODO: fill in fallback before release
pub const MANIFEST_URL_FALLBACK: &str = "";

pub const MAIN_PIPE_NAME: &str = "aurora-updater";
pub const INIT_PIPE_NAME: &str = "aurora-updater-init";

#[cfg(windows)]
pub const AURORA_EXE: &str = "Aurora.exe";
#[cfg(not(windows))]
pub const AURORA_EXE: &str = "Aurora";

#[cfg(windows)]
pub const UPDATER_EXE: &str = "updater.exe";
#[cfg(not(windows))]
pub const UPDATER_EXE: &str = "updater";

pub const LOCAL_MANIFEST_FILE: &str = ".aurora_manifest.json";

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

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);
pub const INIT_CONFIRM_TIMEOUT: Duration = Duration::from_secs(15);

pub fn install_root() -> PathBuf {
    std::env::current_exe()
        .expect("could not resolve exe path")
        .parent()
        .map(PathBuf::from)
        .expect("exe has no parent directory")
}
