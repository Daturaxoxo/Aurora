use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};

const STEAM_APP_ID: &str = "4508340";
const DLL_OVERRIDES: [&str; 3] = ["version", "dsound", "dwmapi"];

#[cfg(target_os = "linux")]
pub fn launch_via_proton(exe: &Path) -> Result<std::process::Child> {
    debug!("launch_via_proton: exe={:?}", exe);

    let steam_root = find_steam_root()
        .ok_or_else(|| anyhow!("could not determine Steam client install directory"))?;

    debug!("Using Steam root at {}", steam_root.display());

    let proton_bin = find_dwproton_script(&steam_root).ok_or_else(|| {
        anyhow!(
            "DW-Proton is not installed (looked for a DW-Proton build in {})",
            steam_root.join("compatibilitytools.d").display()
        )
    })?;

    let compat_data = aurora_compat_data()?;

    if is_in_steam_library(exe) {
        debug!("launch_via_proton: exe lives inside a Steam library");
    } else {
        debug!("launch_via_proton: exe is outside any Steam library");
    }

    // Steam runs a game from its own install directory, and protonfixes reads
    // `PWD` to work out which library the game belongs to.
    let work_dir = exe
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", exe.display()))?;

    // The prefix has to exist before we can write registry keys into it, and
    // on a fresh Aurora compat data directory it won't until Proton has run
    // once. Bootstrap it explicitly rather than racing the game's own startup.
    if !compat_data.join("pfx").is_dir() {
        info!(
            "Prefix at {} does not exist yet; bootstrapping it via Proton",
            compat_data.join("pfx").display()
        );
        if let Err(e) = bootstrap_prefix(&proton_bin, &steam_root, &compat_data, work_dir) {
            warn!("Could not bootstrap the Proton prefix ({e:#})");
        }
    }

    ensure_dll_overrides(&proton_bin, &compat_data.join("pfx"));

    info!(
        "Launching {} via Proton at {} (compat data {})",
        exe.display(),
        proton_bin.display(),
        compat_data.display()
    );

    let child = command_as_real_user(&proton_bin)
        .arg("waitforexitandrun")
        .arg(exe)
        .current_dir(work_dir)
        .env("PWD", work_dir)
        .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", &steam_root)
        .env("STEAM_COMPAT_DATA_PATH", &compat_data)
        .env("SteamAppId", STEAM_APP_ID)
        .env("SteamGameId", STEAM_APP_ID)
        .spawn()
        .with_context(|| format!("failed to spawn proton for {}", exe.display()))?;

    Ok(child)
}

#[cfg(not(target_os = "linux"))]
pub fn launch_via_proton(_exe: &Path) -> Result<std::process::Child> {
    Err(anyhow!("launch_via_proton is only supported on Linux"))
}

/// Runs Proton once with no real work to do, purely so it creates and
/// initializes `<compat_data>/pfx`. Blocks until it finishes.
fn bootstrap_prefix(
    proton_bin: &Path,
    steam_root: &Path,
    compat_data: &Path,
    work_dir: &Path,
) -> Result<()> {
    let status = command_as_real_user(proton_bin)
        .args(["run", "wineboot"])
        .current_dir(work_dir)
        .env("PWD", work_dir)
        .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_root)
        .env("STEAM_COMPAT_DATA_PATH", compat_data)
        .env("SteamAppId", STEAM_APP_ID)
        .env("SteamGameId", STEAM_APP_ID)
        .env("WINEDEBUG", "-all")
        .status()
        .with_context(|| format!("failed to spawn {} run wineboot", proton_bin.display()))?;

    if !status.success() {
        return Err(anyhow!("`proton run wineboot` exited with {status}"));
    }

    Ok(())
}

/// Applies the DLL overrides mods need to `prefix`, using the wine binary
/// belonging to the same Proton build that owns it.
fn ensure_dll_overrides(proton_bin: &Path, prefix: &Path) {
    match apply_overrides(proton_bin, prefix) {
        Ok(()) => info!(
            "Applied WINEDLLOVERRIDES for {:?} to prefix {}",
            DLL_OVERRIDES,
            prefix.display()
        ),
        Err(e) => warn!(
            "Could not automatically configure Wine DLL overrides ({e:#}); \
             mods will not load unless this is set manually via `winecfg` \
             or `protontricks {STEAM_APP_ID} winecfg`"
        ),
    }
}

