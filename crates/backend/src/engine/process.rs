use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use log::*;
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::AuroraEngine;
use super::locks;

/// How long to wait for a process to actually disappear after asking it to die.
const KILL_CONFIRM_TIMEOUT: Duration = Duration::from_secs(5);
const KILL_CONFIRM_POLL: Duration = Duration::from_millis(100);

const PROBE_ATTEMPTS: u32 = 5;
const PROBE_INTERVAL: Duration = Duration::from_millis(300);

fn basename(arg: &str) -> &str {
    arg.rsplit(['/', '\\']).next().unwrap_or(arg)
}

fn matches_process(process: &Process, target_lower: &str) -> bool {
    let name_matches = |s: &str| basename(s).to_lowercase() == target_lower;

    if process
        .exe()
        .and_then(Path::file_name)
        .is_some_and(|f| name_matches(&f.to_string_lossy()))
    {
        return true;
    }

    if process
        .cmd()
        .first()
        .is_some_and(|arg| name_matches(&arg.to_string_lossy()))
    {
        return true;
    }

    name_matches(&process.name().to_string_lossy())
}

#[cfg(target_os = "windows")]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    const STILL_ACTIVE: u32 = 259;

    pub fn is_alive(pid: u32) -> bool {
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);

            if handle.is_null() {
                return GetLastError() != ERROR_INVALID_PARAMETER;
            }

            let mut code: u32 = 0;
            let queried = GetExitCodeProcess(handle, &raw mut code);
            CloseHandle(handle);

            queried == 0 || code == STILL_ACTIVE
        }
    }
}

fn process_gone(pid: Pid) -> bool {
    #[cfg(target_os = "windows")]
    {
        !win::is_alive(pid.as_u32())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        system.process(pid).is_none()
    }
}

fn wait_for_exit(pid: Pid) -> bool {
    let deadline = Instant::now() + KILL_CONFIRM_TIMEOUT;

    loop {
        if process_gone(pid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(KILL_CONFIRM_POLL);
    }
}

/// True when `path` sits inside `dir`. Compared case-insensitively on Windows,
/// where the same directory reaches us in whatever casing the caller stored.
fn path_under(path: &Path, dir: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let path = path.to_string_lossy().to_lowercase().replace('/', "\\");
        let dir = dir.to_string_lossy().to_lowercase().replace('/', "\\");
        path.starts_with(&format!("{}\\", dir.trim_end_matches('\\')))
    }

    #[cfg(not(target_os = "windows"))]
    {
        path.starts_with(dir)
    }
}

pub(super) struct ProcessSnapshot(System);

impl ProcessSnapshot {
    pub fn refresh() -> Self {
        let mut system = System::new();
        Self::refresh_into(&mut system);
        Self(system)
    }

    pub fn rerefresh(&mut self) {
        Self::refresh_into(&mut self.0);
    }

    fn refresh_into(system: &mut System) {
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::Always)
                .with_cmd(UpdateKind::Always),
        );
    }

    pub fn matching<'a>(&'a self, target: &str) -> Vec<(&'a Pid, &'a Process)> {
        let target_lower = target.to_lowercase();
        self.0
            .processes()
            .iter()
            .filter(|(_, p)| matches_process(p, &target_lower))
            .collect()
    }

    pub fn any_matching(&self, targets: &[&str]) -> bool {
        targets.iter().any(|t| !self.matching(t).is_empty())
    }

    pub fn in_dir<'a>(&'a self, dir: &Path) -> Vec<(&'a Pid, &'a Process)> {
        let own_pid = std::process::id();
        self.0
            .processes()
            .iter()
            .filter(|(pid, p)| {
                pid.as_u32() != own_pid && p.exe().is_some_and(|exe| path_under(exe, dir))
            })
            .collect()
    }

    pub fn any_in_dir(&self, dir: &Path) -> bool {
        !self.in_dir(dir).is_empty()
    }
}

