use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use log::{debug, info, warn};

use crate::classes::launch_options::{self, LaunchOptions};
use shared::classes::steam::real_user;
use shared::classes::steam::{
    STEAM_APP_ID, aurora_compat_data_dir, find_steam_root, steam_libraries,
};

const DLL_OVERRIDES: [&str; 3] = ["version", "dsound", "dwmapi"];

pub fn launch_via_proton(exe: &Path, game_args: &[&str]) -> Result<std::process::Child> {
    debug!("launch_via_proton: exe={}", exe.display());

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

    let opts = proton_launch_options();

    let mut command_line: Vec<OsString> = Vec::new();
    command_line.extend(opts.wrapper.iter().map(OsString::from));
    command_line.push(proton_bin.into_os_string());
    command_line.push(OsString::from("waitforexitandrun"));
    command_line.push(exe.as_os_str().to_os_string());
    command_line.extend(game_args.iter().map(OsString::from));
    command_line.extend(opts.trailing_args.iter().map(OsString::from));

    let mut command_line = command_line.into_iter();
    let program = PathBuf::from(
        command_line
            .next()
            .expect("the Proton path is always present"),
    );

    let mut cmd = command_as_real_user(&program);
    cmd.args(command_line);
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }

    let child = cmd
        .current_dir(work_dir)
        .env("PWD", work_dir)
        .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", &steam_root)
        .env("STEAM_COMPAT_DATA_PATH", &compat_data)
        .env("SteamAppId", STEAM_APP_ID)
        .env("SteamGameId", STEAM_APP_ID)
        .spawn()
        .with_context(|| {
            format!(
                "failed to spawn {} for {}",
                program.display(),
                exe.display()
            )
        })?;

    Ok(child)
}

