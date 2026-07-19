use std::fs::OpenOptions;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use log::*;
use sysinfo::{Pid, Process, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::engine::files::FileGroup;

use super::AuroraEngine;

fn matches_process(exe: &Path, target_lower: &str) -> bool {
    exe.file_name()
        .is_some_and(|f| f.to_string_lossy().to_lowercase() == target_lower)
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
            ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
        );
    }

    pub fn matching<'a>(&'a self, target: &str) -> Vec<(&'a Pid, &'a Process)> {
        let target_lower = target.to_lowercase();
        self.0
            .processes()
            .iter()
            .filter(|(_, p)| p.exe().is_some_and(|e| matches_process(e, &target_lower)))
            .collect()
    }

    pub fn any_matching(&self, targets: &[&str]) -> bool {
        targets.iter().any(|t| !self.matching(t).is_empty())
    }
}

impl AuroraEngine {
    pub fn kill_nte_processes(&self) -> Result<()> {
        let snapshot = ProcessSnapshot::refresh();

        let mut names = vec![self.gpaths.launcher_process, self.gpaths.game_process];
        names.extend(self.gpaths.helper_processes.iter().copied());
        trace!("Processes to kill: {}", names.join(", "));

        for name in names {
            for (pid, process) in snapshot.matching(name) {
                let exe = process
                    .exe()
                    .map(|e| e.display().to_string())
                    .unwrap_or_default();
                trace!("Killing process {exe} (pid {pid})");
                if process.kill() {
                    info!("Process {exe} killed");
                } else {
                    error!("Process {exe} could not be killed");
                }
            }
        }

        for file in self
            .managed_files()
            .into_iter()
            .filter(|f| f.group == FileGroup::LoaderDll)
        {
            if !file.destination.exists() {
                continue;
            }
            trace!("Checking {}", file.destination.display());

            let mut still_locked = false;
            for attempt in 0..5 {
                match OpenOptions::new().write(true).open(&file.destination) {
                    Ok(_) => {
                        trace!("{} is not locked", file.label);
                        still_locked = false;
                        break;
                    }
                    Err(e) => {
                        trace!("{} is locked: {e}", file.label);
                        still_locked = true;
                        if attempt < 4 {
                            warn!(
                                "{} is still locked, Aurora Engine is waiting...",
                                file.label
                            );
                            std::thread::sleep(Duration::from_millis(300));
                        }
                    }
                }
            }
            if still_locked {
                warn!(
                    "{} is still locked after 5 attempts, proceeding anyway",
                    file.label
                );
            }
        }

        Ok(())
    }
}
