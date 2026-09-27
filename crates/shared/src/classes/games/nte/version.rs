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
        write!(f, "{}", self.key())
    }
}

impl Distribution {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Epic => "epic",
            Self::Steam => "steam",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Standalone, Self::Epic, Self::Steam]
            .into_iter()
            .find(|distribution| distribution.key() == key)
    }

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
        write!(f, "{}", self.id())
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

    pub const fn id(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Manual => "manual",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::Direct, Self::Manual]
            .into_iter()
            .find(|method| method.id() == id)
    }

    pub fn decode(raw: &serde_json::Value) -> Self {
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
    pub const fn key(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::CN => "cn",
            Self::TW => "tw",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        [Self::Global, Self::CN, Self::TW]
            .into_iter()
            .find(|version| version.key() == key)
    }

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

pub(super) fn detect(game_path: &Path) -> Result<Version> {
    if !game_path.exists() {
        return Err(anyhow!(
            "The game path does not exist: {}",
            game_path.display()
        ));
    }

    for (launcher_exe, version) in LAUNCHER_MAP {
        if game_path.join(launcher_exe).exists() {
            return Ok(*version);
        }
    }

    Err(anyhow!(
        "{} isn't a recognized installation (checked: {:?})",
        game_path.display(),
        LAUNCHER_MAP
            .iter()
            .map(|(exe, _)| (*exe).to_string())
            .collect::<Vec<_>>(),
    ))
}

pub fn detect_version(game_path: &Path) -> Result<Version> {
    detect(game_path)
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
