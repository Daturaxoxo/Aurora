use std::path::Path;

use crate::plan::Plan;
use crate::run::Options;
const ELEVATED_ARG: &str = "--elevated";
const DELETE_CONFIG_ARG: &str = "--delete-config";
const DELETE_MODS_ARG: &str = "--delete-mods";
const PRESERVE_MODS_ARG: &str = "--preserve-mods";
pub fn is_relaunch() -> bool {
    std::env::args().any(|arg| arg == ELEVATED_ARG)
}
pub fn options_from_args() -> Options {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|arg| arg == flag);

    Options {
        delete_config: has(DELETE_CONFIG_ARG),
        delete_mods: has(DELETE_MODS_ARG),
        preserve_mods: has(PRESERVE_MODS_ARG),
    }
}
pub fn needed(plan: &Plan, options: Options) -> bool {
    if options.delete_mods
        && let Some(mods) = plan.mods_dir.as_deref()
        && !can_remove(mods)
    {
        return true;
    }

    if !plan.dev_build
        && let Some(app_dir) = plan.app_dir.as_deref()
        && !can_remove(app_dir)
    {
        return true;
    }

    options.delete_config && !can_remove(&plan.data_dir)
}
fn can_remove(dir: &Path) -> bool {
    if !dir.exists() {
        return true;
    }
    is_writable(dir) && dir.parent().is_none_or(is_writable)
}

fn is_writable(dir: &Path) -> bool {
    let probe = dir.join(".aurora-uninstall-probe");
    match std::fs::File::create(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}
#[cfg(windows)]
pub fn relaunch(options: Options) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    const SE_ERR_ACCESSDENIED: isize = 5;

    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let exe = std::env::current_exe().map_err(|e| format!("could not resolve the exe: {e}"))?;

    let mut params = String::from(ELEVATED_ARG);
    if options.delete_config {
        params.push(' ');
        params.push_str(DELETE_CONFIG_ARG);
    }
    if options.delete_mods {
        params.push(' ');
        params.push_str(DELETE_MODS_ARG);
    }
    if options.preserve_mods {
        params.push(' ');
        params.push_str(PRESERVE_MODS_ARG);
    }

    let operation = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    let params = wide(params.as_ref());
    let dir = exe.parent().map(|p| wide(p.as_os_str()));

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()),
            SW_SHOWNORMAL,
        )
    } as isize;

    match result {
        code if code > 32 => Ok(()),
        SE_ERR_ACCESSDENIED => {
            Err("Administrator permissions are required to remove these files.".into())
        }
        code => Err(format!("could not restart as administrator (code {code})")),
    }
}

#[cfg(not(windows))]
pub fn relaunch(_options: Options) -> Result<(), String> {
    Err("Aurora's files are owned by another user. Please retry with elevated permissions.".into())
}
