use log::*;
use shared::classes::games::capabilities::OverlayAddon;
use std::fs;
use std::path::{Path, PathBuf};

const RESERVED_NAMES: [&str; 3] = ["version.dll", "dsound.dll", "dwmapi.dll"];
const SKIPPED_EXTENSIONS: [&str; 13] = [
    "auadd", "md5", "bat", "sh", "zip", "rar", "7z", "tar", "gz", "bz2", "xz", "zst", "lz4",
];

#[derive(Debug, Clone)]
pub struct OverlayEntry {
    pub source: PathBuf,
    pub name: String,
    pub disabled: bool,
}

pub fn entries(addons_path: &Path, addon: &OverlayAddon) -> Vec<OverlayEntry> {
    let folder = addons_path.join(addon.folder);

    fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let source = entry.path();
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let (name, disabled) = file_name.strip_suffix(".disabled").map_or_else(
                || (file_name.clone(), false),
                |stripped| (stripped.to_string(), true),
            );

            if source.is_file()
                && Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| {
                        SKIPPED_EXTENSIONS
                            .iter()
                            .any(|skipped| skipped.eq_ignore_ascii_case(ext))
                    })
            {
                return None;
            }

            if RESERVED_NAMES
                .iter()
                .any(|reserved| reserved.eq_ignore_ascii_case(&name))
            {
                warn!(
                    "Overlay addon '{}': ignoring '{name}', the name is reserved by the loader",
                    addon.folder
                );
                return None;
            }

            Some(OverlayEntry {
                source,
                name,
                disabled,
            })
        })
        .collect()
}

pub fn persist_settings(
    overlays: &[OverlayAddon],
    addons_path: &Path,
    win64: &Path,
    injected: &[PathBuf],
) {
    for addon in overlays {
        for entry in entries(addons_path, addon) {
            let is_persisted = addon
                .persist
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&entry.name));
            let live = win64.join(&entry.name);

            if entry.disabled || !is_persisted || !live.is_file() || !injected.contains(&live) {
                continue;
            }

            match fs::copy(&live, &entry.source) {
                Ok(_) => debug!(
                    "Overlay addon '{}': saved '{}' back to '{}'",
                    addon.folder,
                    live.display(),
                    entry.source.display()
                ),
                Err(e) => warn!(
                    "Overlay addon '{}': could not save '{}' back: {e}",
                    addon.folder,
                    live.display()
                ),
            }
        }
    }
}
