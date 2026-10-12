use std::path::PathBuf;

use super::Nte;
use crate::classes::games::{
    capabilities::{IniSetting, LaunchArgumentsSupport, ScaleSetting, UnrealSupport},
    platform::PlatformPaths,
};

const PROJECT: &str = "HT";
#[cfg(target_os = "windows")]
const SAVED: &str = "Saved";
#[cfg(not(target_os = "windows"))]
const SAVED: &str = "Saved_Global";

const SCALE: ScaleSetting = ScaleSetting {
    ini: IniSetting {
        file: "Engine.ini",
        section: "[/Script/Engine.UserInterfaceSettings]",
        key: "ApplicationScale",
    },
    min: 0.5,
    max: 2.0,
};

impl UnrealSupport for Nte {
    fn config_locations(&self, platform: &PlatformPaths) -> Vec<PathBuf> {
        let Some(project) = platform
            .local_app_data
            .as_ref()
            .map(|dir| dir.join(PROJECT))
        else {
            log::debug!("ini: no local app data folder yet");
            return Vec::new();
        };
        let Ok(entries) = project.read_dir() else {
            log::debug!("ini: {} does not exist", project.display());
            return Vec::new();
        };

        entries
            .flatten()
            .filter(|dir| dir.file_name().to_string_lossy().contains(SAVED))
            .map(|dir| dir.path().join("Config").join("Windows"))
            .collect()
    }

    fn application_scale(&self) -> Option<ScaleSetting> {
        Some(SCALE)
    }
}

impl LaunchArgumentsSupport for Nte {
    fn catalogue(&self) -> &'static str {
        include_str!("../../../../../../production/engine/NTE/launch_arguments.json")
    }
}
