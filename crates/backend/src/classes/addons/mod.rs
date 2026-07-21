use std::path::{Path, PathBuf};

use shared::utils::get_bin_path;

pub mod pak;
pub mod scale;

pub fn get_all_addon_paths() -> Option<Vec<PathBuf>> {
    let bin_path = get_bin_path()?;
    let addons_path = bin_path.join("Addons");

    let mut addons = Vec::new();
    if let Ok(entries) = std::fs::read_dir(addons_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                addons.push(path);
            }
        }
    }

    Some(addons)
}

/// All files inside an addon folder except blacklisted extensions
pub fn payload_files(folder: &Path) -> Vec<PathBuf> {
    pub const BLACKLISTED_EXTENSIONS: [&str; 10] = [
        "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "lz4", "md5",
    ];

    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name().is_some_and(|n| {
                    !n.to_str().is_some_and(|s| s.ends_with(".auadd"))
                        && !n
                            .to_str()
                            .is_some_and(|s| BLACKLISTED_EXTENSIONS.iter().any(|e| s.ends_with(e)))
                })
        })
        .collect()
}
