use std::path::Path;

use anyhow::Result;
#[cfg(any(target_os = "windows", target_os = "linux"))]
use anyhow::anyhow;

#[cfg(target_os = "windows")]
pub fn create_desktop_shortcut(target: &Path) -> Result<()> {
    let desktop = dirs::desktop_dir().ok_or_else(|| anyhow!("could not find desktop directory"))?;
    create_lnk(target, &desktop.join("Aurora.lnk"))
}

#[cfg(target_os = "linux")]
pub fn create_desktop_shortcut(_target: &Path) -> Result<()> {
    Ok(())
}

pub fn add_start_menu(target: &Path) -> Result<()> {
    shared::desktop_entry::install_for(target);
    Ok(())
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
pub fn launch_aurora_elevated(exe: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    let operation = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    let dir = exe.parent().map(|p| wide(p.as_os_str()));

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            SW_SHOWNORMAL,
        )
    };

    if result as isize <= 32 {
        return Err(anyhow!("ShellExecuteW failed (code {})", result as isize));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn launch_aurora_elevated(exe: &Path) -> Result<()> {
    use std::process::Command;

    // Aurora doesn't need to run as root on Linux.
    Command::new(exe)
        .spawn()
        .map_err(|e| anyhow!("failed to launch {}: {e}", exe.display()))?;
    Ok(())
}
