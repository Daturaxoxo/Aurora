use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakAddon {
    pub config_key: String,
    pub base_name: String,
}

impl PakAddon {
    pub const fn new(config_key: String, base_name: String) -> Self {
        Self {
            config_key,
            base_name,
        }
    }

    pub fn files(&self) -> Vec<String> {
        vec![
            format!("{}.pak", self.base_name),
            format!("{}.utoc", self.base_name),
            format!("{}.ucas", self.base_name),
        ]
    }

    pub fn resolve(&self, pak_dir: &Path) -> Vec<ResolvedAddonFile> {
        self.files()
            .into_iter()
            .map(|file_name| {
                let path = pak_dir.join(&file_name);
                ResolvedAddonFile { file_name, path }
            })
            .collect()
    }

    pub fn get_pak_addons() -> Vec<Self> {
        vec![
            // key::NO_DRIVE_LINE
            Self::new("drv_lin".to_string(), "DisableDrivingLine_P".to_string()),
            // key::HIDE_UID
            Self::new("uid_rem".to_string(), "HideUI_UserID_P".to_string()),
            // key::HIDE_NOTIF_DOTS
            Self::new("nor_rem".to_string(), "Disable_RedDot_P".to_string()),
            // key::COOLDOWN_TIMER
            Self::new("col_tim".to_string(), "CooldownTimer_P".to_string()),
            // key::COLLECTIBLES
            Self::new("collectibles".to_string(), "ItemOutline_P".to_string()),
        ]
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedAddonFile {
    pub file_name: String,
    pub path: PathBuf,
}

impl ResolvedAddonFile {
    pub fn to_folder_name(&self) -> String {
        let base_name = self
            .file_name
            .strip_suffix(".pak")
            .or_else(|| self.file_name.strip_suffix(".utoc"))
            .or_else(|| self.file_name.strip_suffix(".ucas"))
            .unwrap_or(&self.file_name);
        match base_name {
            "DisableDrivingLine_P" => "NoDriveLine",
            "HideUI_UserID_P" => "HideUID",
            "Disable_RedDot_P" => "HideRedDots",
            "CooldownTimer_P" => "CooldownTimers",
            "ItemOutline_P" => "Collectibles",
            _ => base_name,
        }
        .to_string()
    }
}
