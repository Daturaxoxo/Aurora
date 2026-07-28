use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};

const STEAM_APP_ID: &str = "4508340";
const DLL_OVERRIDES: [&str; 2] = ["version", "dsound"];

#[cfg(target_os = "linux")]
pub fn ensure_dll_overrides() {
    match apply_overrides() {
        Ok(()) => info!(
            "Applied WINEDLLOVERRIDES for {:?} to Proton prefix",
            DLL_OVERRIDES
        ),
        Err(e) => warn!(
            "Could not automatically configure Wine DLL overrides ({e:#}); \
             mods will not load this is set manually via `winecfg` \
             or `protontricks {STEAM_APP_ID} winecfg`"
        ),
    }
}

#[cfg(not(target_os = "linux"))]
pub fn ensure_dll_overrides() {}

#[cfg(target_os = "linux")]
pub fn launch_via_proton(exe: &Path) -> Result<std::process::Child> {
    let prefix = find_proton_prefix(STEAM_APP_ID)
        .ok_or_else(|| anyhow!("could not locate NTE's Proton prefix in any Steam library"))?;

    debug!("Found NTE Proton prefix at {}", prefix.display());

    // compatdata/<appid> is the parent of .../pfx
    let compat_data = prefix
        .parent()
        .ok_or_else(|| anyhow!("prefix {} has no parent directory", prefix.display()))?;

    let steam_root = find_steam_root()
        .ok_or_else(|| anyhow!("could not determine Steam client install directory"))?;

    debug!("Using Steam root at {}", steam_root.display());

    let proton_bin = find_proton_script(&prefix)
        .ok_or_else(|| anyhow!("could not locate a `proton` script for this prefix"))?;

    info!(
        "Launching {} via Proton at {}",
        exe.display(),
        proton_bin.display()
    );

    let child = Command::new(&proton_bin)
        .arg("waitforexitandrun")
        .arg(exe)
        .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", &steam_root)
        .env("STEAM_COMPAT_DATA_PATH", compat_data)
        .spawn()
        .with_context(|| format!("failed to spawn proton for {}", exe.display()))?;

    Ok(child)
}

#[cfg(not(target_os = "linux"))]
pub fn launch_via_proton(_exe: &Path) -> Result<std::process::Child> {
    Err(anyhow!("launch_via_proton is only supported on Linux"))
}

fn apply_overrides() -> Result<()> {
    let prefix = find_proton_prefix(STEAM_APP_ID)
        .ok_or_else(|| anyhow!("could not locate NTE's Proton prefix in any Steam library"))?;

    debug!("Found NTE Proton prefix at {}", prefix.display());

    let wine = find_wine_binary(&prefix)
        .ok_or_else(|| anyhow!("could not locate a wine binary (checked Proton dirs and PATH)"))?;

    debug!("Using wine binary at {}", wine.display());

    for dll in DLL_OVERRIDES {
        set_override(&wine, &prefix, dll)
            .with_context(|| format!("failed to set override for {dll}.dll"))?;
    }

    Ok(())
}

fn set_override(wine: &Path, prefix: &Path, dll_name: &str) -> Result<()> {
    let status = Command::new(wine)
        .env("WINEPREFIX", prefix)
        .env("WINEDEBUG", "-all")
        .args([
            "reg",
            "add",
            r"HKEY_CURRENT_USER\Software\Wine\DllOverrides",
            "/v",
            dll_name,
            "/d",
            "native,builtin",
            "/f",
        ])
        .status()
        .with_context(|| format!("failed to spawn `wine reg add` for {dll_name}"))?;

    if !status.success() {
        return Err(anyhow!("`wine reg add` exited with {status}"));
    }

    Ok(())
}

fn find_proton_prefix(app_id: &str) -> Option<PathBuf> {
    for library in steam_libraries() {
        let candidate = library
            .join("steamapps")
            .join("compatdata")
            .join(app_id)
            .join("pfx");
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

fn steam_libraries() -> Vec<PathBuf> {
    let mut libraries = Vec::new();

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        warn!("HOME environment variable not set; cannot locate Steam libraries");
        return libraries;
    };

    let default_roots = [home.join(".steam/steam"), home.join(".local/share/Steam")];

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

    libraries.dedup();
    libraries
}

/// Locates the root Steam client install directory (i.e. what Steam sets
/// `STEAM_COMPAT_CLIENT_INSTALL_PATH` to when it launches a game). This is
/// distinct from `steam_libraries()`, which also returns *additional*
/// library folders that may live on other drives/mounts — the client
/// install path must be the actual Steam installation, not a library.
fn find_steam_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;

    for candidate in [
        home.join(".steam/steam"),
        home.join(".steam/root"),
        home.join(".local/share/Steam"),
    ] {
        // Resolve symlinks (common on Arch, where ~/.steam/steam is a
        // symlink into ~/.local/share/Steam) and confirm it actually
        // points somewhere that looks like a Steam install.
        if let Ok(resolved) = candidate.canonicalize() {
            if resolved.join("steamapps").is_dir() || resolved.join("ubuntu12_32").is_dir() {
                return Some(resolved);
            }
        }
    }

    None
}

fn parse_library_folders(vdf_path: &Path) -> Result<Vec<PathBuf>> {
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

fn find_wine_binary(prefix: &Path) -> Option<PathBuf> {
    // walk prefix -> compatdata/<id> -> steamapps so we can check that same library's steamapps/common for a proton install
    let library_root = prefix
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent);

    if let Some(steamapps) = library_root {
        let common = steamapps.join("common");
        if let Ok(entries) = std::fs::read_dir(&common) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with("Proton") {
                    continue;
                }

                for candidate in [
                    path.join("files").join("bin").join("wine"),
                    path.join("dist").join("bin").join("wine"),
                ] {
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    which_wine()
}

/// Locates the top-level `proton` launcher script (not the `wine` binary
/// buried inside it) for whichever Proton build owns `prefix`. This is
/// what needs to be invoked with `waitforexitandrun` so Steam-style env
/// vars and prefix setup are honored the same way Steam itself would do it.
fn find_proton_script(prefix: &Path) -> Option<PathBuf> {
    let library_root = prefix
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)?;

    let common = library_root.join("common");
    let entries = std::fs::read_dir(&common).ok()?;

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("Proton") {
            continue;
        }

        let script = path.join("proton");
        if script.is_file() {
            return Some(script);
        }
    }

    None
}

fn which_wine() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join("wine"))
        .find(|candidate| candidate.is_file())
}
