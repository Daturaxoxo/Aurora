use std::{path::PathBuf, time::Instant};

use anyhow::{anyhow, Result};
use jwalk::WalkDir;
use log::*;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};

use crate::{
    classes::info::{paths::GAME_FOLDER_NAME, version::LAUNCHER_MAP},
    config::{get, key, set},
};

#[allow(clippy::ptr_arg)]
fn validate_game_path(path: &PathBuf) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    for launcher in LAUNCHER_MAP {
        let launcher_path = path.join(launcher.0);
        if launcher_path.exists() {
            return Ok(true);
        }
    }

    let game_found = path.read_dir()?.any(|entry| match entry {
        Ok(e) => e.file_name().to_str().unwrap() == GAME_FOLDER_NAME,
        Err(_) => false,
    });

    Ok(game_found)
}

pub fn candidate_directories() -> Result<Option<PathBuf>, std::io::Error> {
    const EXCLUDED_FOLDERS: &[&str] = if cfg!(windows) {
        &[
            "Windows",
            "AppData",
            "ProgramData",
            "Program Files",
            "Program Files (x86)",
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
                if entry.file_type().is_dir() && entry.file_name() == GAME_FOLDER_NAME {
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
    let path = get(key::GAME_PATH)
        .as_str()
        .ok_or_else(|| anyhow!("Game directory not found"))?
        .into();
    if validate_game_path(&path)? {
        return Ok(path);
    }

    warn!("Game directory {} not valid", path.display());

    let instant = Instant::now();
    if let Some(candidate) = candidate_directories()? {
        trace!("Trying {}", candidate.display());
        if validate_game_path(&candidate)? {
            info!("Found game directory {}", candidate.display());
            let elapsed = instant.elapsed();
            info!("Candidate search took {elapsed:?}");
            set(key::GAME_PATH, candidate.display().to_string());

            return Ok(candidate);
        }
    }

    error!("Game directory not found");

    Err(anyhow!("Game directory not found"))
}
