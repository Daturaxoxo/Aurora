use anyhow::{anyhow, Result};
use log::*;
use std::path::Path;

pub const APP_ID: &str = "aurora";

const ICON: &[u8] = include_bytes!("../../../../production/icons/logo.png");

pub fn install() {
    if let Err(e) = install_inner() {
        warn!("Could not install the desktop entry: {e}");
    }
}

fn install_inner() -> Result<()> {
    let data_dir =
        dirs::data_dir().ok_or_else(|| anyhow!("could not resolve the data directory"))?;

    let icon_path = data_dir
        .join("icons/hicolor/64x64/apps")
        .join(format!("{APP_ID}.png"));
    write_if_changed(&icon_path, ICON)?;

    let entry_path = data_dir
        .join("applications")
        .join(format!("{APP_ID}.desktop"));
    write_if_changed(&entry_path, entry_contents()?.as_bytes())?;

    Ok(())
}

fn entry_contents() -> Result<String> {
    let exec = quote_exec(&std::env::current_exe()?.display().to_string());

    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Aurora\n\
         Comment=Mod manager and launcher for NTE\n\
         Exec={exec}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Game;\n\
         StartupNotify=true\n\
         StartupWMClass={APP_ID}\n"
    ))
}

fn quote_exec(path: &str) -> String {
    let escaped = path.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<()> {
    if std::fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }

    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)?;
    std::fs::write(path, contents)?;
    info!("Wrote {}", path.display());

    Ok(())
}
