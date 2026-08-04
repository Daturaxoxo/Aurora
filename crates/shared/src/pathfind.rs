use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use crate::classes::games::markers::{find_marker, folder_name_matches, known_folder_names};
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

const MAX_ANCESTOR_HOPS: usize = 8; // maximum number of ancestor jumps done when searching for Neverness to Everness
// ^^ done so if someone selects "C:\Program Files\Neverness To Everness\Client\WindowsNoEditor\HT\Binaries", it'll resolve to "C:\Program Files\Neverness To Everness"
const LIBRARY_FOLDERS: &[&str] = &[
    "common",
    "Games",
    "SteamLibrary",
    "Program Files",
    "Program Files (x86)",
];

fn selected_game_folder_name() -> String {
    let selected = get(key::SELECTED_GAME);
    match selected.as_str() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => GAME_FOLDER_NAME.to_string(),
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        crate::classes::steam::real_home().or_else(dirs::home_dir)
    }
    #[cfg(not(target_os = "linux"))]
    {
        dirs::home_dir()
    }
}

fn is_library_folder(name: &str) -> bool {
    LIBRARY_FOLDERS.iter().any(|f| f.eq_ignore_ascii_case(name))
}

fn has_launcher(path: &Path) -> bool {
    LAUNCHER_MAP
        .iter()
        .any(|(launcher, _)| path.join(launcher).exists())
}

pub fn validate_game_path(path: &Path, game_folder_name: &str) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }

    for (launcher, _) in LAUNCHER_MAP {
        if path.join(launcher).exists() {
            trace!(
                "Validated {} via launcher marker {launcher}",
                path.display()
            );
            return Ok(true);
        }
    }

    if path.join(CLIENT_WIN64).is_dir() {
        trace!("Validated {} via client tree", path.display());
        return Ok(true);
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

pub fn resolve_game_root(path: &Path, game_folder_name: &str) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }

    if validate_game_path(path, game_folder_name).unwrap_or(false) {
        return Some(path.to_path_buf());
    }

    for ancestor in path.ancestors().skip(1).take(MAX_ANCESTOR_HOPS) {
        if validate_game_path(ancestor, game_folder_name).unwrap_or(false) {
            info!(
                "{} points inside the install; using its root {}",
                path.display(),
                ancestor.display()
            );
            return Some(ancestor.to_path_buf());
        }
    }

    let children = fs::read_dir(path).ok()?;
    for entry in children.flatten() {
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name = name.to_string_lossy();
        let matches_name = folder_name_matches(&name, game_folder_name);

        if (matches_name || has_launcher(&child))
            && validate_game_path(&child, game_folder_name).unwrap_or(false)
        {
            info!(
                "{} is the parent of the install; using {}",
                path.display(),
                child.display()
            );
            return Some(child);
        }
    }

    None
}

pub fn resolve_selected_game_root(path: &Path) -> Option<PathBuf> {
    resolve_game_root(path, &selected_game_folder_name())
}

fn default_install_paths(game_folder_name: &str) -> Vec<PathBuf> {
    let names = known_folder_names(game_folder_name);
    let mut paths: Vec<PathBuf> = vec![];

    if cfg!(windows) {
        paths.extend(
            names
                .iter()
                .map(|name| PathBuf::from(r"C:\Program Files").join(name)),
        );

        for root in get_root_paths() {
            if root.as_os_str() != OsStr::new(r"C:\") {
                for name in &names {
                    paths.push(root.join("Program Files").join(name));
                }
            }
        }

        return paths;
    }

    let Some(home) = home_dir() else {
        return paths;
    };

    let bases = [
        home.clone(),
        home.join("Games"),
        home.join("Games/Heroic"),
        home.join(".wine/drive_c/Program Files"),
        home.join(".wine/drive_c/Program Files (x86)"),
        home.join(".wine/drive_c/Games"),
        PathBuf::from("/opt"),
    ];

    for base in bases {
        for name in &names {
            paths.push(base.join(name));
        }
    }

    paths
}

#[cfg(windows)]
fn should_descend(_parent: &Path, name: &str) -> bool {
    const EXCLUDED: &[&str] = &[
        "Windows",
        "AppData",
        "ProgramData",
        "$Recycle.Bin",
        "System Volume Information",
    ];

    !EXCLUDED.iter().any(|e| e.eq_ignore_ascii_case(name))
}

#[cfg(not(windows))]
fn should_descend(parent: &Path, name: &str) -> bool {
    const PRUNED_ROOTS: &[&str] = &[
        "proc",
        "sys",
        "dev",
        "boot",
        "etc",
        "tmp",
        "usr",
        "bin",
        "sbin",
        "lib",
        "lib32",
        "lib64",
        "libx32",
        "var",
        "lost+found",
        "snap",
    ];

    if parent == Path::new("/") {
        return !PRUNED_ROOTS.contains(&name);
    }

    if parent == Path::new("/run") {
        return matches!(name, "media" | "mount" | "mnt");
    }

    true
}

#[cfg(target_os = "linux")]
fn steam_candidate(game_folder_name: &str) -> Option<PathBuf> {
    use crate::classes::steam::steam_libraries;

    let libraries = steam_libraries();
    if libraries.is_empty() {
        debug!("No Steam libraries found");
        return None;
    }

    debug!("Checking {} Steam library folder(s)", libraries.len());

    for library in libraries {
        let common = library.join("steamapps").join("common");
        let Ok(entries) = fs::read_dir(&common) else {
            continue;
        };

        trace!("Scanning Steam library {}", common.display());

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let name = entry.file_name();
            let name = name.to_string_lossy();

            if folder_name_matches(&name, game_folder_name) {
                if validate_game_path(&path, game_folder_name).unwrap_or(false) {
                    return Some(path);
                }
            } else if has_launcher(&path) {
                info!(
                    "Steam library entry {} was renamed but carries a launcher",
                    path.display()
                );
                return Some(path);
            }
        }
    }

    None
}

