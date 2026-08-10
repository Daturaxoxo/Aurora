pub mod addons;
pub mod gbbrowser;
pub mod lua;
pub mod modmanager;
pub mod modules;
pub mod screenshots;
pub mod settings;

use std::path::{Component, Path};

const MAX_FILENAME_LEN: usize = 128;

const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub (crate) const INVALID_FILENAME_CHARS: [char; 10] = ['/', '\\', '\0', ':', '*', '?', '"', '<', '>', '|'];

fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    RESERVED_DEVICE_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
}

fn trim_filename(name: &str) -> &str {
    name.trim_matches(|c: char| c.is_whitespace() || c == '.')
}

pub fn sanitize_download_filename(name: &str) -> Option<String> {
    if name.len() > 1024 {
        return None;
    }

    let trimmed = trim_filename(name);
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains(&INVALID_FILENAME_CHARS[..]) {
        return None;
    }
    if trimmed.chars().any(char::is_control) {
        return None;
    }

    let mut components = Path::new(trimmed).components();
    let Some(Component::Normal(only)) = components.next() else {
        return None;
    };
    if components.next().is_some() || only.to_str() != Some(trimmed) {
        return None;
    }

    let mut safe = trimmed.to_string();
    if safe.len() > MAX_FILENAME_LEN {
        let ext = Path::new(&safe)
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.len() <= 16)
            .map_or_else(String::new, |e| format!(".{e}"));
        let mut cut = MAX_FILENAME_LEN.saturating_sub(ext.len());
        while cut > 0 && !safe.is_char_boundary(cut) {
            cut -= 1;
        }
        safe.truncate(cut);
        let shortened = format!("{}{ext}", trim_filename(&safe));
        safe = shortened;
    }

    let safe = trim_filename(&safe).to_string();
    if safe.is_empty() || is_reserved_device_name(&safe) {
        return None;
    }

    Some(safe)
}