fn apply_overrides(proton_bin: &Path, prefix: &Path) -> Result<()> {
    if !prefix.is_dir() {
        return Err(anyhow!("prefix {} does not exist", prefix.display()));
    }

    let wine = find_wine_binary(proton_bin)
        .ok_or_else(|| anyhow!("could not locate a wine binary (checked Proton dirs and PATH)"))?;

    debug!("Using wine binary at {}", wine.display());

    for dll in DLL_OVERRIDES {
        set_override(&wine, prefix, dll)
            .with_context(|| format!("failed to set override for {dll}.dll"))?;
    }

    Ok(())
}

fn set_override(wine: &Path, prefix: &Path, dll_name: &str) -> Result<()> {
    let status = command_as_real_user(wine)
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

/// Whether `exe` lives inside one of the Steam library folders we know about.
/// Both sides are canonicalized first so symlinked library roots (very common
/// on Arch, where `~/.steam/steam` points into `~/.local/share/Steam`) still
/// compare equal.
fn is_in_steam_library(exe: &Path) -> bool {
    let Ok(exe) = exe.canonicalize() else {
        warn!(
            "could not canonicalize {}; treating it as outside Steam",
            exe.display()
        );
        return false;
    };

    steam_libraries()
        .iter()
        .filter_map(|library| library.canonicalize().ok())
        .any(|library| exe.starts_with(&library))
}

/// The compat data directory Aurora owns.
/// Proton creates and populates `pfx/` inside it on first launch.
///
/// The app id leaf mirrors Steam's own `compatdata/<appid>` layout. It isn't
/// cosmetic: protonfixes recovers the game id by pulling the last run of digits
/// out of `STEAM_COMPAT_DATA_PATH`, so a path with no digits in it makes it
/// fail outright.
///
/// Aurora runs as root, but Proton is dropped back to the invoking user, so
/// anything we create here has to be handed over to them or Proton won't be
/// able to write into it.
fn aurora_compat_data() -> Result<PathBuf> {
    let home =
        real_home().ok_or_else(|| anyhow!("could not determine the user's home directory"))?;

    let aurora_dir = home.join(".local/share/Aurora");
    let compatdata_dir = aurora_dir.join("compatdata");
    let dir = compatdata_dir.join(STEAM_APP_ID);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create compat data directory {}", dir.display()))?;

    for created in [&aurora_dir, &compatdata_dir, &dir] {
        if let Err(e) = chown_to_real_user(created) {
            warn!(
                "could not chown {} to the invoking user: {e:#}",
                created.display()
            );
        }
    }

    Ok(dir)
}

/// Identity of the user who actually invoked Aurora. Aurora is always run with
/// `sudo`, so the process environment describes root (`HOME=/root`) — but
/// Steam, its libraries and its Proton prefixes all live in the invoking
/// user's home, and Proton must run as them rather than as root.
struct RealUser {
    uid: u32,
    gid: u32,
    name: String,
    home: PathBuf,
}

/// Resolves the invoking user from `$SUDO_USER` via the password database, so
/// non-standard home directories are picked up correctly. Falls back to the
/// process environment when we aren't running under sudo.
#[cfg(target_os = "linux")]
fn real_user() -> Option<RealUser> {
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

    // getpwnam returns a pointer into static storage, so everything we need
    // gets copied out before the next libc call can clobber it.
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

#[cfg(not(target_os = "linux"))]
fn real_user() -> Option<RealUser> {
    None
}

fn real_home() -> Option<PathBuf> {
    real_user().map(|user| user.home)
}

/// Builds a `Command` that runs as the invoking user rather than as root.
/// Proton writes a great deal into the prefix and into Steam's own
/// directories; doing that as root would leave files the user can no longer
/// touch, and would break launching the game through Steam normally.
#[cfg(target_os = "linux")]
fn command_as_real_user(program: &Path) -> Command {
    use std::os::unix::process::CommandExt;

    let mut cmd = Command::new(program);

    let Some(user) = real_user() else {
        return cmd;
    };

    if unsafe { libc::geteuid() } != 0 || user.uid == 0 {
        return cmd;
    }

    // std applies gid before uid, and clears supplementary groups for us.
    cmd.uid(user.uid).gid(user.gid);
    cmd.env("HOME", &user.home);
    cmd.env("USER", &user.name);
    cmd.env("LOGNAME", &user.name);

    // Proton and wine both want a writable runtime dir; the one inherited
    // from sudo belongs to root.
    let runtime_dir = PathBuf::from(format!("/run/user/{}", user.uid));
    if runtime_dir.is_dir() {
        cmd.env("XDG_RUNTIME_DIR", &runtime_dir);
    }

    debug!(
        "Dropping privileges to uid {} for {}",
        user.uid,
        program.display()
    );

    cmd
}

#[cfg(not(target_os = "linux"))]
fn command_as_real_user(program: &Path) -> Command {
    Command::new(program)
}

#[cfg(target_os = "linux")]
fn chown_to_real_user(path: &Path) -> Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let Some(user) = real_user() else {
        return Ok(());
    };

    let c_path = CString::new(path.as_os_str().as_bytes())
        .with_context(|| format!("{} contains an interior NUL", path.display()))?;

    if unsafe { libc::chown(c_path.as_ptr(), user.uid, user.gid) } != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("chown {}", path.display()));
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn chown_to_real_user(_path: &Path) -> Result<()> {
    Ok(())
}

/// Locates the newest DW-Proton build installed under Steam's
/// `compatibilitytools.d`. The priority for choosing the version is:
/// 10.* > 11.* > Any other
fn find_dwproton_script(steam_root: &Path) -> Option<PathBuf> {
    let tools_dir = steam_root.join("compatibilitytools.d");
    let entries = std::fs::read_dir(&tools_dir).ok()?;

    let mut candidates = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        let lower = name.to_lowercase();
        if !lower.contains("dw-proton") && !lower.contains("dwproton") {
            continue;
        }

        let script = entry.path().join("proton");
        if !script.is_file() {
            debug!("skipping DW-Proton candidate {name}: no `proton` script inside");
            continue;
        }

        let is_latest = lower.contains("latest");
        let vkey = version_key(&name);

        let priority = match vkey.first() {
            Some(&10) => 2,
            Some(&11) => 1,
            _ => 0,
        };

        candidates.push(((priority, is_latest, vkey), name, script));
    }

    let (_, name, script) = candidates.into_iter().max_by_key(|c| c.0.clone())?;

    info!("Selected DW-Proton build {name} at {}", script.display());

    Some(script)
}

