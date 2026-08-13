use anyhow::{anyhow, Result};
use log::*;
use std::path::{Path, PathBuf};

use jwalk::DirEntry;

use crate::{
    classes::info::paths::CLIENT_PAK_DIR,
    config::{self, key},
};

const VERSION: &[u8] = include_bytes!("../../../production/VERSION");

pub fn get_local_version() -> String {
    String::from_utf8_lossy(VERSION).trim().to_string()
}

/// Flattens an error and every one of its sources into a single line.
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        out.push_str(" <- ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}

pub fn get_current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .cast_signed()
}

pub fn get_mods_path() -> Option<PathBuf> {
    config::get(key::GAME_PATH)
        .as_str()
        .map(PathBuf::from)
        .map(|p| p.join(CLIENT_PAK_DIR))
}

/// Returns the path to the bin folder:
/// - In debug mode, it returns the path to the Bin folder in the project directory.
/// - In release mode, it returns the path to the Bin folder inside the state
///   root: the executable's directory on Windows, and the XDG data directory
///   on Linux, where an `AppImage` cannot write next to itself.
pub fn get_bin_path() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .map(|p| p.join("Bin"))
    }
    #[cfg(not(debug_assertions))]
    {
        Some(ipc::state_root().join("Bin"))
    }
}

pub fn read_dir_recursive(path: &PathBuf) -> Vec<DirEntry<((), ())>> {
    use jwalk::WalkDir;

    let mut paths = vec![];

    for e in WalkDir::new(path).into_iter().flatten() {
        paths.push(e);
    }

    paths
}

pub fn format_bytes(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let b = bytes as f64;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", b / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", b / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

pub fn get_cache_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| ".".into())
        .join("Aurora")
        .join("Cache")
}

pub fn get_gamebanana_download_dir() -> PathBuf {
    std::env::temp_dir().join("Aurora").join("GameBanana")
}

pub fn write_if_changed(path: &Path, contents: &[u8]) -> Result<()> {
    if std::fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)?;
    std::fs::write(path, contents)?;
    info!("Wrote {}", path.display());

    Ok(())
}

pub fn remove_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            info!("Removed {}", path.display());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn open_folder(path: &Path) -> Result<()> {
    if !path.is_dir() {
        return Err(anyhow!("{} is not a directory", path.display()));
    }

    if let Err(e) = open::that(path) {
        warn!("Failed to open folder: {}", e);

        #[cfg(target_os = "windows")]
        return match std::process::Command::new("open")
            .arg(path)
            .status()
            .map_err(|e| anyhow!("Failed to open folder with fallback: {}", e))
        {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };

        #[cfg(target_os = "linux")]
        return match std::process::Command::new("xdg-open")
            .arg(path)
            .status()
            .map_err(|e| anyhow!("Failed to open folder with fallback: {}", e))
        {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
    }
    Ok(())
}
