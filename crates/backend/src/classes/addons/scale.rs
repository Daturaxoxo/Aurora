use log::error;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const SECTION_HEADER: &str = "[/Script/Engine.UserInterfaceSettings]";
pub const KEY: &str = "ApplicationScale";

fn get_windows_ini_path() -> PathBuf {
    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_default();
    PathBuf::from(local_app_data).join("HT/Saved_Global/Config/Windows/Engine.ini")
}

#[cfg(unix)]
fn get_unix_ini_path() -> PathBuf {
    todo!("Linux/Proton path for Engine.ini is not yet implemented");
}

pub fn get_ini_path() -> PathBuf {
    cfg_select! {
        windows => get_windows_ini_path(),
        unix => get_unix_ini_path()
    }
}

pub fn ini_path() -> PathBuf {
    get_ini_path()
}

pub fn is_readonly(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.permissions().readonly())
}

pub fn set_readonly(path: &Path, readonly: bool) {
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
        if line.trim().eq_ignore_ascii_case(SECTION_HEADER) {
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
        if trimmed.eq_ignore_ascii_case(SECTION_HEADER) {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with('[') {
                break;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                if k.trim().eq_ignore_ascii_case(KEY) {
                    return v.trim().parse().unwrap_or(1.0);
                }
            }
        }
    }

    1.0
}

pub fn apply_scale(scale: f64) -> bool {
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

    // Avoid leading blank lines when writing into a fresh/empty file.
    let mut new_text = base;
    if !new_text.is_empty() {
        new_text.push_str("\n\n");
    }
    new_text.push_str(&format!("{SECTION_HEADER}\n{KEY}={scale}\n"));

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
