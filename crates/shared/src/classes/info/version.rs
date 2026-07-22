use std::fmt;
use std::path::Path;

use anyhow::{anyhow, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Version {
    Global,
    CN,
    TW,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Global => "global",
            Self::CN => "cn",
            Self::TW => "tw",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionSpec {
    pub launcher_process: &'static str,
    pub helper_processes: &'static [&'static str],
}

impl Version {
    pub const fn spec(&self) -> VersionSpec {
        match self {
            Self::Global => VersionSpec {
                launcher_process: "NTEGlobalLauncher.exe",
                helper_processes: &["NTEGlobal.exe", "NTEGlobalGame.exe"],
            },
            Self::CN => VersionSpec {
                launcher_process: "NTELauncher.exe",
                helper_processes: &["NTEGame.exe"],
            },
            Self::TW => VersionSpec {
                launcher_process: "NTETWLauncher.exe",
                helper_processes: &["NTETWGame.exe"],
            },
        }
    }
}

pub const LAUNCHER_MAP: &[(&str, Version)] = &[
    ("NTEGlobalLauncher.exe", Version::Global),
    ("NTELauncher.exe", Version::CN),
    ("NTETWLauncher.exe", Version::TW),
];

pub fn detect_version(game_path: &Path) -> Result<Version> {
    if !game_path.exists() {
        return Err(anyhow!(
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
    Err(anyhow!(
        "Could not detect NTE version in '{}'. None of the expected launchers were found: {:?}",
        game_path.display(),
        checked
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BypassMethod {
    Version,
    DSound,
    DDraw,
}

impl fmt::Display for BypassMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Version => "version.dll",
            Self::DSound => "dsound.dll",
            Self::DDraw => "ddraw.dll",
        };
        write!(f, "{s}")
    }
}

impl BypassMethod {
    pub fn to_dll_names(&self, version: Version) -> Vec<&'static str> {
        match self {
            Self::Version => vec!["version.dll"],
            Self::DSound => {
                if version == Version::CN {
                    vec!["dsound.dll", "ddraw.dll"]
                } else {
                    vec!["dsound.dll"]
                }
            }
            Self::DDraw => vec!["ddraw.dll"],
        }
    }

    pub fn from_num(i: impl Into<i64>, version: Version) -> Result<Self> {
        let i = i.into();
        match i {
            0 => Ok(Self::Version),
            1 => {
                if version == Version::CN {
                    Ok(Self::DSound)
                } else {
                    Ok(Self::DDraw)
                }
            }
            _ => Err(anyhow!("Invalid bypass method: {i}")),
        }
    }
}
