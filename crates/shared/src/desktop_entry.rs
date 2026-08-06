use anyhow::{anyhow, Result};
use log::*;
use std::path::Path;

pub const APP_ID: &str = "aurora";

#[cfg(target_os = "windows")]
const SHORTCUT_FILE: &str = "Aurora.lnk";

#[cfg(target_os = "linux")]
const ICON: &[u8] = include_bytes!("../../../production/icons/logo.png");

pub fn install() {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = ipc::appimage_path() {
        install_for(&appimage);
        return;
    }

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

#[cfg(target_os = "linux")]
pub fn uninstall() {
    if let Err(e) = uninstall_inner_linux() {
        warn!("Could not remove the desktop entry: {e}");
    }
}

#[cfg(target_os = "windows")]
pub fn uninstall() {
    if let Err(e) = uninstall_inner_windows() {
        warn!("Could not remove the desktop entry: {e}");
    }
}

#[cfg(target_os = "windows")]
fn start_menu_shortcut() -> Result<std::path::PathBuf> {
    Ok(dirs::config_dir()
        .ok_or_else(|| anyhow!("could not find the AppData directory"))?
        .join("Microsoft/Windows/Start Menu/Programs")
        .join(SHORTCUT_FILE))
}

#[cfg(target_os = "windows")]
fn install_inner_windows(exe: &Path) -> Result<()> {
    let shortcut = start_menu_shortcut()?;
    if let Some(parent) = shortcut.parent() {
        std::fs::create_dir_all(parent)?;
    }
    create_lnk(exe, &shortcut)
}

#[cfg(target_os = "windows")]
fn uninstall_inner_windows() -> Result<()> {
    remove_if_present(&start_menu_shortcut()?)
}

#[cfg(target_os = "windows")]
fn create_lnk(target: &Path, shortcut: &Path) -> Result<()> {
    let mut link = mslnk::ShellLink::new(target)?;
    link.set_name(Some("Aurora".to_string()));
    link.set_working_dir(
        target
            .parent()
            .and_then(|p| p.to_str())
            .map(std::string::ToString::to_string),
    );
    link.create_lnk(shortcut)?;
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn create_desktop_shortcut(target: &Path) -> Result<()> {
    let desktop = dirs::desktop_dir().ok_or_else(|| anyhow!("could not find desktop directory"))?;
    create_lnk(target, &desktop.join(SHORTCUT_FILE))
}

#[cfg(target_os = "windows")]
pub fn remove_desktop_shortcut() -> Result<()> {
    let desktop = dirs::desktop_dir().ok_or_else(|| anyhow!("could not find desktop directory"))?;
    remove_if_present(&desktop.join(SHORTCUT_FILE))
}

#[cfg(not(target_os = "windows"))]
pub fn remove_desktop_shortcut() -> Result<()> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_inner_linux() -> Result<()> {
    let data_dir =
        dirs::data_dir().ok_or_else(|| anyhow!("could not resolve the data directory"))?;

    remove_if_present(
        &data_dir
            .join("applications")
            .join(format!("{APP_ID}.desktop")),
    )?;
    remove_if_present(
        &data_dir
            .join("icons/hicolor/64x64/apps")
            .join(format!("{APP_ID}.png")),
    )?;

    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn remove_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            info!("Removed {}", path.display());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
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
    write_if_changed(&entry_path, entry_contents(exe).as_bytes())?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn entry_contents(exe: &Path) -> String {
    let exec = quote_exec(&exe.display().to_string());

    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Aurora\n\
         Comment=Mod manager and launcher\n\
         Exec={exec}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Game;\n\
         StartupNotify=true\n\
         StartupWMClass={APP_ID}\n"
    )
}

#[cfg(target_os = "linux")]
fn quote_exec(path: &str) -> String {
    let escaped = path.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}

#[cfg(target_os = "linux")]
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
