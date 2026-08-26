use log::*;
use crate::classes::helpers::ini::{self, Ini, IniFile};
pub const SECTION_HEADER: &str = "[/Script/Engine.UserInterfaceSettings]";
pub const KEY: &str = "ApplicationScale";
pub const SUPPORTED: bool = ini::SUPPORTED;

pub fn get_current_scale() -> f64 {
    ini::value(IniFile::Engine, SECTION_HEADER, KEY)
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0)
}

pub fn apply_scale(scale: f64) -> bool {
    let scale = scale.clamp(0.5, 2.0);
    let scale = (scale * 100.0).round() / 100.0;

    match Ini::file(IniFile::Engine)
        .set(SECTION_HEADER, KEY, scale.to_string())
        .commit()
    {
        Ok(report) if report.found > 0 => true,
        Ok(_) => {
            error!("scale: found no Engine.ini to write");
            false
        }
        Err(e) => {
            error!("scale: {e}");
            false
        }
    }
}

pub fn remove_scale() -> bool {
    match Ini::file(IniFile::Engine)
        .remove(SECTION_HEADER, KEY)
        .commit()
    {
        Ok(_) => true,
        Err(e) => {
            error!("scale: {e}");
            false
        }
    }
}
