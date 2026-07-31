use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use log::{debug, info, warn};
use shared::classes::steam::real_user;
use shared::classes::steam::{find_steam_root, real_home, steam_libraries};

const STEAM_APP_ID: &str = "4508340";
const DLL_OVERRIDES: [&str; 3] = ["version", "dsound", "dwmapi"];

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

/// Builds a `Command` that runs as the invoking user rather than as root.
/// Proton writes a great deal into the prefix and into Steam's own
/// directories; doing that as root would leave files the user can no longer
/// touch, and would break launching the game through Steam normally.
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
