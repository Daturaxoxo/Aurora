use std::{fs, path::PathBuf};

use scandir::Walk;

use log::*;

use crate::{
    classes::info::{GAME_FOLDER_NAME, LAUNCHER_MAP},
    config::{load_config, save_config, Config},
};

fn validate_game_path(path: &PathBuf) -> Result<bool, std::io::Error> {
    let path = fs::canonicalize(path);
    if path.is_err() {
        return Ok(false);
    }
    let path = path.unwrap();

    for launcher in LAUNCHER_MAP {
        let launcher_path = path.join(launcher.0);
        if launcher_path.exists() {
            return Ok(true);
        }
    }

    let game_found = path.read_dir()?.any(|entry| {
        let entry = entry.unwrap();
        entry.file_name().to_str().unwrap() == GAME_FOLDER_NAME
    });

    Ok(game_found)
}

fn candidate_directories() -> Vec<PathBuf> {
    const BLACKLISTED_DIRECTORIES: [&str; 5] = [
        "$RECYCLE.BIN",
        "System Volume Information",
        "Windows",
        "AppData",
        "ProgramData",
    ];
    let mut candidates = Vec::new();
    for drive_letter in "ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().map(|c| c.to_string()) {
        if fs::metadata(format!("{}:\\", drive_letter)).is_err() {
            continue;
        }

        let entries = Walk::new(format!("{}:\\", drive_letter), None)
            .unwrap()
            .dir_exclude(Some(
                BLACKLISTED_DIRECTORIES
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ))
            .follow_links(false)
            .collect()
            .unwrap();

        for dir in entries.dirs() {
            if dir.contains(GAME_FOLDER_NAME) {
                candidates.push(PathBuf::from(format!("{}:\\{}", drive_letter, dir)));
            }
        }
    }

    candidates
}

pub fn get_game_directory() -> Result<PathBuf, std::io::Error> {
    let path = load_config()?.game_path();
    if validate_game_path(&path)? {
        return Ok(PathBuf::from(&path));
    } else {
        warn!("Game directory {} not valid", path.display());
    }

    for candidate in candidate_directories() {
        if validate_game_path(&candidate)? {
            save_config(Config::new(candidate.clone()))?;
            return Ok(PathBuf::from(&candidate));
        }
    }

    error!("Game directory not found");

    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Game directory not found",
    ))
}
