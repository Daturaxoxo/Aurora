use crate::UninstallerWindow;
use crate::owned;
use crate::plan::Plan;
use shared::utils::format_bytes;
use slint::{Model, SharedString, VecModel, Weak};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
pub const CLEANUP_ARG: &str = "--cleanup";
const CLEANUP_RETRY_INTERVAL: Duration = Duration::from_millis(200);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
static PENDING_CLEANUP: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Clone, Copy)]
pub struct Options {
    pub delete_config: bool,
    pub delete_mods: bool,
    pub preserve_mods: bool,
}

pub fn run(ui: Weak<UninstallerWindow>, plan: Plan, app_dir: PathBuf, options: Options) {
    std::thread::spawn(move || {
        let result = run_inner(&ui, &plan, &app_dir, options);
        let (backup, error) = match result {
            Ok(backup) => (backup.unwrap_or_default(), String::new()),
            Err(e) => {
                log_line(&ui, format!("ERROR: {e}"));
                (String::new(), e)
            }
        };

        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = ui.upgrade() {
                w.set_uninstall_success(error.is_empty());
                w.set_uninstall_error(error.into());
                w.set_backup_path(backup.into());
                w.set_uninstall_done(true);
            }
        });
    });
}

pub fn fail(ui: &Weak<UninstallerWindow>, error: String) {
    log_line(ui, format!("ERROR: {error}"));

    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = ui.upgrade() {
            w.set_uninstall_success(false);
            w.set_uninstall_error(error.into());
            w.set_uninstall_done(true);
        }
    });
}

fn run_inner(
    ui: &Weak<UninstallerWindow>,
    plan: &Plan,
    app_dir: &Path,
    options: Options,
) -> Result<Option<String>, String> {
    log_line(ui, "Preparing uninstallation...".into());

    if aurora_is_running() {
        return Err("Aurora is still running. Please close it and try again.".into());
    }

    let mods_dir = options
        .delete_mods
        .then_some(plan.mods_dir.as_deref())
        .flatten();
    let backup_wanted = options.preserve_mods && mods_dir.is_some();

    let steps = 1
        + usize::from(backup_wanted)
        + usize::from(mods_dir.is_some())
        + usize::from(options.delete_config);
    let mut step = 0usize;

    let mut backup: Option<String> = None;
    if let Some(mods) = mods_dir {
        if backup_wanted {
            let archive = backup_path(&plan.backup_dir);
            log_line(ui, format!("Archiving mods to {}...", archive.display()));
            archive_mods(ui, mods, &archive, ratio(step, steps), ratio(1, steps))?;
            backup = Some(archive.to_string_lossy().into_owned());
            advance(ui, &mut step, steps);
        }

        log_line(ui, format!("Deleting {}...", mods.display()));
        crate::plan::verify_mods_dir(mods)?;
        remove_dir(mods)?;
        advance(ui, &mut step, steps);
    }

    if options.delete_config {
        log_line(ui, format!("Deleting {}...", plan.data_dir.display()));
        crate::plan::verify_data_dir(&plan.data_dir)?;
        remove_dir(&plan.data_dir)?;
        advance(ui, &mut step, steps);
    }

    log_line(ui, "Removing shortcuts...".into());
    shared::desktop_entry::uninstall();
    if let Err(e) = shared::desktop_entry::remove_desktop_shortcut() {
        log_line(
            ui,
            format!("WARNING: could not remove the desktop shortcut: {e}"),
        );
    }
    if let Err(e) = unregister_uninstall_entry() {
        log_line(
            ui,
            format!("WARNING: could not remove the Add or Remove Programs entry: {e}"),
        );
    }

    log_line(ui, format!("Removing Aurora from {}...", app_dir.display()));
    if remove_app_dir(ui, app_dir)?
        && let Ok(exe) = std::env::current_exe()
    {
        *PENDING_CLEANUP.lock().unwrap() = Some(exe);
        log_line(
            ui,
            "The uninstaller itself is removed once this window closes.".into(),
        );
    }
    advance(ui, &mut step, steps);

    set_progress(ui, 1.0);
    log_line(ui, "Uninstallation complete.".into());
    Ok(backup)
}

fn archive_mods(
    ui: &Weak<UninstallerWindow>,
    mods: &Path,
    archive: &Path,
    base: f32,
    span: f32,
) -> Result<(), String> {
    let mut last_progress = Instant::now();
    let mut current = PathBuf::new();

    shared::archive::zip_directory(mods, archive, |rel, done, total| {
        if rel != current {
            current = rel.to_path_buf();
            log_line(
                ui,
                format!(
                    "  {} of {}: {}",
                    format_bytes(done),
                    format_bytes(total),
                    rel.display()
                ),
            );
        }

        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            last_progress = Instant::now();
            #[allow(clippy::cast_precision_loss)]
            let fraction = if total > 0 {
                done as f32 / total as f32
            } else {
                0.0
            };
            set_progress(ui, base + span * fraction.clamp(0.0, 1.0));
        }
    })
    .map_err(|e| format!("failed to archive {}: {e}", mods.display()))
}

