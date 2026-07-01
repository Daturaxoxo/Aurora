use log::error;
use std::env;
use std::fs::{self};
use std::path::{Path, PathBuf};

pub const SECTION_HEADER: &str = "[/Script/Engine.UserInterfaceSettings]";
pub const KEY: &str = "ApplicationScale";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakAddon {
    pub config_key: String,
    pub base_name: String,
}

impl PakAddon {
    #[must_use]
    pub const fn new(config_key: String, base_name: String) -> Self {
        Self {
            config_key,
            base_name,
        }
    }

    #[must_use]
    pub fn files(&self) -> Vec<String> {
        vec![
            format!("{}.pak", self.base_name),
            format!("{}.utoc", self.base_name),
            format!("{}.ucas", self.base_name),
        ]
    }

    #[must_use]
    pub fn get_pak_addons() -> Vec<Self> {
        vec![
            // Key.NO_DRIVE_LINE
            Self::new("drv_lin".to_string(), "auddl_P".to_string()),
            // Key.HIDE_UID
            Self::new("uid_rem".to_string(), "uidrm_P".to_string()),
            // Key.HIDE_NOTIF_DOTS
            Self::new("nor_rem".to_string(), "nrdrm_P".to_string()),
        ]
    }
}

#[must_use]
pub fn get_ini_path() -> PathBuf {
    // TODO: Make compatible with linux
    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_default();
    PathBuf::from(local_app_data).join("HT/Saved_Global/Config/Windows/Engine.ini")
}

#[must_use]
pub fn is_readonly(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.permissions().readonly())
}

pub fn set_readonly(path: &Path, readonly: bool) {
    // let flag = if readonly { "+R" } else { "-R" };
    // let _ = Command::new("attrib").args([flag, path.to_str().unwrap()]).output();

    if let Ok(metadata) = fs::metadata(path) {
        let mut perms = metadata.permissions();
        perms.set_readonly(readonly);
        let _ = fs::set_permissions(path, perms);
    }
}

pub fn strip_section(text: &str) -> String {
    let mut result = Vec::new();
    let mut in_section = false;

    for line in text.lines() {
        if line
            .trim()
            .eq_ignore_ascii_case("[/Script/Engine.UserInterfaceSettings]")
        {
            in_section = true;
            continue;
        }
        if in_section {
            if line.trim_start().starts_with('[') {
                in_section = false;
            } else {
                continue;
            }
        }
        result.push(line);
    }

    let mut out = String::new();
    let mut blank_count = 0u32;
    for line in &result {
        if line.trim().is_empty() {
            blank_count += 1;
            if blank_count <= 2 {
                out.push('\n');
            }
        } else {
            blank_count = 0;
            out.push_str(line);
            out.push('\n');
        }
    }

    out.trim_end_matches('\n').to_string()
}

pub fn get_current_scale() -> f64 {
    let path = get_ini_path();
    if !path.exists() {
        return 1.0;
    }

    let Ok(text) = fs::read_to_string(&path) else {
        return 1.0;
    };

    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("[/Script/Engine.UserInterfaceSettings]") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with('[') {
                break;
            }
            if let Some(rest) = trimmed.split_once('=') {
                if rest.0.trim().eq_ignore_ascii_case("ApplicationScale") {
                    return rest.1.trim().parse().unwrap_or(1.0);
                }
            }
        }
    }

    1.0
}

#[must_use]
pub fn apply_scale(scale: f64) -> bool {
    // Original python code: scale = round(max(0.5, min(2.0, scale)), 2)
    let scale = scale.clamp(0.5, 2.0);
    let scale = (scale * 100.0).round() / 100.0;
    let path = get_ini_path();

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if path.exists() {
        set_readonly(&path, false);
    }

    let existing = fs::read_to_string(&path).unwrap_or_default();
    let base = strip_section(&existing);
    let new_text = format!("{base}\n\n{SECTION_HEADER}\n{KEY}={scale}\n");

    match fs::write(&path, new_text) {
        Ok(()) => {
            set_readonly(&path, true);
            true
        }
        Err(e) => {
            error!("engine_ini.apply_scale failed: {e}");
            false
        }
    }
}

#[must_use]
pub fn remove_scale() -> bool {
    let path = get_ini_path();
    if !path.exists() {
        return true;
    }

    if is_readonly(&path) {
        set_readonly(&path, false);
    }

    fs::read_to_string(&path).is_ok_and(|existing| {
        let cleaned = strip_section(&existing);
        fs::write(&path, format!("{cleaned}\n")).is_ok()
    })
}

#[must_use]
pub fn ini_path() -> PathBuf {
    get_ini_path()
}
