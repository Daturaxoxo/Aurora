use std::path::PathBuf;

use super::Game;

// per-user folders a game writes to, inside the game's Proton prefix on Linux
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlatformPaths {
    pub local_app_data: Option<PathBuf>,
    pub pictures: Option<PathBuf>,
}

impl PlatformPaths {
    #[cfg(target_os = "windows")]
    pub fn observe_host(_game: &dyn Game) -> Self {
        Self {
            local_app_data: std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
            pictures: dirs::picture_dir(),
        }
    }

    #[cfg(target_os = "linux")]
    pub fn observe_host(game: &dyn Game) -> Self {
        let Some(user) = game
            .descriptor()
            .steam
            .and_then(|metadata| crate::classes::steam::compat_prefix(&metadata))
            .map(|prefix| prefix.join("drive_c").join("users").join("steamuser"))
        else {
            return Self::default();
        };

        Self {
            local_app_data: Some(user.join("AppData").join("Local")),
            pictures: Some(user.join("Pictures")),
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    pub fn observe_host(_game: &dyn Game) -> Self {
        Self::default()
    }
}
