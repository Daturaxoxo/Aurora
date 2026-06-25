use std::env;
use std::fs::{self};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use log::*;
use regex::{Regex, RegexBuilder};

pub const SECTION_HEADER: &str = "[/Script/Engine.UserInterfaceSettings]";
pub const KEY: &str = "ApplicationScale";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakAddon {
    pub config_key: String,
    pub base_name: String,
}

impl PakAddon {
    pub fn new(config_key: String, base_name: String) -> Self {
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

    pub fn get_pak_addons() -> Vec<PakAddon> {
        vec![
            // Key.NO_DRIVE_LINE
            PakAddon::new("drv_lin".to_string(), "auddl_P".to_string()),
            // Key.HIDE_UID
            PakAddon::new("uid_rem".to_string(), "uidrm_P".to_string()),
            // Key.HIDE_NOTIF_DOTS
            PakAddon::new("nor_rem".to_string(), "nrdrm_P".to_string()),
        ]
    }
}

pub fn get_ini_path() -> PathBuf {
    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_default();
    PathBuf::from(local_app_data).join("HT/Saved_Global/Config/Windows/Engine.ini")
}

pub fn is_readonly(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.permissions().readonly())
        .unwrap_or(false)
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
    static SECTION_RE: OnceLock<Regex> = OnceLock::new();
    static NEWLINES_RE: OnceLock<Regex> = OnceLock::new();

    let section_re = SECTION_RE.get_or_init(|| {
        RegexBuilder::new(r"\[/Script/Engine\.UserInterfaceSettings\][^\[]*")
            .case_insensitive(true)
            .build()
            .unwrap()
    });

    let newlines_re = NEWLINES_RE.get_or_init(|| Regex::new(r"\n{3,}").unwrap());

    let cleaned = section_re.replace_all(text, "");
    let cleaned = newlines_re.replace_all(&cleaned, "\n\n");

    cleaned.trim_end_matches('\n').to_string()
}

pub fn get_current_scale() -> f64 {
    let path = get_ini_path();
    if !path.exists() {
        return 1.0;
    }

    if let Ok(text) = fs::read_to_string(&path) {
        static SCALE_RE: OnceLock<Regex> = OnceLock::new();
        let scale_re = SCALE_RE.get_or_init(|| {
            RegexBuilder::new(
                r"\[/Script/Engine\.UserInterfaceSettings\].*?ApplicationScale\s*=\s*([0-9.]+)",
            )
            .case_insensitive(true)
            .dot_matches_new_line(true)
            .build()
            .unwrap()
        });

        if let Some(caps) = scale_re.captures(&text) {
            if let Some(m) = caps.get(1) {
                return m.as_str().parse().unwrap_or(1.0);
            }
        }
    }

    1.0
}

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
    let new_text = format!("{}\n\n{}\n{}={}\n", base, SECTION_HEADER, KEY, scale);

    match fs::write(&path, new_text) {
        Ok(_) => {
            set_readonly(&path, true);
            true
        }
        Err(e) => {
            error!("engine_ini.apply_scale failed: {}", e);
            false
        }
    }
}

pub fn remove_scale() -> bool {
    let path = get_ini_path();
    if !path.exists() {
        return true;
    }

    if is_readonly(&path) {
        set_readonly(&path, false);
    }

    match fs::read_to_string(&path) {
        Ok(existing) => {
            let cleaned = strip_section(&existing);
            fs::write(&path, format!("{}\n", cleaned)).is_ok()
        }
        Err(_) => false,
    }
}

pub fn ini_path() -> PathBuf {
    get_ini_path()
}
