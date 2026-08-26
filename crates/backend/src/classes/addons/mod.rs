use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use log::*;
use shared::utils::get_bin_path;

pub mod pak;
pub mod scale;

pub const CENSORSHIP_DIR: &str = "Censorship";

const REMOVED_ADDONS: [&str; 2] = ["Collectibles", "CooldownTimers"];

pub fn migrate() {
    let Some(bin_path) = get_bin_path() else {
        warn!("Addon migration: could not resolve the Bin path");
        return;
    };

    for name in REMOVED_ADDONS {
        let folder = bin_path.join("Addons").join(name);
        if !folder.is_dir() {
            continue;
        }

        match std::fs::remove_dir_all(&folder) {
            Ok(()) => info!("Addon migration: removed '{}'", folder.display()),
            Err(e) => warn!(
                "Addon migration: could not remove '{}': {e}",
                folder.display()
            ),
        }
    }
}

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

pub fn manifest_path(folder: &Path) -> Option<PathBuf> {
    std::fs::read_dir(folder).ok().and_then(|entries| {
        entries.flatten().map(|e| e.path()).find(|p| {
            p.is_file()
                && p.extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("auadd"))
        })
    })
}

fn manifest_urls(folder: &Path) -> Vec<String> {
    let Some(manifest) = manifest_path(folder) else {
        return Vec::new();
    };

    let Ok(contents) = std::fs::read_to_string(&manifest) else {
        warn!("Addon repair: could not read '{}'", manifest.display());
        return Vec::new();
    };

    contents
        .lines()
        .filter_map(|line| line.split_once('|'))
        .filter(|(key, _)| key.trim() == "FILE")
        .map(|(_, value)| value.trim().to_string())
        .collect()
}

/// Re-downloads `file_name` into `folder` using the addon manifest's `FILE` urls.
pub fn repair_file(folder: &Path, file_name: &str) -> Result<()> {
    if Path::new(file_name).file_name() != Some(OsStr::new(file_name)) {
        return Err(anyhow!("refusing to repair unsafe file name '{file_name}'"));
    }

    let url = manifest_urls(folder)
        .into_iter()
        .find(|url| {
            url.rsplit('/')
                .next()
                .and_then(|last| last.split(['?', '#']).next())
                .is_some_and(|name| name.eq_ignore_ascii_case(file_name))
        })
        .ok_or_else(|| {
            anyhow!(
                "'{file_name}' is not listed in the manifest in '{}'",
                folder.display()
            )
        })?;

    info!("Addon repair: downloading missing '{file_name}' from {url}");
    shared::api::download_to(&url, &folder.join(file_name))
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
                    !n.to_str().is_some_and(|s| {
                        std::path::Path::new(s)
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("auadd"))
                    }) && !n
                        .to_str()
                        .is_some_and(|s| BLACKLISTED_EXTENSIONS.iter().any(|e| s.ends_with(e)))
                })
        })
        .collect()
}
