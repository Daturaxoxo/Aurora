use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use log::*;

use shared::utils::get_mods_path;

const LEGACY_PREFIX: &str = "~mod";

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub migrated: usize,
    pub failures: Vec<String>,
}

/// Folders in the Paks directory that start with `~mod`, case-insensitively.
pub fn find_legacy_folders() -> Vec<PathBuf> {
    let Some(pak_dir) = get_mods_path().and_then(|p| p.parent().map(Path::to_path_buf)) else {
        debug!("[LegacyMods] the game folder is not set, skipping the check");
        return vec![];
    };

    let folders: Vec<PathBuf> = fs::read_dir(&pak_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_lowercase()
                .starts_with(LEGACY_PREFIX)
        })
        .map(|entry| entry.path())
        .collect();

    if !folders.is_empty() {
        info!(
            "[LegacyMods] found {} legacy mod folder(s) in {}",
            folders.len(),
            pak_dir.display()
        );
    }

    folders
}

/// Moves everything out of the legacy folders into `AuroraMods`.
pub fn migrate(folders: &[PathBuf]) -> Result<MigrationReport> {
    let mods_path =
        get_mods_path().ok_or_else(|| anyhow!("the game folder is not set in the settings"))?;

    fs::create_dir_all(&mods_path)?;

    let mut report = MigrationReport::default();

    for folder in folders {
        migrate_folder(folder, &mods_path, &mut report);
        remove_if_empty(folder);
    }

    info!(
        "[LegacyMods] migrated {} mod(s) with {} failure(s)",
        report.migrated,
        report.failures.len()
    );

    Ok(report)
}

fn migrate_folder(folder: &Path, mods_path: &Path, report: &mut MigrationReport) {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(e) => {
            report.failures.push(format!("{}: {e}", folder.display()));
            return;
        }
    };

    // Loose files sharing a stem (mod.pak, mod.utoc, mod.ucas) belong to the
    // same mod, so they travel into one folder named after that stem.
    let mut loose: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                report.failures.push(format!("{}: {e}", folder.display()));
                continue;
            }
        };

        let path = entry.path();

        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            let name = entry.file_name().to_string_lossy().into_owned();
            match move_into(&[path], &unique_destination(mods_path, &name)) {
                Ok(()) => report.migrated += 1,
                Err(e) => report.failures.push(e),
            }
            continue;
        }

        let stem = path
            .file_stem()
            .map(OsStr::to_string_lossy)
            .unwrap_or_default()
            .into_owned();

        loose.entry(stem).or_default().push(path);
    }

    for (stem, files) in loose {
        match move_into(&files, &unique_destination(mods_path, &stem)) {
            Ok(()) => report.migrated += 1,
            Err(e) => report.failures.push(e),
        }
    }
}

/// `name`, or `name (2)`, `name (3)`... when `AuroraMods` already holds it.
fn unique_destination(mods_path: &Path, name: &str) -> PathBuf {
    let candidate = mods_path.join(name);
    if !candidate.exists() {
        return candidate;
    }

    for suffix in 2.. {
        let candidate = mods_path.join(format!("{name} ({suffix})"));
        if !candidate.exists() {
            return candidate;
        }
    }

    unreachable!()
}

fn move_into(sources: &[PathBuf], destination: &Path) -> std::result::Result<(), String> {
    let move_one = |source: &PathBuf| -> std::io::Result<()> {
        // A directory moves as itself; loose files move inside a folder named
        // after their stem, since the mod manager only lists folders.
        let target = if source.is_dir() {
            destination.to_path_buf()
        } else {
            fs::create_dir_all(destination)?;
            destination.join(source.file_name().unwrap_or_default())
        };

        fs::rename(source, &target)
    };

    for source in sources {
        if let Err(e) = move_one(source) {
            error!(
                "[LegacyMods] could not move {} to {}: {e}",
                source.display(),
                destination.display()
            );
            return Err(format!("{}: {e}", source.display()));
        }

        info!(
            "[LegacyMods] moved {} to {}",
            source.display(),
            destination.display()
        );
    }

    Ok(())
}

fn remove_if_empty(folder: &Path) {
    let empty = fs::read_dir(folder).is_ok_and(|mut entries| entries.next().is_none());
    if !empty {
        info!(
            "[LegacyMods] keeping {}, it still holds files",
            folder.display()
        );
        return;
    }

    match fs::remove_dir(folder) {
        Ok(()) => info!("[LegacyMods] removed the empty {}", folder.display()),
        Err(e) => warn!("[LegacyMods] could not remove {}: {e}", folder.display()),
    }
}
