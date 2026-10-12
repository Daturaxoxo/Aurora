use crate::classes::helpers::ini::{self, Ini, IniFile};
use log::*;
use shared::classes::games::capabilities::ScaleSetting;
use std::path::PathBuf;
pub const SUPPORTED: bool = ini::SUPPORTED;

fn file(setting: &ScaleSetting) -> Option<IniFile> {
    let file = IniFile::from_name(setting.ini.file);
    if file.is_none() {
        error!("scale: {} is not a known ini file", setting.ini.file);
    }
    file
}

pub fn get_current_scale(dirs: &[PathBuf], setting: &ScaleSetting) -> f64 {
    file(setting)
        .and_then(|file| ini::value(dirs, file, setting.ini.section, setting.ini.key))
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0)
}

pub fn apply_scale(dirs: &[PathBuf], setting: &ScaleSetting, scale: f64) -> bool {
    let Some(file) = file(setting) else {
        return false;
    };
    let scale = scale.clamp(setting.min, setting.max);
    let scale = (scale * 100.0).round() / 100.0;

    match Ini::file(dirs, file)
        .set(setting.ini.section, setting.ini.key, scale.to_string())
        .commit()
    {
        Ok(report) if report.found > 0 => true,
        Ok(_) => {
            error!("scale: found no {file} to write");
            false
        }
        Err(e) => {
            error!("scale: {e}");
            false
        }
    }
}

pub fn remove_scale(dirs: &[PathBuf], setting: &ScaleSetting) -> bool {
    let Some(file) = file(setting) else {
        return false;
    };
    match Ini::file(dirs, file)
        .remove(setting.ini.section, setting.ini.key)
        .commit()
    {
        Ok(_) => true,
        Err(e) => {
            error!("scale: {e}");
            false
        }
    }
}