#[cfg(not(target_os = "linux"))]
const fn steam_candidate(_game_folder_name: &str) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
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

#[cfg(not(target_os = "linux"))]
const fn compatdata_candidate(_game_folder_name: &str) -> Option<PathBuf> {
    None
}

pub fn candidate_directories() -> Result<Option<PathBuf>, std::io::Error> {
    let game_folder_name = selected_game_folder_name();

    if let Some(candidate) = steam_candidate(&game_folder_name) {
        info!(
            "Found game directory in a Steam library at {}",
            candidate.display()
        );
        return Ok(Some(candidate));
    }

    for candidate in default_install_paths(&game_folder_name) {
        trace!("Probing default install path {}", candidate.display());
        if validate_game_path(&candidate, &game_folder_name).unwrap_or(false) {
            info!(
                "Found game directory via default install path {}",
                candidate.display()
            );
            return Ok(Some(candidate));
        }
    }

    if let Some(candidate) = compatdata_candidate(&game_folder_name) {
        info!(
            "Found game directory inside a Proton prefix at {}",
            candidate.display()
        );
        return Ok(Some(candidate));
    }

    info!("Falling back to a full filesystem scan");

    let roots = get_root_paths();

    let result = roots.into_par_iter().find_map_any(|root| {
        WalkDir::new(root)
            .follow_links(false)
            .skip_hidden(true)
            .process_read_dir(|_, parent, (), dir_entry_results| {
                dir_entry_results.retain(|dir_entry_result| {
                    if let Ok(dir_entry) = dir_entry_result {
                        if !dir_entry.file_type.is_dir() {
                            return false;
                        }

                        should_descend(parent, &dir_entry.file_name.to_string_lossy())
                    } else {
                        true
                    }
                });
            })
            .into_iter()
            .find_map(|dir_entry_result| {
                let entry = dir_entry_result.ok()?;
                if !entry.file_type().is_dir() {
                    return None;
                }

                let name = entry.file_name().to_string_lossy().into_owned();
                let matches_name = folder_name_matches(&name, &game_folder_name);
                let in_library = entry
                    .parent_path()
                    .file_name()
                    .is_some_and(|parent| is_library_folder(&parent.to_string_lossy()));

                if !matches_name && !in_library {
                    return None;
                }

                let path = entry.path();
                let valid = if matches_name {
                    validate_game_path(&path, &game_folder_name).unwrap_or(false)
                } else {
                    has_launcher(&path)
                };

                if !valid {
                    if matches_name {
                        debug!(
                            "{} matches the game folder name but holds no game files; \
                             continuing the search",
                            path.display()
                        );
                    }
                    return None;
                }

                Some(path)
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

    let stored = get(key::GAME_PATH).as_str().unwrap_or_default().to_string();

    if stored.is_empty() {
        debug!("No game directory stored yet; searching for one");
    } else {
        let stored = PathBuf::from(stored);
        if let Some(root) = resolve_game_root(&stored, &game_folder_name) {
            if root != stored {
                info!(
                    "Normalized stored game directory {} to {}",
                    stored.display(),
                    root.display()
                );
            }
            set(key::GAME_PATH, root.display().to_string());
            return Ok(root);
        }

        warn!(
            "Stored game directory {} is not a valid install; searching for one",
            stored.display()
        );
    }

    let instant = Instant::now();
    if let Some(candidate) = candidate_directories()? {
        info!(
            "Found game directory {} in {:?}",
            candidate.display(),
            instant.elapsed()
        );
        set(key::GAME_PATH, candidate.display().to_string());

        return Ok(candidate);
    }

    warn!("No game directory found after {:?}", instant.elapsed());

    Err(anyhow!("Game directory not found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aurora-pathfind-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolves_a_root_from_inside_the_install() {
        let root = scratch("inside").join("Neverness To Everness");
        let inner = root.join(CLIENT_WIN64);
        fs::create_dir_all(&inner).unwrap();
        fs::write(root.join("NTEGlobalLauncher.exe"), []).unwrap();

        assert_eq!(
            resolve_game_root(&inner, GAME_FOLDER_NAME),
            Some(root.clone())
        );
        assert_eq!(
            resolve_game_root(root.parent().unwrap(), GAME_FOLDER_NAME),
            Some(root)
        );
    }

    #[test]
    fn a_folder_named_like_the_game_is_not_enough() {
        let decoy = scratch("decoy").join("nte");
        fs::create_dir_all(&decoy).unwrap();

        assert!(!validate_game_path(&decoy, GAME_FOLDER_NAME).unwrap());
        assert_eq!(resolve_game_root(&decoy, GAME_FOLDER_NAME), None);
    }

    #[test]
    fn empty_paths_resolve_to_nothing() {
        assert_eq!(resolve_game_root(Path::new(""), GAME_FOLDER_NAME), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn external_mounts_stay_walkable() {
        assert!(should_descend(Path::new("/"), "run"));
        assert!(should_descend(Path::new("/"), "media"));
        assert!(should_descend(Path::new("/"), "mnt"));
        assert!(should_descend(Path::new("/run"), "media"));
        assert!(!should_descend(Path::new("/"), "proc"));
        assert!(!should_descend(Path::new("/run"), "user"));
        assert!(should_descend(Path::new("/home/user"), "media"));
    }
}
