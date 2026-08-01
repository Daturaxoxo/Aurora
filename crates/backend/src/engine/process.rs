use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use anyhow::Result;
use log::*;
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use super::AuroraEngine;

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

    process
        .cmd()
        .iter()
        .any(|arg| arg.to_string_lossy().contains(target_lower))
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
}

#[derive(Clone)]
pub struct KillSnapshot {
    pub launcher_process: &'static str,
    pub game_process: &'static str,
    pub helper_processes: Vec<&'static str>,
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
            .map_err(|e| anyhow::anyhow!("Kill snapshot poisoned: {e}"))?;
        guard
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Kill snapshot not initialized yet"))?
    };

    let snapshot_data = ProcessSnapshot::refresh();

    let mut names = vec![snapshot.launcher_process, snapshot.game_process];
    names.extend(snapshot.helper_processes.iter().copied());
    trace!("Processes to kill: {}", names.join(", "));

    let mut kill_failures: Vec<String> = Vec::new();

    for name in names {
        for (pid, process) in snapshot_data.matching(name) {
            let exe = process
                .exe()
                .map(|e| e.display().to_string())
                .unwrap_or_default();
            trace!("Killing process {exe} (pid {pid})");

            if process.kill() {
                info!("Process {exe} killed");
                continue;
            }

            #[cfg(target_os = "windows")]
            {
                let pid_u32 = pid.as_u32();
                trace!("sysinfo kill failed for {exe}, retrying via taskkill /F /PID {pid_u32}");
                match std::process::Command::new("taskkill")
                    .args(["/F", "/PID", &pid_u32.to_string()])
                    .output()
                {
                    Ok(output) if output.status.success() => {
                        info!("{exe} killed via taskkill");
                        continue;
                    }
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        error!(
                            "{exe} could not be killed (taskkill exit {}: {stderr}). \
                             Ensure Aurora Engine is running as Administrator.",
                            output.status
                        );
                    }
                    Err(e) => {
                        error!(
                            "{exe} could not be killed (taskkill unavailable: {e}). \
                             Ensure Aurora Engine is running as Administrator."
                        );
                    }
                }
            }

            #[cfg(not(target_os = "windows"))]
            error!("{exe} could not be killed");

            kill_failures.push(exe);
        }
    }

    if !kill_failures.is_empty() {
        return Err(anyhow::anyhow!(
            "Failed to kill {} process(es): {}. \
             On Windows, ensure Aurora Engine is running as Administrator.",
            kill_failures.len(),
            kill_failures.join(", ")
        ));
    }

    for (label, destination) in &snapshot.loader_dlls {
        if !destination.exists() {
            continue;
        }
        trace!("Checking {}", destination.display());

        let mut still_locked = false;
        for attempt in 0..5 {
            match OpenOptions::new().write(true).open(destination) {
                Ok(_) => {
                    trace!("{label} is not locked");
                    still_locked = false;
                    break;
                }
                Err(e) => {
                    trace!("{label} is locked: {e}");
                    still_locked = true;
                    if attempt < 4 {
                        warn!("{label} is still locked, Aurora Engine is waiting...");
                        std::thread::sleep(Duration::from_millis(300));
                    }
                }
            }
        }
        if still_locked {
            warn!("{label} is still locked after 5 attempts, proceeding anyway");
        }
    }

    Ok(())
}

impl AuroraEngine {
    pub fn kill_nte_processes(&self) -> Result<()> {
        kill_nte_processes_standalone()
    }
}