pub(super) fn kill_processes(processes: Vec<(Pid, &Process)>) -> Result<HashSet<Pid>> {
    let mut seen = HashSet::new();
    let mut killed = HashSet::new();
    let mut remaining = Vec::new();

    for (pid, process) in processes {
        if !seen.insert(pid) {
            continue;
        }

        let exe = process
            .exe()
            .map(|e| e.display().to_string())
            .unwrap_or_default();
        trace!("Killing process {exe} (pid {pid})");

        if process.kill() && wait_for_exit(pid) {
            info!("Process {exe} killed");
            killed.insert(pid);
        } else if process_gone(pid) {
            trace!("{exe} (pid {pid}) already exited on its own");
            killed.insert(pid);
        } else {
            remaining.push((pid, exe));
        }
    }

    #[cfg(target_os = "windows")]
    if !remaining.is_empty() {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

        let mut parameters = String::from("/F");
        for (pid, _) in &remaining {
            parameters.push_str(" /PID ");
            parameters.push_str(&pid.as_u32().to_string());
        }

        trace!(
            "Retrying {} process(es) via elevated taskkill.exe",
            remaining.len()
        );
        let operation: Vec<u16> = "runas".encode_utf16().chain(std::iter::once(0)).collect();
        let file: Vec<u16> = "taskkill.exe"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let parameters: Vec<u16> = parameters
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                operation.as_ptr(),
                file.as_ptr(),
                parameters.as_ptr(),
                std::ptr::null(),
                SW_HIDE,
            )
        } as isize;

        if result <= 32 {
            return Err(anyhow!(
                "Failed to launch elevated taskkill.exe (code {result})"
            ));
        }

        remaining.retain(|(pid, exe)| {
            if wait_for_exit(*pid) {
                info!("{exe} killed via elevated taskkill.exe");
                killed.insert(*pid);
                false
            } else {
                true
            }
        });
    }

    if !remaining.is_empty() {
        let failures: Vec<_> = remaining.into_iter().map(|(_, exe)| exe).collect();
        return Err(anyhow!(
            "Failed to kill {} process(es): {}",
            failures.len(),
            failures.join(", ")
        ));
    }

    Ok(killed)
}

#[derive(Clone)]
pub struct KillSnapshot {
    pub launcher_process: &'static str,
    pub game_process: &'static str,
    pub helper_processes: Vec<&'static str>,
    pub win64: PathBuf,
    pub loader_dlls: Vec<(String, PathBuf)>,
}

static KILL_SNAPSHOT: OnceLock<RwLock<Option<KillSnapshot>>> = OnceLock::new();

fn kill_snapshot_lock() -> &'static RwLock<Option<KillSnapshot>> {
    KILL_SNAPSHOT.get_or_init(|| RwLock::new(None))
}

pub fn set_kill_snapshot(snapshot: KillSnapshot) {
    if let Ok(mut guard) = kill_snapshot_lock().write() {
        *guard = Some(snapshot);
    }
}

pub fn kill_nte_processes_standalone() -> Result<()> {
    let snapshot = {
        let guard = kill_snapshot_lock()
            .read()
            .map_err(|e| anyhow!("Kill snapshot poisoned: {e}"))?;
        guard
            .clone()
            .ok_or_else(|| anyhow!("Kill snapshot not initialized yet"))?
    };

    let snapshot_data = ProcessSnapshot::refresh();

    let mut names = vec![snapshot.launcher_process, snapshot.game_process];
    names.extend(snapshot.helper_processes.iter().copied());
    trace!("Processes to kill: {}", names.join(", "));

    let mut to_kill = Vec::new();
    for name in names {
        for (pid, process) in snapshot_data.matching(name) {
            to_kill.push((*pid, process));
        }
    }

    for (pid, process) in snapshot_data.in_dir(&snapshot.win64) {
        to_kill.push((*pid, process));
        trace!(
            "Killing {} (runs from Win64)",
            process.name().to_string_lossy()
        );
    }

    let killed = kill_processes(to_kill)?;
    trace!("Killed {} process(es)", killed.len());

    for (label, destination) in &snapshot.loader_dlls {
        if !destination.exists() {
            continue;
        }
        trace!("Checking {}", destination.display());
        probe_lock(label, destination);
    }

    Ok(())
}

fn probe_lock(label: &str, path: &Path) {
    let mut readonly_cleared = false;
    let mut attempt = 0;

    loop {
        let Err(err) = OpenOptions::new().write(true).open(path) else {
            trace!("{label} is not locked");
            return;
        };

        match err.kind() {
            ErrorKind::NotFound => {
                trace!("{label} is already gone");
                return;
            }
            ErrorKind::PermissionDenied if !readonly_cleared && clear_readonly(path) => {
                readonly_cleared = true;
                trace!("{label} was read-only, cleared the attribute and retrying");
                continue;
            }
            _ => {}
        }

        attempt += 1;
        if attempt >= PROBE_ATTEMPTS {
            report_stuck(label, path, &err);
            return;
        }

        trace!("{label} is not writable yet ({err}), retrying");
        std::thread::sleep(PROBE_INTERVAL);
    }
}

fn report_stuck(label: &str, path: &Path, err: &std::io::Error) {
    let holders = locks::holders(path);

    if holders.is_empty() {
        warn!(
            "{label} is not writable after {PROBE_ATTEMPTS} attempts ({err}). \
             Clean-up will still try to remove it."
        );
    } else {
        warn!(
            "{label} is held by {} ({err}). Clean-up will still try to remove it.",
            holders.join(", ")
        );
    }
}

fn clear_readonly(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };

    let mut perms = metadata.permissions();
    if !perms.readonly() {
        return false;
    }

    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(path, perms).is_ok()
}

impl AuroraEngine {
    pub fn kill_nte_processes(&self) -> Result<()> {
        kill_nte_processes_standalone()
    }
}
