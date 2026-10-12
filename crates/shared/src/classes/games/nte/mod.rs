pub mod addons;
mod characters;
mod gamebanana;
mod launcher;
mod lua;
mod mods;
pub mod patcher;
pub mod paths;
mod screenshots;
mod unreal;
pub mod version;

use std::{path::Path, sync::LazyLock};

use anyhow::Result;
use serde_json::Value;

use super::{
    EngineKind, Game, GameDescriptor, InstallationFacts, LauncherIdentifier, SteamMetadata,
    capabilities::{
        AddonSupport, CharacterSupport, GameBananaSupport, LaunchArgumentsSupport,
        LauncherSupport, LuaSupport, ModManagerSupport, ModuleSupport, ScreenshotSupport,
        UnrealSupport,
    },
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
const STEAM_APP_ID: &str = "4508340";
const FOLDER_NAMES: &[&str] = &[paths::GAME_FOLDER_NAME, "異環", "NTE"];
const PAYLOAD_FILES: &[&str] = &[
    "AuroraEngine.dll",
    "Everlight.asi",
    "Plugins/chksum.asi",
    "Wrappers/version.dll",
    "Wrappers/dsound.dll",
    "Addons/Censorship/censorship.auadd",
    "Addons/HideRedDots/hideRedDots.auadd",
    "Addons/HideUID/hideUID.auadd",
    "Addons/UIModPack/uiModPack.auadd",
];
const MARKERS: &[&str] = &[
    "NTELauncher.exe",
    "NTEGlobalLauncher.exe",
    "NTETWLauncher.exe",
    "Client/WindowsNoEditor/HT/Content/Paks",
];

pub const LEGACY_BIN_SIGNATURE: &[&str] = &["cutils.dll", "Everlight.asi"];
pub const LEGACY_BIN_FILES: &[&str] = &["AuroraEngine.dll", "Everlight.asi", "cutils.dll"];
pub const LEGACY_BIN_DIRS: &[&str] = &["Plugins", "Wrappers", "Addons", "Lua"];

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
                game_executable: Some(path(NTE_GAME_EXE)),
                binaries: Some(path(paths::CLIENT_WIN64)),
                payload_dir: path("nte"),
                payload_files: PAYLOAD_FILES.iter().copied().map(path).collect(),
                steam: Some(SteamMetadata {
                    app_id: STEAM_APP_ID,
                }),
                engine: Some(EngineKind::Pak),
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
        config::default_value(key.as_str())
    }

    fn launcher(&self) -> Option<&dyn LauncherSupport> {
        Some(self)
    }

    fn addons(&self) -> Option<&dyn AddonSupport> {
        Some(self)
    }

    fn mod_manager(&self) -> Option<&dyn ModManagerSupport> {
        Some(self)
    }

    fn modules(&self) -> Option<&dyn ModuleSupport> {
        Some(self)
    }

    fn lua(&self) -> Option<&dyn LuaSupport> {
        Some(self)
    }

    fn characters(&self) -> Option<&dyn CharacterSupport> {
        Some(self)
    }

    fn gamebanana(&self) -> Option<&dyn GameBananaSupport> {
        Some(self)
    }

    fn screenshots(&self) -> Option<&dyn ScreenshotSupport> {
        Some(self)
    }

    fn unreal(&self) -> Option<&dyn UnrealSupport> {
        Some(self)
    }

    fn launch_arguments(&self) -> Option<&dyn LaunchArgumentsSupport> {
        Some(self)
    }
}
