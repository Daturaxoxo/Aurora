use anyhow::Result;
use log::*;
use std::path::Path;

pub const APP_ID: &str = "aurora";

#[cfg(target_os = "linux")]
const ICON: &[u8] = include_bytes!("../../../production/icons/logo.png");

pub fn install() {
    match std::env::current_exe() {
        Ok(exe) => install_for(&exe),
        Err(e) => warn!("Could not resolve the current exe for the desktop entry: {e}"),
    }
}

pub fn install_for(exe: &Path) {
    #[cfg(target_os = "linux")]
    if let Err(e) = install_inner_linux(exe) {
        warn!("Could not install the desktop entry: {e}");
    }

    #[cfg(target_os = "windows")]
    if let Err(e) = install_inner_windows(exe) {
        warn!("Could not install the desktop entry: {e}");
    }
}

#[cfg(target_os = "windows")]
fn install_inner_windows(_exe: &Path) -> Result<()> {
    todo!()
}

#[cfg(target_os = "linux")]
fn install_inner_linux(exe: &Path) -> Result<()> {
    let data_dir =
        dirs::data_dir().ok_or_else(|| anyhow!("could not resolve the data directory"))?;

    let icon_path = data_dir
        .join("icons/hicolor/64x64/apps")
        .join(format!("{APP_ID}.png"));
    write_if_changed(&icon_path, ICON)?;

    let entry_path = data_dir
        .join("applications")
        .join(format!("{APP_ID}.desktop"));
    write_if_changed(&entry_path, entry_contents(exe)?.as_bytes())?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn entry_contents(exe: &Path) -> Result<String> {
    let exec = quote_exec(&exe.display().to_string());

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

#[cfg(target_os = "linux")]
fn quote_exec(path: &str) -> String {
    let escaped = path.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}

#[cfg(target_os = "linux")]
fn write_if_changed(path: &Path, contents: &[u8]) -> Result<()> {
    use anyhow::anyhow;

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
