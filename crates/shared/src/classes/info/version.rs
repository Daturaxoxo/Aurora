use std::path::Path;
use std::{fmt, fs};

use anyhow::{Result, anyhow};
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
    Steam,
}

impl std::fmt::Display for Distribution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Standalone => "standalone",
            Self::Epic => "epic",
            Self::Steam => "steam",
        };
        write!(f, "{s}")
    }
}

impl Distribution {
    pub const fn launch_args(&self) -> &'static [&'static str] {
        match self {
            Self::Standalone | Self::Steam => &[],
            Self::Epic => &["-AUTH_PASSWORD=1234", "-AUTH_TYPE=exchangecode"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StartMethod {
    Direct,
    #[default]
    Manual,
}

impl fmt::Display for StartMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Direct => "direct",
            Self::Manual => "manual",
        };
        write!(f, "{s}")
    }
}

impl StartMethod {
    pub const fn launch_args(&self) -> &'static [&'static str] {
        match self {
            Self::Direct => &["/autoplay"],
            Self::Manual => &[],
        }
    }

    pub const fn from_num(i: i64) -> Self {
        match i {
            0 => Self::Direct,
            _ => Self::Manual,
        }
    }

    pub const fn as_index(self) -> i32 {
        match self {
            Self::Direct => 0,
            Self::Manual => 1,
        }
    }

    pub fn from_config() -> Self {
        let raw = crate::config::get(crate::config::key::START_METHOD);

        raw.as_i64()
            .or_else(|| raw.as_str().and_then(|s| s.parse::<i64>().ok()))
            .map_or_else(
                || {
                    debug!("Unreadable start_method {raw:?}, leaving the launcher on screen");
                    Self::default()
                },
                Self::from_num,
            )
    }
}

pub fn detect_distribution(game_path: &Path) -> Distribution {
    let mut distribution = Distribution::Standalone;
    if let Ok(entries) = fs::read_dir(game_path) {
        for entry in entries.filter_map(Result::ok) {
            if !entry.file_name().to_string_lossy().starts_with("NTE") {
                continue;
            }
            let path = entry.path();
            if path.join("EOSSDK-Win64-Shipping.dll").is_file() {
                return Distribution::Epic;
            }
            if path.join("steam_api64.dll").is_file() {
                distribution = Distribution::Steam;
            }
        }
    }
    distribution
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
                helper_processes: &["NTEGlobal.exe", "NTEGlobalGame.exe", "NTEGlobalBrowser.exe"],
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
