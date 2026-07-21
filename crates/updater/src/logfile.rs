use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(install_root: &Path) {
    let logs_dir = install_root.join("Logs");
    let path = if fs::create_dir_all(&logs_dir).is_ok() {
        logs_dir.join("updater.log")
    } else {
        install_root.join("updater.log")
    };
    let _ = LOG_PATH.set(path);
}

pub fn log(msg: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let line = format!("[{ts}] {msg}\n");

    #[cfg(debug_assertions)]
    eprint!("{line}");

    if let Some(path) = LOG_PATH.get() {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
    }
}
