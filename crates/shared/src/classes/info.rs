//! Constants, functions and structs related to NTE's files, versions, and directories.

use std::{
    fmt,
    path::{Path, PathBuf},
};

pub const GAME_FOLDER_NAME: &str = "Neverness To Everness";

pub const CLIENT_WIN64: &str = "Client/WindowsNoEditor/HT/Binaries/Win64";
pub const CLIENT_PAK_DIR: &str = "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods";

pub const NTE_PROCESSES: &[&str] = &[
    // GL
    "ntegloballauncher.exe",
    "nteglobal.exe",
    "nteglobalgame.exe",
    // CN
    "ntelauncher.exe",
    "ntegame.exe",
    // TW
    "ntetwlauncher.exe",
    "ntetwgame.exe",
    // ALL
    "htgame.exe",
];

pub const LAUNCHER_MAP: &[(&str, Version)] = &[
    ("NTEGlobalLauncher.exe", Version::Global),
    ("NTELauncher.exe", Version::CN),
    ("NTETWLauncher.exe", Version::TW),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    Global,
    CN,
    TW,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Version::Global => "global",
            Version::CN => "cn",
            Version::TW => "tw",
        };
        write!(f, "{}", s)
    }
}

impl Version {
    pub fn spec(&self) -> VersionSpec {
        match self {
            Version::Global => VersionSpec {
                launcher_subfolder: "NTEGlobal",
                launcher_process: "NTEGlobalLauncher.exe",
                helper_processes: &["NTEGlobal.exe", "NTEGlobalGame.exe"],
            },
            Version::CN => VersionSpec {
                launcher_subfolder: "NTELauncher",
                launcher_process: "NTELauncher.exe",
                helper_processes: &["NTEGame.exe"],
            },
            Version::TW => VersionSpec {
                launcher_subfolder: "NTETW",
                launcher_process: "NTETWLauncher.exe",
                helper_processes: &["NTETWGame.exe"],
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BypassMethod {
    Version,
    DSound,
    DDraw,
}

impl BypassMethod {
    pub fn to_dll_names(&self, version: Version) -> Vec<&'static str> {
        match self {
            BypassMethod::Version => vec!["version.dll"],
            BypassMethod::DSound => {
                if version == Version::CN {
                    vec!["dsound.dll", "ddraw.dll"]
                } else {
                    vec!["dsound.dll"]
                }
            }
            BypassMethod::DDraw => vec!["ddraw.dll"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionSpec {
    pub launcher_subfolder: &'static str,
    pub launcher_process: &'static str,
    pub helper_processes: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub struct DllSlot {
    pub name: String,
    pub root: PathBuf,
    pub bin: PathBuf,
    pub launcher: Option<PathBuf>,
}

impl DllSlot {
    pub fn all_targets(&self) -> Vec<(&'static str, PathBuf)> {
        let mut targets = vec![("root", self.root.clone()), ("bin", self.bin.clone())];
        if let Some(launcher) = &self.launcher {
            targets.push(("launcher", launcher.clone()));
        }
        targets
    }
}

#[derive(Debug, Clone)]
pub struct VersionPaths {
    pub version: Version,
    pub win64: PathBuf,
    pub pak_base: PathBuf,
    pub dll_slots: Vec<DllSlot>,
    pub asi_plugin: PathBuf,
    pub launcher_process: &'static str,
    pub helper_processes: Vec<&'static str>,
    pub game_process: &'static str,
}

impl VersionPaths {
    pub fn all_dll_targets(&self) -> Vec<(String, PathBuf)> {
        self.dll_slots
            .iter()
            .flat_map(|slot| {
                slot.all_targets()
                    .into_iter()
                    .map(move |(label, path)| (format!("{}:{}", slot.name, label), path))
            })
            .collect()
    }
}

pub fn get_version_paths(
    game_path: &Path,
    version: Version,
    engine_method: BypassMethod,
) -> VersionPaths {
    let spec = version.spec();

    let win64 = game_path.join(CLIENT_WIN64);
    let pak_base = game_path.join(CLIENT_PAK_DIR);
    let launcher_dir = game_path.join(spec.launcher_subfolder);

    let dll_names = engine_method.to_dll_names(version);

    let dll_slots: Vec<DllSlot> = dll_names
        .into_iter()
        .map(|dll_name| DllSlot {
            name: dll_name.to_string(),
            root: game_path.join(dll_name),
            bin: win64.join(dll_name),
            launcher: Some(launcher_dir.join(dll_name)),
        })
        .collect();

    VersionPaths {
        version,
        win64: win64.clone(),
        pak_base,
        dll_slots,
        asi_plugin: win64.join("ausigbp.asi"),
        launcher_process: spec.launcher_process,
        helper_processes: spec.helper_processes.to_vec(),
        game_process: "HTGame.exe",
    }
}

pub fn detect_version(game_path: &Path) -> Result<Version, String> {
    if !game_path.exists() {
        return Err(format!(
            "Aurora couldn't find the game path: {}",
            game_path.display()
        ));
    }

    for (launcher_exe, version) in LAUNCHER_MAP {
        if game_path.join(launcher_exe).exists() {
            return Ok(*version);
        }
    }

    let checked: Vec<&str> = LAUNCHER_MAP.iter().map(|(exe, _)| *exe).collect();
    Err(format!(
        "Could not detect NTE version in '{}'. None of the expected launchers were found: {:?}",
        game_path.display(),
        checked
    ))
}
