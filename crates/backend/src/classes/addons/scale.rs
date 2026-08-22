use log::*;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

pub const SECTION_HEADER: &str = "[/Script/Engine.UserInterfaceSettings]";
pub const KEY: &str = "ApplicationScale";

/// Whether application scaling is available on this platform.
pub const SUPPORTED: bool = cfg!(any(target_os = "windows", target_os = "linux"));

#[cfg(target_os = "windows")]
fn get_windows_ini_paths() -> Option<Vec<PathBuf>> {
    use std::env;

    let local_app_data = env::var("LOCALAPPDATA").unwrap_or_default();
    let ht_path = PathBuf::from(local_app_data).join("HT/");
    let mut result = Vec::new();
    for dir in ht_path.read_dir().ok()?.flatten() {
        if dir.file_name().to_string_lossy().contains("Saved") {
            result.push(dir.path().join("Config/Windows/Engine.ini"));
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

#[cfg(unix)]
fn get_unix_ini_paths() -> Option<Vec<PathBuf>> {
    use shared::classes::steam;

    let pfx = steam::aurora_prefix()?;
    let ht_path = pfx
        .join("drive_c")
        .join("users")
        .join("steamuser")
        .join("AppData")
        .join("Local")
        .join("HT");

    let mut result = Vec::new();
    for dir in ht_path.read_dir().ok()?.flatten() {
        if dir.file_name().to_string_lossy().contains("Saved_Global") {
            result.push(dir.path().join("Config/Windows/Engine.ini"));
        }
    }

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

pub fn get_ini_paths() -> Option<Vec<PathBuf>> {
    if !SUPPORTED {
        return None;
    }

    cfg_select! {
        windows => get_windows_ini_paths(),
        unix => get_unix_ini_paths(),
    }
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

fn scale_in(text: &str) -> Option<f64> {
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
            if let Some((k, v)) = trimmed.split_once('=')
                && k.trim().eq_ignore_ascii_case(KEY) {
                    return v.trim().parse().ok();
                }
        }
    }

    None
}

pub fn get_current_scale() -> f64 {
    get_ini_paths()
        .unwrap_or_default()
        .iter()
        .find_map(|path| {
            let text = fs::read_to_string(path).ok()?;
            scale_in(&text)
        })
        .unwrap_or(1.0)
}

pub fn apply_scale(scale: f64) -> bool {
    let scale = scale.clamp(0.5, 2.0);
    let scale = (scale * 100.0).round() / 100.0;

    let Some(paths) = get_ini_paths() else {
        error!("engine_ini.apply_scale found no Engine.ini to write");
        return false;
    };

    let mut all_written = true;
    for path in &paths {
        if !apply_scale_to(path, scale) {
            all_written = false;
        }
    }

    all_written
}

fn apply_scale_to(path: &Path, scale: f64) -> bool {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if path.exists() {
        set_readonly(path, false);
    }

    let existing = fs::read_to_string(path).unwrap_or_default();
    let base = strip_section(&existing);

    // Avoid leading blank lines when writing into a fresh/empty file.
    let mut new_text = base;
    if !new_text.is_empty() {
        new_text.push_str("\n\n");
    }
    let _ = write!(new_text, "{SECTION_HEADER}\n{KEY}={scale}\n");

    let tmp = path.with_extension("ini.tmp");
    if let Err(e) = fs::write(&tmp, &new_text) {
        error!("engine_ini.apply_scale failed: {e}");
        return false;
    }

    if !existing.is_empty() {
        let backup = path.with_extension("ini.bak");
        if let Err(e) = fs::write(&backup, &existing) {
            warn!(
                "engine_ini.apply_scale could not back up {}: {e}",
                path.display()
            );
        }
    }

    if let Err(e) = fs::rename(&tmp, path) {
        error!("engine_ini.apply_scale failed: {e}");
        let _ = fs::remove_file(&tmp);
        return false;
    }

    set_readonly(path, true);
    true
}

pub fn remove_scale() -> bool {
    let Some(paths) = get_ini_paths() else {
        return true;
    };

    let mut all_cleaned = true;
    for path in &paths {
        if !remove_scale_from(path) {
            all_cleaned = false;
        }
    }

    all_cleaned
}

fn remove_scale_from(path: &Path) -> bool {
    if !path.exists() {
        return true;
    }

    if is_readonly(path) {
        set_readonly(path, false);
    }

    fs::read_to_string(path).is_ok_and(|existing| {
        let cleaned = strip_section(&existing);
        fs::write(path, format!("{cleaned}\n")).is_ok()
    })
}
