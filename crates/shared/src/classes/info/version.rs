use std::fmt;
use std::path::Path;

use anyhow::{anyhow, Result};
use log::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Version {
    Global,
    CN,
    TW,
    #[default]
    Unknown,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Global => "global",
            Self::CN => "cn",
            Self::TW => "tw",
            Self::Unknown => "Unknown",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Distribution {
    #[default]
    Standalone,
    Epic,
}

impl std::fmt::Display for Distribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Standalone => "standalone",
            Self::Epic => "epic",
        };
        write!(f, "{s}")
    }
}

impl Distribution {
    pub const fn launch_args(&self) -> &'static [&'static str] {
        match self {
            Self::Standalone => &[],
            Self::Epic => &["-AUTH_PASSWORD=1234", "-AUTH_TYPE=exchangecode"],
        }
    }
}

pub fn detect_distribution(game_path: &Path) -> Distribution {
    if game_path
        .join("NTEGlobal")
        .join("EOSSDK-Win64-Shipping.dll")
        .is_file()
    {
        Distribution::Epic
    } else {
        Distribution::Standalone
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
            Self::Unknown => VersionSpec {
                launcher_process: "Unknown",
                helper_processes: &[],
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
}

impl fmt::Display for BypassMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Version => "version.dll",
            Self::DSound => "dsound.dll",
        };
        write!(f, "{s}")
    }
}

impl BypassMethod {
    pub const ALL_DLL_NAMES: &'static [&'static str] = &["version.dll", "dsound.dll"];

    pub fn to_dll_names(&self) -> Vec<&'static str> {
        match self {
            Self::Version => vec!["version.dll"],
            Self::DSound => vec!["dsound.dll"],
        }
    }

    pub fn resolve(raw: impl Into<i64>, version: Version) -> Result<Self> {
        let method = Self::from_num(raw.into())?;

        if version == Version::CN && method != Self::DSound {
            debug!("CN installation detected: forcing engine method {method} -> dsound.dll (ACE)");
            return Ok(Self::DSound);
        }

        Ok(method)
    }

    fn from_num(i: i64) -> Result<Self> {
        match i {
            0 => Ok(Self::Version),
            1 => Ok(Self::DSound),
            _ => Err(anyhow!("Invalid bypass method: {i}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cn_resolve() {
        for raw in 0..=1 {
            let method = BypassMethod::resolve(raw, Version::CN).unwrap();
            assert_eq!(method, BypassMethod::DSound, "CN index {raw}");
            assert!(!method.to_dll_names().contains(&"version.dll"));
        }
    }

    #[test]
    fn global_resolve() {
        for version in [Version::Global, Version::TW] {
            assert_eq!(
                BypassMethod::resolve(0, version).unwrap(),
                BypassMethod::Version
            );
            assert_eq!(
                BypassMethod::resolve(1, version).unwrap(),
                BypassMethod::DSound
            );
        }
    }

    #[test]
    fn reject_ofr_index() {
        assert!(BypassMethod::resolve(2, Version::Global).is_err());
        assert!(BypassMethod::resolve(-1, Version::CN).is_err());
    }

    // function below is kind of temporary, just added it so people on CN v2.0.0 who have the old version.dll files in their \Win64 directory can easily clean them
    // so they don't have to deal with any old installations messing their experience (perchappenchance) -datura
    #[test]
    fn sweep_previous() {
        for method in [BypassMethod::Version, BypassMethod::DSound] {
            for dll in method.to_dll_names() {
                assert!(
                    BypassMethod::ALL_DLL_NAMES.contains(&dll),
                    "{dll} would be stranded by sanitize"
                );
            }
        }
    }
}
