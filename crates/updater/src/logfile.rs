use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

pub fn init(install_root: &Path) {
    let logs_dir = install_root.join("Logs");
    let path = if fs::create_dir_all(&logs_dir).is_ok() {
        logs_dir.join("updater.log")
    } else {
        install_root.join("updater.log")
    };
    rotate_if_large(&path);
    let _ = LOG_PATH.set(path);
}

fn rotate_if_large(path: &Path) {
    let too_big = fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_LOG_BYTES);
    if !too_big {
        return;
    }
    let previous = path.with_extension("log.1");
    let _ = fs::remove_file(&previous);
    if fs::rename(path, &previous).is_err() {
        let _ = fs::remove_file(path);
    }
}

pub fn log(msg: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let line = format!("[{ts}] {msg}\n");

    #[cfg(debug_assertions)]
    eprint!("{line}");

    if let Some(path) = LOG_PATH.get()
        && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(line.as_bytes());
        }
}