/// Numeric components of a build name, in order, so `dwproton-10.2` sorts
/// above `dwproton-9.11` (a plain string compare would get that backwards).
fn version_key(name: &str) -> Vec<u64> {
    let mut parts = Vec::new();
    let mut current = String::new();

    for ch in name.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            parts.extend(current.parse::<u64>().ok());
            current.clear();
        }
    }
    if !current.is_empty() {
        parts.extend(current.parse::<u64>().ok());
    }

    parts
}

fn steam_libraries() -> Vec<PathBuf> {
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

/// Locates the root Steam client install directory (i.e. what Steam sets
/// `STEAM_COMPAT_CLIENT_INSTALL_PATH` to when it launches a game). This is
/// distinct from `steam_libraries()`, which also returns *additional*
/// library folders that may live on other drives/mounts — the client
/// install path must be the actual Steam installation, not a library.
fn find_steam_root() -> Option<PathBuf> {
    let home = real_home()?;

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

/// The `wine` binary shipped inside the Proton build whose launcher script is
/// `proton_bin`. Using that build's own wine (rather than whatever is on PATH)
/// keeps the registry writes consistent with the prefix it created.
fn find_wine_binary(proton_bin: &Path) -> Option<PathBuf> {
    if let Some(build_dir) = proton_bin.parent() {
        for candidate in [
            build_dir.join("files").join("bin").join("wine"),
            build_dir.join("dist").join("bin").join("wine"),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    which_wine()
}

fn which_wine() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join("wine"))
        .find(|candidate| candidate.is_file())
}
