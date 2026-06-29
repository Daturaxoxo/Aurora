use std::path::PathBuf;

use jwalk::DirEntry;

use crate::{
    classes::info::CLIENT_PAK_DIR,
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

pub fn read_dir_recursive(path: &PathBuf) -> Vec<DirEntry<((), ())>> {
    use jwalk::WalkDir;

    let mut paths = vec![];

    for entry in WalkDir::new(path) {
        paths.push(entry.ok().unwrap());
    }

    paths
}
