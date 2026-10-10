use std::path::{Path, PathBuf};

use crate::logic::GameRoot;
use crate::plan::Plan;
use crate::run::Options;

const ELEVATED_ARG: &str = "--elevated";
const DELETE_CONFIG_ARG: &str = "--delete-config";
const DELETE_MODS_ARG: &str = "--delete-mods";
const PRESERVE_MODS_ARG: &str = "--preserve-mods";
const DATA_DIR_ARG: &str = "--data-dir";
const APP_DIR_ARG: &str = "--app-dir";
const GAME_ROOT_ARG: &str = "--game-root";
const BACKUP_DIR_ARG: &str = "--backup-dir";

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

pub fn plan_from_args() -> Plan {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| -> Option<PathBuf> {
        args.iter()
            .position(|arg| arg == flag)
            .and_then(|index| args.get(index + 1))
            .filter(|value| !value.is_empty() && !value.starts_with("--"))
            .map(PathBuf::from)
    };

    let game_roots: Vec<GameRoot> = args
        .windows(2)
        .filter(|pair| pair[0] == GAME_ROOT_ARG)
        .filter_map(|pair| match GameRoot::parse_arg(&pair[1]) {
            Ok(root) => Some(root),
            Err(e) => {
                log::warn!("Ignoring {GAME_ROOT_ARG} {}: {e:#}", pair[1]);
                None
            }
        })
        .collect();

    let fallback = Plan::resolve();
    Plan::from_paths(
        value(DATA_DIR_ARG).unwrap_or(fallback.data_dir),
        value(APP_DIR_ARG).or(fallback.app_dir),
        if game_roots.is_empty() {
            fallback.game_roots
        } else {
            game_roots
        },
        value(BACKUP_DIR_ARG).unwrap_or(fallback.backup_dir),
    )
}

pub fn needed(plan: &Plan, options: Options) -> bool {
    if options.delete_mods && plan.mods.iter().any(|mods| !can_remove(&mods.path)) {
        return true;
    }

    if let Some(app_dir) = plan.app_dir.as_deref()
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
    dir.parent().is_none_or(is_writable) && is_writable(dir)
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

pub fn relaunch(plan: &Plan, options: Options) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    const SE_ERR_ACCESSDENIED: isize = 5;

    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn push_path(params: &mut String, flag: &str, path: Option<&Path>) {
        let Some(path) = path else {
            return;
        };
        push_value(params, flag, &path.display().to_string());
    }

    fn push_value(params: &mut String, flag: &str, text: &str) {
        let trimmed = text.trim_end_matches('\\');
        if trimmed.is_empty() {
            return;
        }
        params.push(' ');
        params.push_str(flag);
        params.push_str(" \"");
        params.push_str(trimmed);
        params.push('"');
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
    push_path(&mut params, DATA_DIR_ARG, Some(&plan.data_dir));
    push_path(&mut params, APP_DIR_ARG, plan.app_dir.as_deref());
    for root in &plan.game_roots {
        push_value(&mut params, GAME_ROOT_ARG, &root.to_arg());
    }
    push_path(&mut params, BACKUP_DIR_ARG, Some(&plan.backup_dir));

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
