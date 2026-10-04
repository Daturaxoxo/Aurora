use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, anyhow};
use jwalk::WalkDir;
use log::*;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};

use crate::classes::games::{Game, InstallationFacts};

const MAX_ANCESTOR_HOPS: usize = 5; // maximum number of ancestor jumps done when searching for the install root
// ^^ done so if someone selects "C:\Program Files\Neverness To Everness\Client\WindowsNoEditor\HT\Binaries", it'll resolve to "C:\Program Files\Neverness To Everness"
const LIBRARY_FOLDERS: &[&str] = &[
    "common",
    "Games",
    "SteamLibrary",
    "Program Files",
    "Program Files (x86)",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provenance {
    Configured,
    Ancestor,
    Child,
    SteamLibrary,
    KnownLocation,
    CompatData,
    MarkerScan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationCandidate {
    root: PathBuf,
    provenance: Vec<Provenance>,
}

impl InstallationCandidate {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn provenance(&self) -> &[Provenance] {
        &self.provenance
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiscoveryInputs {
    pub steam_libraries: Vec<PathBuf>,
    pub install_bases: Vec<PathBuf>,
    pub compat_prefixes: Vec<PathBuf>,
    pub scan_roots: Vec<PathBuf>,
}

fn is_library_folder(name: &str) -> bool {
    LIBRARY_FOLDERS.iter().any(|f| f.eq_ignore_ascii_case(name))
}

fn folder_name_matches(game: &dyn Game, candidate: &str) -> bool {
    game.descriptor()
        .aliases
        .iter()
        .any(|alias| alias.eq_ignore_ascii_case(candidate))
}

fn has_launcher(game: &dyn Game, path: &Path) -> bool {
    game.descriptor()
        .launchers
        .iter()
        .any(|launcher| launcher.file.resolve_under(path).exists())
}

pub fn validate_game_path(game: &dyn Game, path: &Path) -> bool {
    if !path.is_dir() {return false}

    let d = game.descriptor();
    if let Some(launcher) = d
        .launchers
        .iter()
        .find(|launcher| launcher.file.resolve_under(path).exists())
    {
        trace!(
            "Validated {} via launcher marker {}",
            path.display(),
            launcher.file
        );
        return true;
    }

    if d.binaries.resolve_under(path).is_dir() {
        trace!("Validated {} via client tree", path.display());
        return true;
    }

    if let Some(marker) = d
        .markers
        .iter()
        .find(|marker| marker.resolve_under(path).exists())
    {
        trace!("Validated {} via game marker {marker}", path.display());
        return true;
    }

    trace!(
        "Validation failed for {}: no launcher or marker match",
        path.display()
    );
    false
}

pub fn resolve_game_root(game: &dyn Game, path: &Path) -> Option<(PathBuf, Provenance)> {
    if path.as_os_str().is_empty() {return None}
    if validate_game_path(game, path) {return Some((path.to_path_buf(), Provenance::Configured))}

    for ancestor in path.ancestors().skip(1).take(MAX_ANCESTOR_HOPS) {
        if validate_game_path(game, ancestor) {
            info!(
                "{} points inside the install; using its root {}",
                path.display(),
                ancestor.display()
            );
            return Some((ancestor.to_path_buf(), Provenance::Ancestor));
        }
    }

    let mut children: Vec<PathBuf> = fs::read_dir(path)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    children.sort();

    for child in children {
        let name = child.file_name().unwrap_or_default().to_string_lossy();
        if (folder_name_matches(game, &name) || has_launcher(game, &child))
            && validate_game_path(game, &child)
        {
            info!(
                "{} is the parent of the install; using {}",
                path.display(),
                child.display()
            );
            return Some((child, Provenance::Child));
        }
    }

    None
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

fn steam_candidates(game: &dyn Game, libraries: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();

    for library in libraries {
        let common = library.join("steamapps").join("common");
        let Ok(entries) = fs::read_dir(&common) else {
            continue;
        };

        trace!("Scanning Steam library {}", common.display());

        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .collect();
        dirs.sort();

        for path in dirs {
            let name = path.file_name().unwrap_or_default().to_string_lossy();

            if folder_name_matches(game, &name) {
                if validate_game_path(game, &path) {
                    found.push(path);
                }
            } else if has_launcher(game, &path) {
                info!(
                    "Steam library entry {} was renamed but carries a launcher",
                    path.display()
                );
                found.push(path);
            }
        }
    }

    found
}

fn known_location_candidates(game: &dyn Game, bases: &[PathBuf]) -> Vec<PathBuf> {
    bases
        .iter()
        .flat_map(|base| {
            game.descriptor()
                .aliases
                .iter()
                .map(move |name| base.join(name))
        })
        .filter(|candidate| {
            trace!("Probing default install path {}", candidate.display());
            validate_game_path(game, candidate)
        })
        .collect()
}

fn compat_data_candidates(game: &dyn Game, prefixes: &[PathBuf]) -> Vec<PathBuf> {
    if prefixes.is_empty() {
        return Vec::new();
    }

    debug!(
        "Searching {} Proton prefix(es) for the game",
        prefixes.len()
    );

    let per_prefix: Vec<Vec<PathBuf>> = prefixes
        .to_vec()
        .into_par_iter()
        .map(|prefix| {
            let mut found: Vec<PathBuf> = WalkDir::new(&prefix)
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
                .filter_map(|dir_entry_result| {
                    let entry = dir_entry_result.ok()?;
                    if !folder_name_matches(game, &entry.file_name().to_string_lossy()) {return None}

                    let path = entry.path();
                    trace!("Prefix {} contains {}", prefix.display(), path.display());
                    validate_game_path(game, &path).then_some(path)
                })
                .collect();
            found.sort();
            found
        })
        .collect();

    per_prefix.into_iter().flatten().collect()
}

fn scan_candidate(game: &dyn Game, roots: &[PathBuf]) -> Option<PathBuf> {
    roots.to_vec().into_par_iter().find_map_any(|root| {
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
                let matches_name = folder_name_matches(game, &name);
                let in_library = entry
                    .parent_path()
                    .file_name()
                    .is_some_and(|parent| is_library_folder(&parent.to_string_lossy()));

                if !matches_name && !in_library {return None}

                let path = entry.path();
                let valid = if matches_name {
                    validate_game_path(game, &path)
                } else {
                    has_launcher(game, &path)
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
    })
}

fn same_location(a: &Path, b: &Path) -> bool {
    if a == b {return true}
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn push_candidate(
    candidates: &mut Vec<InstallationCandidate>,
    root: PathBuf,
    provenance: Provenance,
) {
    if let Some(existing) = candidates
        .iter_mut()
        .find(|candidate| same_location(&candidate.root, &root))
    {
        if !existing.provenance.contains(&provenance) {
            existing.provenance.push(provenance);
        }
        return;
    }

    candidates.push(InstallationCandidate {
        root,
        provenance: vec![provenance],
    });
}

fn check_absolute(field: &str, paths: &[PathBuf]) -> Result<()> {
    paths
        .iter()
        .find(|path| !path.is_absolute())
        .map_or(Ok(()), |path| {
            Err(anyhow!(
                "discovery input {field} has a relative path {}",
                path.display()
            ))
        })
}

pub fn discover(game: &dyn Game, inputs: &DiscoveryInputs) -> Result<Vec<InstallationCandidate>> {
    check_absolute("steam_libraries", &inputs.steam_libraries)?;
    check_absolute("install_bases", &inputs.install_bases)?;
    check_absolute("compat_prefixes", &inputs.compat_prefixes)?;
    check_absolute("scan_roots", &inputs.scan_roots)?;

    let mut candidates = Vec::new();
    for root in steam_candidates(game, &inputs.steam_libraries) {
        push_candidate(&mut candidates, root, Provenance::SteamLibrary);
    }
    for root in known_location_candidates(game, &inputs.install_bases) {
        push_candidate(&mut candidates, root, Provenance::KnownLocation);
    }
    for root in compat_data_candidates(game, &inputs.compat_prefixes) {
        push_candidate(&mut candidates, root, Provenance::CompatData);
    }

    if candidates.is_empty() && !inputs.scan_roots.is_empty() {
        info!("Falling back to a full filesystem scan");
        if let Some(root) = scan_candidate(game, &inputs.scan_roots) {
            push_candidate(&mut candidates, root, Provenance::MarkerScan);
        }
    }

    Ok(candidates)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedRoot {
    pub root: PathBuf,
    pub provenance: Vec<Provenance>,
}

pub fn select_configured(game: &dyn Game, path: &Path) -> Option<SelectedRoot> {
    resolve_game_root(game, path).map(|(root, provenance)| SelectedRoot {
        root,
        provenance: vec![provenance],
    })
}

pub fn select_root(
    game: &dyn Game,
    configured: Option<&Path>,
    candidates: &[InstallationCandidate],
) -> Result<Option<SelectedRoot>> {
    if let Some(path) = configured.filter(|path| !path.as_os_str().is_empty()) {
        return select_configured(game, path)
            .map(Some)
            .ok_or_else(|| anyhow!("{} is not a game installation", path.display()));
    }

    Ok(candidates
        .iter()
        .find(|candidate| validate_game_path(game, &candidate.root))
        .map(|candidate| SelectedRoot {
            root: candidate.root.clone(),
            provenance: candidate.provenance.clone(),
        }))
}

pub fn resolve_installation(
    game: &dyn Game,
    configured: Option<&Path>,
    candidates: &[InstallationCandidate],
) -> Result<Option<InstallationFacts>> {
    let Some(selected) = select_root(game, configured, candidates)? else {
        return Ok(None);
    };

    game.inspect_installation(&selected.root)
        .map(Some)
        .with_context(|| format!("could not inspect {}", selected.root.display()))
}

impl DiscoveryInputs {
    pub fn observe_host(game: &dyn Game) -> Self {
        let steam_libraries = steam_libraries();

        #[cfg(target_os = "linux")]
        let compat_prefixes = crate::classes::steam::compatdata_prefixes(
            &steam_libraries,
            game.descriptor().steam.as_ref(),
        );
        #[cfg(not(target_os = "linux"))]
        let compat_prefixes = {
            let _ = game;
            Vec::new()
        };

        Self {
            install_bases: install_bases(),
            steam_libraries,
            compat_prefixes,
            scan_roots: get_root_paths(),
        }
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

fn install_bases() -> Vec<PathBuf> {
    if cfg!(windows) {
        let mut bases = vec![PathBuf::from(r"C:\Program Files")];
        bases.extend(
            get_root_paths()
                .into_iter()
                .filter(|root| root.as_os_str() != std::ffi::OsStr::new(r"C:\"))
                .map(|root| root.join("Program Files")),
        );
        return bases;
    }

    let Some(home) = home_dir() else {
        return Vec::new();
    };

    vec![
        home.clone(),
        home.join("Games"),
        home.join("Games/Heroic"),
        home.join(".wine/drive_c/Program Files"),
        home.join(".wine/drive_c/Program Files (x86)"),
        home.join(".wine/drive_c/Games"),
        PathBuf::from("/opt"),
    ]
}

#[cfg(target_os = "linux")]
fn steam_libraries() -> Vec<PathBuf> {
    crate::classes::steam::steam_libraries()
}

#[cfg(windows)]
fn registry_string(hive: isize, subkey: &str, value: &str) -> Option<PathBuf> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
    use windows_sys::Win32::System::Registry::RegGetValueW;

    const RRF_RT_REG_SZ: u32 = 0x0000_0002;
    const ERROR_SUCCESS: u32 = 0;

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let subkey_w = wide(subkey);
    let value_w = wide(value);
    let mut size: u32 = 0;

    let status = unsafe {
        RegGetValueW(
            hive as *mut _,
            subkey_w.as_ptr(),
            value_w.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut size,
        )
    };

    if status != ERROR_SUCCESS || size == 0 {
        trace!("Registry value {subkey}\\{value} is unavailable (status {status})");
        return None;
    }

    let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
    let mut written = size;

    let status = unsafe {
        RegGetValueW(
            hive as *mut _,
            subkey_w.as_ptr(),
            value_w.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut written,
        )
    };

    if status != ERROR_SUCCESS {
        warn!("Could not read registry value {subkey}\\{value} (status {status})");
        return None;
    }

    let chars = (written as usize / 2).min(buffer.len());
    let text: Vec<u16> = buffer[..chars]
        .iter()
        .copied()
        .take_while(|unit| *unit != 0)
        .collect();

    if text.is_empty() {
        return None;
    }

    Some(PathBuf::from(std::ffi::OsString::from_wide(&text)))
}

#[cfg(windows)]
fn parse_library_folders(vdf_path: &Path) -> Vec<PathBuf> {
    let Ok(text) = fs::read_to_string(vdf_path) else {
        return Vec::new();
    };

    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("\"path\"")?;
            let value = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
            Some(PathBuf::from(value.replace("\\\\", "\\")))
        })
        .collect()
}

#[cfg(windows)]
fn steam_libraries() -> Vec<PathBuf> {
    const HKEY_CURRENT_USER: isize = -2_147_483_647;
    const HKEY_LOCAL_MACHINE: isize = -2_147_483_646;

    let mut roots: Vec<PathBuf> = [
        (HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
    ]
    .into_iter()
    .filter_map(|(hive, subkey, value)| registry_string(hive, subkey, value))
    .collect();

    roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));

    let mut libraries = Vec::new();
    for root in roots {
        let steamapps = root.join("steamapps");
        if !steamapps.is_dir() {
            continue;
        }

        trace!("Found a Steam install at {}", root.display());
        libraries.push(root);
        libraries.extend(parse_library_folders(&steamapps.join("libraryfolders.vdf")));
    }

    libraries.sort();
    libraries.dedup();
    libraries
}

#[cfg(not(any(target_os = "linux", windows)))]
const fn steam_libraries() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(windows)]
fn suppress_error_dialogs() {
    use std::sync::Once;
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    static ONCE: Once = Once::new();

    ONCE.call_once(|| unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode;

        let previous = SetErrorMode(SEM_FAILCRITICALERRORS);
        SetErrorMode(previous | SEM_FAILCRITICALERRORS);
    });
}

fn get_root_paths() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        suppress_error_dialogs();

        (b'A'..=b'Z')
            .filter_map(|b| {
                let path = PathBuf::from(format!("{}:\\", b as char));
                if !path.exists() {
                    return None;
                }

                if fs::read_dir(&path).is_err() {
                    debug!("Skipping drive {} because it is not ready", path.display());
                    return None;
                }

                Some(path)
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![PathBuf::from("/")]
    }
}