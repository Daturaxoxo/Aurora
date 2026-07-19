use std::path::PathBuf;

use jwalk::DirEntry;

use crate::{
    classes::info::paths::CLIENT_PAK_DIR,
    config::{self, key},
};

const VERSION: &[u8] = include_bytes!("../../../production/VERSION");

pub fn get_local_version() -> String {
    String::from_utf8_lossy(VERSION).to_string()
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
/// - In release mode, it returns the path to the Bin folder in the executable directory.
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
        std::env::current_exe()
            .expect("Addons Manager could not resolve exe path")
            .parent()
            .map(PathBuf::from)
            .and_then(|p| Some(p.join("Bin")))
    }
}

pub fn read_dir_recursive(path: &PathBuf) -> Vec<DirEntry<((), ())>> {
    use jwalk::WalkDir;

    let mut paths = vec![];

    for entry in WalkDir::new(path) {
        paths.push(entry.ok().unwrap());
    }

    paths
}

pub fn format_size(bytes: u64) -> String {
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
