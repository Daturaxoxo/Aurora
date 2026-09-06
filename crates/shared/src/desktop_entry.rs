use anyhow::{Result, anyhow};
use log::*;
use std::path::Path;

use crate::utils::{remove_if_present, write_if_changed};

pub const APP_ID: &str = "aurora";

#[cfg(target_os = "windows")]
const SHORTCUT_FILE: &str = "Aurora.lnk";

#[cfg(target_os = "windows")]
const QUICK_START_FILE: &str = "Aurora Quick Start.lnk";

#[cfg(target_os = "windows")]
const QUICK_START_ICON_FILE: &str = "startup.ico";

#[cfg(target_os = "windows")]
const QUICK_START_ICON: &[u8] = include_bytes!("../../../production/icons/startup.ico");

#[cfg(target_os = "linux")]
const ICON: &[u8] = include_bytes!("../../../production/icons/logo.png");

#[cfg(target_os = "windows")]
#[derive(Default)]
struct LnkOptions<'a> {
    description: Option<&'a str>,
    arguments: Option<&'a str>,
    icon: Option<&'a Path>,
}

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
    create_lnk(exe, &start_menu_shortcut()?, &LnkOptions::default())
}

#[cfg(target_os = "windows")]
fn uninstall_inner_windows() -> Result<()> {
    let start_menu = start_menu_shortcut().and_then(|path| remove_if_present(&path));
    let quick_start = remove_quick_start_shortcut();
    start_menu.and(quick_start)
}

#[cfg(target_os = "windows")]
fn create_lnk(target: &Path, shortcut: &Path, options: &LnkOptions) -> Result<()> {
    if let Some(parent) = shortcut.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut link = mslnk::ShellLink::new(target)?;
    link.set_name(Some(options.description.unwrap_or("Aurora").to_string()));
    link.set_working_dir(
        target
            .parent()
            .and_then(|p| p.to_str())
            .map(std::string::ToString::to_string),
    );
    link.set_arguments(options.arguments.map(std::string::ToString::to_string));
    link.set_icon_location(
        options
            .icon
            .and_then(|icon| icon.to_str())
            .map(std::string::ToString::to_string),
    );
    link.create_lnk(shortcut)?;

    info!("Created {}", shortcut.display());
    Ok(())
}

#[cfg(target_os = "windows")]
fn desktop_dir() -> Result<std::path::PathBuf> {
    dirs::desktop_dir().ok_or_else(|| anyhow!("could not find the desktop directory"))
}

#[cfg(target_os = "windows")]
fn quick_start_icon_path() -> std::path::PathBuf {
    crate::config::get_userdata_path().with_file_name(QUICK_START_ICON_FILE)
}

#[cfg(target_os = "windows")]
pub fn create_desktop_shortcut(target: &Path) -> Result<()> {
    create_lnk(
        target,
        &desktop_dir()?.join(SHORTCUT_FILE),
        &LnkOptions::default(),
    )
}

#[cfg(target_os = "windows")]
pub fn create_quick_start_shortcut(target: &Path) -> Result<()> {
    let shortcut = desktop_dir()?.join(QUICK_START_FILE);
    if shortcut.exists() {
        info!("{} already exists; leaving it as it is", shortcut.display());
        return Ok(());
    }

    let icon = quick_start_icon_path();
    write_if_changed(&icon, QUICK_START_ICON)?;

    create_lnk(
        target,
        &shortcut,
        &LnkOptions {
            description: Some("Aurora Quick Start"),
            arguments: Some(ipc::QUICK_START_ARG),
            icon: Some(&icon),
        },
    )
    .inspect_err(|_| {
        let _ = std::fs::remove_file(&icon);
    })
}

#[cfg(target_os = "windows")]
pub fn remove_quick_start_shortcut() -> Result<()> {
    let icon = remove_if_present(&quick_start_icon_path());
    let shortcut =
        desktop_dir().and_then(|desktop| remove_if_present(&desktop.join(QUICK_START_FILE)));
    icon.and(shortcut)
}

#[cfg(target_os = "windows")]
pub fn remove_desktop_shortcut() -> Result<()> {
    remove_if_present(&desktop_dir()?.join(SHORTCUT_FILE))
}

#[cfg(not(target_os = "windows"))]
pub const fn remove_desktop_shortcut() -> Result<()> {
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

#[cfg(target_os = "linux")]
fn install_inner_linux(exe: &Path) -> Result<()> {
    use std::process::Command;

    let data_dir =
        dirs::data_dir().ok_or_else(|| anyhow!("could not resolve the data directory"))?;

    let icon_path = data_dir
        .join("icons/hicolor/64x64/apps")
        .join(format!("{APP_ID}.png"));
    write_if_changed(&icon_path, ICON)?;

    let applications = data_dir.join("applications");
    let entry_path = applications.join(format!("{APP_ID}.desktop"));
    write_if_changed(&entry_path, entry_contents(exe).as_bytes())?;

    match Command::new("update-desktop-database")
        .arg(&applications)
        .status()
    {
        Ok(status) if !status.success() => {
            info!("update-desktop-database exited with {status}");
        }
        Err(e) => info!("update-desktop-database is unavailable: {e}"),
        Ok(_) => {}
    }

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
         Exec={exec} %u\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         Categories=Game;\n\
         MimeType=x-scheme-handler/{scheme};\n\
         StartupNotify=true\n\
         StartupWMClass={APP_ID}\n",
        scheme = crate::oneclick::SCHEME,
    )
}

#[cfg(target_os = "linux")]
fn quote_exec(path: &str) -> String {
    let escaped = path.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}
