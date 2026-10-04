use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::{debug, warn};

use crate::classes::games::SteamMetadata;

pub struct RealUser {
    pub uid: u32,
    pub gid: u32,
    pub name: String,
    pub home: PathBuf,
}

pub fn real_user() -> Option<RealUser> {
    use std::ffi::{CStr, CString};
    use std::os::unix::ffi::OsStrExt;

    let Some(name) = std::env::var_os("SUDO_USER") else {
        debug!("SUDO_USER not set; using the process environment as-is");
        return Some(RealUser {
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            name: std::env::var("USER").unwrap_or_default(),
            home: std::env::var_os("HOME").map(PathBuf::from)?,
        });
    };

    let c_name = CString::new(name.as_bytes()).ok()?;
    let pw = unsafe { libc::getpwnam(c_name.as_ptr()) };
    if pw.is_null() {
        warn!(
            "SUDO_USER={:?} is not in the password database",
            name.to_string_lossy()
        );
        return None;
    }

    let pw = unsafe { &*pw };
    let home = unsafe { CStr::from_ptr(pw.pw_dir) };
    let home = PathBuf::from(std::ffi::OsStr::from_bytes(home.to_bytes()));

    debug!(
        "Running under sudo; resolved invoking user {:?} (uid {}) with home {}",
        name.to_string_lossy(),
        pw.pw_uid,
        home.display()
    );

    Some(RealUser {
        uid: pw.pw_uid,
        gid: pw.pw_gid,
        name: name.to_string_lossy().into_owned(),
        home,
    })
}

pub fn real_home() -> Option<PathBuf> {
    real_user().map(|user| user.home)
}

pub fn compat_data_dir(steam: &SteamMetadata) -> Option<PathBuf> {
    real_home().map(|home| compat_data_under(&home, steam))
}

pub fn compat_prefix(steam: &SteamMetadata) -> Option<PathBuf> {
    compat_data_dir(steam).map(|dir| dir.join("pfx"))
}

fn compat_data_under(home: &Path, steam: &SteamMetadata) -> PathBuf {
    home.join(".local/share/Aurora")
        .join("compatdata")
        .join(steam.app_id)
}

pub fn steam_libraries() -> Vec<PathBuf> {
    let mut libraries = Vec::new();

    let Some(home) = real_home() else {
        warn!("could not determine the user's home directory; cannot locate Steam libraries");
        return libraries;
    };

    let default_roots = [
        home.join(".steam/root"),
        home.join(".steam/steam"),
        home.join(".local/share/Steam"),
        home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        home.join("snap/steam/common/.local/share/Steam"),
    ];

    for root in &default_roots {
        if root.is_dir() {
            libraries.push(root.clone());
        }
    }

    for root in &default_roots {
        let vdf_path = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(extra) = parse_library_folders(&vdf_path) {
            libraries.extend(extra);
        }
    }

    libraries.sort();
    libraries.dedup();
    libraries
}

pub fn find_steam_root() -> Option<PathBuf> {
    let home = real_home()?;

    for candidate in [
        home.join(".steam/steam"),
        home.join(".steam/root"),
        home.join(".local/share/Steam"),
    ] {
        if let Ok(resolved) = candidate.canonicalize()
            && (resolved.join("steamapps").is_dir() || resolved.join("ubuntu12_32").is_dir())
        {
            return Some(resolved);
        }
    }

    None
}

pub fn parse_library_folders(vdf_path: &Path) -> Result<Vec<PathBuf>> {
    let text = std::fs::read_to_string(vdf_path)
        .with_context(|| format!("could not read {}", vdf_path.display()))?;

    let mut paths = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"path\"") {
            let rest = rest.trim();
            if let Some(value) = rest.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                let unescaped = value.replace("\\\\", "\\");
                paths.push(PathBuf::from(unescaped));
            }
        }
    }
    Ok(paths)
}

pub fn compatdata_prefixes(libraries: &[PathBuf], steam: Option<&SteamMetadata>) -> Vec<PathBuf> {
    let mut prefixes = Vec::new();

    for library in libraries {
        let compatdata = library.join("steamapps").join("compatdata");
        let Ok(entries) = std::fs::read_dir(&compatdata) else {
            continue;
        };

        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let path = entry.path();
            prefixes.push(path.canonicalize().unwrap_or(path));
        }
    }

    prefixes.sort();
    prefixes.dedup();
    if let Some(steam) = steam {
        prefixes
            .sort_by_key(|prefix| prefix.file_name() != Some(std::ffi::OsStr::new(steam.app_id)));
    }
    prefixes
}