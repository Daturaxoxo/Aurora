use std::path::{Path, PathBuf};

use crate::classes::info::Target;

use super::version::{BypassMethod, Version};

pub const GAME_FOLDER_NAME: &str = "Neverness To Everness";

cfg_select! {
    windows => {
        pub const CLIENT_WIN64: &str = "Client\\WindowsNoEditor\\HT\\Binaries\\Win64";
        pub const CLIENT_PAK_DIR: &str = "Client\\WindowsNoEditor\\HT\\Content\\Paks\\AuroraMods";
    }
    unix => {
        pub const CLIENT_WIN64: &str = "Client/WindowsNoEditor/HT/Binaries/Win64";
        pub const CLIENT_PAK_DIR: &str = "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods";
    }
}

#[derive(Debug, Clone)]
pub struct DllSlot {
    pub name: String,
    pub root: PathBuf,
    pub bin: PathBuf,
}

impl DllSlot {
    pub fn all_targets(&self) -> Vec<(&'static str, PathBuf)> {
        vec![("root", self.root.clone()), ("bin", self.bin.clone())]
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

    pub fn pak_dir(&self) -> Option<&Path> {
        self.pak_base.parent()
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

    let dll_names = engine_method.to_dll_names();

    let dll_slots: Vec<DllSlot> = dll_names
        .into_iter()
        .map(|dll_name| DllSlot {
            name: dll_name.to_string(),
            root: game_path.join(dll_name),
            bin: win64.join(dll_name),
        })
        .collect();

    VersionPaths {
        version,
        win64: win64.clone(),
        pak_base,
        dll_slots,
        asi_plugin: win64.join(Target::AsiPlugin.as_file()),
        launcher_process: spec.launcher_process,
        helper_processes: spec.helper_processes.to_vec(),
        game_process: "HTGame.exe",
    }
}