fn proton_launch_options() -> LaunchOptions {
    let raw = shared::config::get(shared::config::key::PROTON_ARGS);
    let raw = raw.as_str().unwrap_or("").trim().to_string();

    if raw.is_empty() {
        return LaunchOptions::default();
    }

    let opts = launch_options::parse(&raw);

    info!(
        "Proton launch options: env={:?} wrapper={:?} args={:?} (%command% {})",
        opts.env,
        opts.wrapper,
        opts.trailing_args,
        if opts.has_command {
            "present"
        } else {
            "absent, treating the remaining tokens as game arguments"
        }
    );

    opts
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

/// Creates the compat data directory Aurora owns and hands it to the invoking
/// user. The path itself is defined by [`aurora_compat_data_dir`].
///
/// Aurora runs as root, but Proton is dropped back to the invoking user, so
/// anything we create here has to be handed over to them or Proton won't be
/// able to write into it.
fn aurora_compat_data() -> Result<PathBuf> {
    let dir = aurora_compat_data_dir()
        .ok_or_else(|| anyhow!("could not determine the user's home directory"))?;

    std::fs::create_dir_all(&dir)
        .with_context(|| format!("could not create compat data directory {}", dir.display()))?;

    let compatdata_dir = dir
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", dir.display()))?;
    let aurora_dir = compatdata_dir
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", compatdata_dir.display()))?;

    for created in [aurora_dir, compatdata_dir, dir.as_path()] {
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

struct DwProtonBuild {
    rank: (u8, bool, Vec<u64>),
    name: String,
    script: PathBuf,
}

impl DwProtonBuild {
    const fn new(rank: (u8, bool, Vec<u64>), name: String, script: PathBuf) -> Self {
        Self { rank, name, script }
    }
}

fn dwproton_builds(steam_root: &Path) -> Vec<DwProtonBuild> {
    let tools_dir = steam_root.join("compatibilitytools.d");
    let Ok(entries) = std::fs::read_dir(&tools_dir) else {
        debug!("could not read {}", tools_dir.display());
        return Vec::new();
    };

    let mut builds = Vec::new();

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

        builds.push(DwProtonBuild::new(
            (priority, is_latest, vkey),
            name,
            script,
        ));
    }

    builds.sort_by(|a, b| b.rank.cmp(&a.rank));

    builds
}

pub fn installed_dwproton_builds() -> Vec<String> {
    let Some(steam_root) = find_steam_root() else {
        warn!("could not determine the Steam client install directory; no DW-Proton builds listed");
        return Vec::new();
    };

    dwproton_builds(&steam_root)
        .into_iter()
        .map(|build| build.name)
        .collect()
}

pub fn resolve_proton_dirs(picked: &Path) -> Vec<PathBuf> {
    if is_proton_dir(picked) {
        return vec![picked.to_path_buf()];
    }

    let Ok(entries) = std::fs::read_dir(picked) else {
        warn!("could not read {}", picked.display());
        return Vec::new();
    };

    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| is_proton_dir(path))
        .collect();

    dirs.sort();
    dirs
}

fn is_proton_dir(dir: &Path) -> bool {
    dir.join("proton").is_file()
}

fn saved_custom_paths() -> Vec<PathBuf> {
    let configured = shared::config::get(shared::config::key::PROTON_CUSTOM_PATHS);
    let Some(paths) = configured.as_array() else {
        return Vec::new();
    };

    paths
        .iter()
        .filter_map(|path| path.as_str())
        .map(PathBuf::from)
        .collect()
}

fn save_custom_paths(paths: &[PathBuf]) {
    let paths: Vec<String> = paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();

    shared::config::set(shared::config::key::PROTON_CUSTOM_PATHS, paths);
}

pub fn custom_proton_builds() -> Vec<PathBuf> {
    saved_custom_paths()
        .into_iter()
        .filter(|dir| {
            let valid = is_proton_dir(dir);
            if !valid {
                debug!("custom Proton installation {} is gone", dir.display());
            }
            valid
        })
        .collect()
}

pub fn add_custom_proton_builds(dirs: &[PathBuf]) {
    let mut paths = saved_custom_paths();

    for dir in dirs {
        if paths.contains(dir) {
            debug!("{} was already added", dir.display());
        } else {
            paths.push(dir.clone());
        }
    }

    save_custom_paths(&paths);
}

pub fn remove_custom_proton_build(dir: &Path) {
    let mut paths = saved_custom_paths();
    paths.retain(|path| path != dir);
    save_custom_paths(&paths);
}

fn selected_custom_proton() -> Option<PathBuf> {
    let configured = shared::config::get(shared::config::key::PROTON_CUSTOM_PATH);
    let configured = configured.as_str().unwrap_or("").trim().to_string();

    if configured.is_empty() {
        return None;
    }

    let dir = PathBuf::from(configured);
    if !is_proton_dir(&dir) {
        warn!(
            "the selected Proton installation {} no longer exists; \
             falling back to automatic selection",
            dir.display()
        );
        return None;
    }

    Some(dir)
}

pub fn is_proton_version_not_recommended(version: &str) -> bool {
    debug!("is_proton_version_not_recommended: version={version:?}");
    !version.contains("10.") && !version.is_empty()
}

fn find_dwproton_script(steam_root: &Path) -> Option<PathBuf> {
    if let Some(dir) = selected_custom_proton() {
        info!(
            "Using the manually selected Proton installation at {}",
            dir.display()
        );
        return Some(dir.join("proton"));
    }

    let builds = dwproton_builds(steam_root);

    let configured = shared::config::get(shared::config::key::PROTON_VERSION);
    let configured = configured.as_str().unwrap_or("").trim().to_string();

    if !configured.is_empty() {
        match builds.iter().find(|build| build.name == configured) {
            Some(build) => {
                info!(
                    "Using the configured DW-Proton build {} at {}",
                    build.name,
                    build.script.display()
                );
                return Some(build.script.clone());
            }
            None => warn!(
                "The configured DW-Proton build {configured:?} is not installed any more; \
                 falling back to automatic selection"
            ),
        }
    }

    let build = builds.into_iter().next()?;

    info!(
        "Selected DW-Proton build {} at {}",
        build.name,
        build.script.display()
    );

    Some(build.script)
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