fn advance(ui: &Weak<UninstallerWindow>, step: &mut usize, steps: usize) {
    *step += 1;
    set_progress(ui, ratio(*step, steps));
}

fn remove_app_dir(ui: &Weak<UninstallerWindow>, dir: &Path) -> Result<bool, String> {
    if !dir.exists() {
        return Ok(false);
    }
    crate::plan::verify_app_dir(dir)?;

    let owned = owned::resolve(dir);
    if !owned.from_manifest {
        log_line(
            ui,
            format!(
                "WARNING: no install manifest in {}, so only Aurora's standard files are removed.",
                dir.display()
            ),
        );
    }

    let self_exe = std::env::current_exe()
        .unwrap_or_default()
        .canonicalize()
        .ok();
    let mut skipped = false;

    for tree in &owned.trees {
        log_line(ui, format!("  Deleting {}...", tree.display()));
        remove_path(tree)?;
    }

    for file in &owned.files {
        if !file.exists() {continue}
        if self_exe.is_some() && self_exe == file.canonicalize().ok() {
            skipped = true;
            continue;
        }
        remove_path(file)?;
    }

    for empty in &owned.prune {
        let _ = fs::remove_dir(empty);
    }

    for kept in &owned.foreign {
        log_line(
            ui,
            format!("  Keeping {} - Aurora did not install it.", kept.display()),
        );
    }

    if fs::remove_dir(dir).is_ok() {
        return Ok(false);
    }

    if !skipped {
        log_line(
            ui,
            format!(
                "Left {} in place: it still holds files that are not Aurora's.",
                dir.display()
            ),
        );
    }

    Ok(skipped)
}

fn remove_path(path: &Path) -> Result<(), String> {
    let result = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|e| format!("failed to delete {}: {e}", path.display()))
}

fn remove_dir(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    fs::remove_dir_all(dir).map_err(|e| format!("failed to delete {}: {e}", dir.display()))
}

fn backup_path(base: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    base.join(format!("AuroraMods-{stamp}.zip"))
}

fn unregister_uninstall_entry() -> Result<(), String> {
    crate::registry::delete_entry()
}

fn aurora_is_running() -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );

    let target = ipc::AURORA_EXE.to_lowercase();
    system.processes().values().any(|process| {
        process
            .exe()
            .and_then(Path::file_name)
            .is_some_and(|name| name.to_string_lossy().to_lowercase() == target)
    })
}

pub fn take_pending_cleanup() -> Option<PathBuf> {
    PENDING_CLEANUP.lock().unwrap().take()
}

pub fn cleanup_target() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|arg| arg == CLEANUP_ARG)
        .and_then(|index| args.get(index + 1))
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn run_cleanup(target: &Path) {
    if target.is_dir() {
        eprintln!(
            "Refusing to clean up {}: expected a file, not a folder",
            target.display()
        );
        return;
    }

    let deadline = Instant::now() + CLEANUP_TIMEOUT;

    loop {
        match fs::remove_file(target) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) if Instant::now() >= deadline => {
                eprintln!("Gave up deleting {}: {e}", target.display());
                break;
            }
            Err(_) => std::thread::sleep(CLEANUP_RETRY_INTERVAL),
        }
    }

    if let Some(parent) = target.parent() {
        let _ = fs::remove_dir(parent);
    }

    delete_self_on_reboot();
}

fn delete_self_on_reboot() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_DELAY_UNTIL_REBOOT, MoveFileExW};

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let wide: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    if unsafe { MoveFileExW(wide.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) } == 0 {
        eprintln!(
            "Could not schedule {} for deletion on reboot",
            exe.display()
        );
    }
}

pub fn cleanup_after_exit(target: &Path) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let schedule = || -> std::io::Result<()> {
        let exe = std::env::current_exe()?;
        let helper = std::env::temp_dir().join(format!(
            "aurora-cleanup-{}-{}.exe",
            std::process::id(),
            chrono::Local::now().format("%H%M%S")
        ));
        fs::copy(&exe, &helper)?;

        Command::new(&helper)
            .arg(CLEANUP_ARG)
            .arg(target)
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()?;
        Ok(())
    };

    if let Err(e) = schedule() {
        eprintln!("Failed to schedule cleanup of {}: {e}", target.display());
    }
}

#[allow(clippy::cast_precision_loss)]
fn ratio(done: usize, total: usize) -> f32 {
    if total == 0 {
        return 1.0;
    }
    (done as f32 / total as f32).clamp(0.0, 1.0)
}

fn log_line(ui: &Weak<UninstallerWindow>, message: String) {
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = ui.upgrade() {
            let log = w.get_uninstall_log();
            if let Some(model) = log.as_any().downcast_ref::<VecModel<SharedString>>() {
                model.push(message.into());
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                w.set_uninstall_log_count(model.row_count() as i32);
            }
        }
    });
}

fn set_progress(ui: &Weak<UninstallerWindow>, progress: f32) {
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = ui.upgrade() {
            w.set_uninstall_progress(progress);
        }
    });
}
