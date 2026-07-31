use std::{ffi::OsStr, path::PathBuf, time::Instant};

use crate::classes::games::markers::{find_marker, folder_name_matches};
use anyhow::{anyhow, Result};
use jwalk::WalkDir;
use log::*;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};

use crate::{
    classes::info::{
        paths::{CLIENT_WIN64, GAME_FOLDER_NAME},
        version::LAUNCHER_MAP,
    },
    config::{get, key, set},
};

fn selected_game_folder_name() -> String {
    let selected = get(key::SELECTED_GAME);
    match selected.as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => GAME_FOLDER_NAME.to_string(),
    }
}

fn normalize_game_root(path: PathBuf) -> PathBuf {
    let suffix = PathBuf::from(CLIENT_WIN64);
    if path.ends_with(&suffix) {
        let mut root = path.clone();
        for _ in suffix.components() {
            root.pop();
        }
        warn!(
            "Stored game path {} pointed inside the client tree; normalized to {}",
            path.display(),
            root.display()
        );
        return root;
    }
    path
}

#[allow(clippy::ptr_arg)]
pub fn validate_game_path(path: &PathBuf, game_folder_name: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    for launcher in LAUNCHER_MAP {
        let launcher_path = path.join(launcher.0);
        if launcher_path.exists() {
            trace!(
                "Validated {} via launcher marker {}",
                path.display(),
                launcher.0
            );
            return Ok(true);
        }
    }

    if let Some(markers) = find_marker(game_folder_name) {
        for marker in markers {
            if path.join(marker).exists() {
                trace!("Validated {} via game marker {marker}", path.display());
                return Ok(true);
            }
        }
    } else {
        error!("No marker set registered for game '{game_folder_name}'");
    }

    warn!(
        "Validation failed for {}: no launcher or marker match",
        path.display()
    );
    Ok(false)
}

fn default_install_paths(game_folder_name: &str) -> Vec<PathBuf> {
    if !cfg!(windows) {
        return vec![];
    }

    let names = crate::classes::games::markers::known_folder_names(game_folder_name);

    let mut paths: Vec<PathBuf> = names
        .iter()
        .map(|name| PathBuf::from(r"C:\Program Files").join(name))
        .collect();

    for root in get_root_paths() {
        if root.as_os_str() != OsStr::new(r"C:\") {
            for name in &names {
                paths.push(root.join("Program Files").join(name));
            }
        }
    }
    paths
}

const EXCLUDED_FOLDERS: &[&str] = if cfg!(windows) {
    &[
        "Windows",
        "AppData",
        "ProgramData",
        "$Recycle.Bin",
        "System Volume Information",
    ]
} else {
    &[
        "proc",
        "sys",
        "dev",
        "run",
        "bin",
        "sbin",
        "lib",
        "lib64",
        "usr",
        "boot",
        "tmp",
        "var",
        "etc",
        "mnt",
        "media",
        "lost+found",
    ]
};

#[cfg(not(target_os = "windows"))]
fn compatdata_candidate(game_folder_name: &str) -> Option<PathBuf> {
    use crate::classes::steam::compatdata_prefixes;

    let prefixes = compatdata_prefixes();
    if prefixes.is_empty() {
        return None;
    }

    debug!(
        "Searching {} Proton prefix(es) for the game",
        prefixes.len()
    );

    prefixes.into_par_iter().find_map_any(|prefix| {
        WalkDir::new(&prefix)
            .follow_links(false)
            .skip_hidden(true)
            .process_read_dir(|_, _, (), dir_entry_results| {
                dir_entry_results.retain(|dir_entry_result| {
                    dir_entry_result
                        .as_ref()
                        .is_ok_and(|dir_entry| dir_entry.file_type.is_dir())
                });
            })
            .into_iter()
            .find_map(|dir_entry_result| {
                let entry = dir_entry_result.ok()?;
                if !folder_name_matches(&entry.file_name().to_string_lossy(), game_folder_name) {
                    return None;
                }

                let path = entry.path();
                trace!("Prefix {} contains {}", prefix.display(), path.display());
                validate_game_path(&path, game_folder_name)
                    .unwrap_or(false)
                    .then_some(path)
            })
    })
}

#[cfg(target_os = "windows")]
const fn compatdata_candidate(_game_folder_name: &str) -> Option<PathBuf> {
    None
}

pub fn candidate_directories() -> Result<Option<PathBuf>, std::io::Error> {
    let game_folder_name = selected_game_folder_name();

    if let Some(candidate) = compatdata_candidate(&game_folder_name) {
        info!(
            "Found game directory inside a Proton prefix at {}",
            candidate.display()
        );
        return Ok(Some(candidate));
    }

    for candidate in default_install_paths(&game_folder_name) {
        trace!("Probing default install path {}", candidate.display());
        if candidate.is_dir() && validate_game_path(&candidate, &game_folder_name).unwrap_or(false)
        {
            info!(
                "Found game directory via default install path {}",
                candidate.display()
            );
            return Ok(Some(candidate));
        }
    }

    let roots = get_root_paths();

    let result = roots.into_par_iter().find_map_any(|root| {
        WalkDir::new(root)
            .follow_links(false)
            .skip_hidden(true)
            .process_read_dir(|_, _, (), dir_entry_results| {
                dir_entry_results.retain(|dir_entry_result| {
                    if let Ok(dir_entry) = dir_entry_result {
                        if !dir_entry.file_type.is_dir() {
                            return false;
                        }

                        let name = dir_entry.file_name.to_string_lossy();
                        !EXCLUDED_FOLDERS.contains(&name.as_ref())
                    } else {
                        true
                    }
                });
            })
            .into_iter()
            .find_map(|dir_entry_result| {
                let entry = dir_entry_result.ok()?;
                if entry.file_type().is_dir()
                    && folder_name_matches(&entry.file_name().to_string_lossy(), &game_folder_name)
                {
                    Some(entry.path())
                } else {
                    None
                }
            })
    });

    Ok(result)
}

fn get_root_paths() -> Vec<PathBuf> {
    if cfg!(windows) {
        (b'A'..=b'Z')
            .filter_map(|b| {
                let path = PathBuf::from(format!("{}:\\", b as char));
                path.exists().then_some(path)
            })
            .collect()
    } else {
        vec![PathBuf::from("/")]
    }
}

pub fn get_game_directory() -> Result<PathBuf> {
    let game_folder_name = selected_game_folder_name();

    let path: PathBuf = get(key::GAME_PATH)
        .as_str()
        .ok_or_else(|| anyhow!("Game directory not found"))?
        .into();
    let path = normalize_game_root(path);
    if validate_game_path(&path, &game_folder_name)? {
        set(key::GAME_PATH, path.display().to_string());
        return Ok(path);
    }

    warn!("Game directory {} not valid", path.display());

    let instant = Instant::now();
    if let Some(candidate) = candidate_directories()? {
        trace!("Trying {}", candidate.display());
        if validate_game_path(&candidate, &game_folder_name)? {
            info!("Found game directory {}", candidate.display());
            let elapsed = instant.elapsed();
            info!("Candidate search took {elapsed:?}");
            set(key::GAME_PATH, candidate.display().to_string());

            return Ok(candidate);
        }
    }
    
    Err(anyhow!("Game directory not found"))
}