pub mod addons;
mod launcher;
pub mod patcher;
pub mod paths;
pub mod version;

use std::{path::Path, sync::LazyLock};

use anyhow::Result;
use serde_json::Value;

use super::{
    EngineKind, Game, GameDescriptor, InstallationFacts, LauncherIdentifier,
    capabilities::{AddonSupport, LauncherSupport},
    identity::{GameId, SafeRelativePath},
};
use crate::config::{self, ProfileKey};

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

pub const NTE_GAME_EXE: &str = "HTGame.exe";
const FOLDER_NAMES: &[&str] = &[paths::GAME_FOLDER_NAME, "異環", "NTE"];
const MARKERS: &[&str] = &[
    "NTELauncher.exe",
    "NTEGlobalLauncher.exe",
    "NTETWLauncher.exe",
    "Client/WindowsNoEditor/HT/Content/Paks",
];

pub static NTE: LazyLock<Nte> = LazyLock::new(Nte::new);

pub struct Nte {
    descriptor: GameDescriptor,
}

fn path(raw: &str) -> SafeRelativePath {
    SafeRelativePath::parse(raw).unwrap_or_else(|e| panic!("NTE path '{raw}' is invalid: {e}"))
}

impl Nte {
    fn new() -> Self {
        Self {
            descriptor: GameDescriptor {
                id: GameId::parse("nte").expect("NTE id is valid"),
                display_name: paths::GAME_FOLDER_NAME,
                aliases: FOLDER_NAMES,
                launchers: version::LAUNCHER_MAP
                    .iter()
                    .map(|(file, version)| LauncherIdentifier {
                        file: path(file),
                        variant: version.key(),
                    })
                    .collect(),
                markers: MARKERS.iter().copied().map(path).collect(),
                processes: NTE_PROCESSES,
                game_executable: path(NTE_GAME_EXE),
                binaries: path(paths::CLIENT_WIN64),
                payload_dir: path("nte"),
                engine: EngineKind::Pak,
            },
        }
    }
}

impl Game for Nte {
    fn descriptor(&self) -> &GameDescriptor {
        &self.descriptor
    }

    fn inspect_installation(&self, root: &Path) -> Result<InstallationFacts> {
        let version = version::detect(root)?;
        let distribution = version::detect_distribution(root);
        Ok(InstallationFacts::new(
            root.to_path_buf(),
            version.key(),
            distribution.key(),
        ))
    }

    fn profile_default(&self, key: ProfileKey) -> Value {
        // The legacy unscoped defaults are NTE's, since NTE was the only game.
        config::default_value(key.as_str())
    }

    fn launcher(&self) -> Option<&dyn LauncherSupport> {
        Some(self)
    }

    fn addons(&self) -> Option<&dyn AddonSupport> {
        Some(self)
    }
}
