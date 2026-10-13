use super::everlight;
use super::process::{ProcessSnapshot, ProcessTargets, kill_processes, kill_targets};
use crate::classes::rpc::RPC;
use crate::engine::contract::{EventSink, SessionExit};
use anyhow::{Result, anyhow};
use log::*;
use shared::classes::games::{InstallationFacts, pak::PakSupport};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

// This is intentionally high, because when the launcher updates and restarts it takes a while.
const LAUNCHER_GRACE_SECS: u32 = 10;

// TODO: Probably want to consider decreasing these at some point
// Is it worth decreasing tho? -Datura
// No it's not, i had to increase it - wapr
pub(super) const POST_EXIT_KILL_GRACE: Duration = Duration::from_secs(7);
const THREAD_SLEEP_DURATION: Duration = Duration::from_millis(500);
const POST_EXIT_POLL: Duration = Duration::from_millis(150);

pub(super) struct Monitor {
    pub targets: ProcessTargets,
    pub pak: &'static dyn PakSupport,
    pub installation: InstallationFacts,
    pub ignore_checksum: bool,
    pub events: EventSink,
    pub stop: Arc<AtomicBool>,
}

impl Monitor {
    pub fn run(&self) -> Result<SessionExit> {
        info!(
            "Helper processes: {}",
            self.targets.helper_processes.join(", ")
        );
        info!(
            "Monitoring for {}, you must press \"Play\" in the launcher!",
            self.targets.game_name
        );

        match self.wait_for_launcher(LAUNCHER_GRACE_SECS) {
            Some(SessionExit::GameExited) => {}
            Some(exit) => return Ok(self.finish(exit)),
            None => return Ok(SessionExit::Stopped),
        }

        let watcher_stop = Arc::new(AtomicBool::new(false));
        let watcher = {
            let win64 = self.targets.win64.clone();
            let pak = self.pak;
            let installation = self.installation.clone();
            let events = self.events.clone();
            let ignore_checksum = self.ignore_checksum;
            let stop = watcher_stop.clone();
            thread::spawn(move || {
                let content_version = || pak.content_version(&installation);
                everlight::watch(&win64, &content_version, ignore_checksum, &events, &stop);
            })
        };

        let exited = self.wait_for_game_exit();
        watcher_stop.store(true, Ordering::Relaxed);
        if watcher.join().is_err() {
            error!("Everlight watcher thread panicked");
        }

        if !exited? {
            return Ok(SessionExit::Stopped);
        }
        info!(
            "{} was closed, initializing clean-up process...",
            self.targets.game_name
        );
        Ok(self.finish(SessionExit::GameExited))
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn finish(&self, exit: SessionExit) -> SessionExit {
        if let Err(e) = ensure_processes_gone(&self.targets, POST_EXIT_KILL_GRACE) {
            warn!(
                "Could not confirm every {} process exited: {e}",
                self.targets.game_name
            );
        }
        exit
    }

    fn wait_for_launcher(&self, grace_secs: u32) -> Option<SessionExit> {
        let launcher_process = self.targets.launcher_process;
        let game_process = self.targets.game_process;
        let mut watch_names = vec![launcher_process];
        watch_names.extend(self.targets.helper_processes.iter().copied());
        #[cfg(not(target_os = "windows"))]
        {
            watch_names.push("wineserver");
        }

        let mut snapshot = ProcessSnapshot::refresh();
        let mut launcher_seen = false;
        let mut missing_ticks = 0u32;
        let grace_ticks = u32::try_from(
            (Duration::from_secs(u64::from(grace_secs)).as_millis()
                / THREAD_SLEEP_DURATION.as_millis())
            .max(1),
        )
        .unwrap_or(1);

        loop {
            thread::sleep(THREAD_SLEEP_DURATION);
            if self.stopped() {
                return None;
            }
            snapshot.rerefresh();

            if !snapshot.matching(game_process).is_empty() {
                info!(
                    "{} process ({game_process}) was detected, game is running.",
                    self.targets.game_name
                );
                if let Err(e) = RPC.set_ingame() {
                    warn!("Failed to update Discord RPC: {e}");
                }
                return Some(SessionExit::GameExited);
            }

            if snapshot.any_matching(&watch_names) {
                missing_ticks = 0;
                if !launcher_seen {
                    info!("{} Launcher activity detected.", self.targets.game_name);
                    launcher_seen = true;
                    if let Err(e) = RPC.set_launching() {
                        warn!("Failed to update Discord RPC: {e}");
                    }
                }
                continue;
            }

            if !launcher_seen {
                continue;
            }

            missing_ticks += 1;
            if missing_ticks == 1 {
                warn!("{} Launcher process not detected", self.targets.game_name);
            }
            if missing_ticks >= grace_ticks {
                warn!(
                    "{} Launcher failed to resolve within {grace_secs}s of continuous absence. Aborting monitor.",
                    self.targets.game_name
                );
                return Some(SessionExit::LauncherAbandoned);
            }
        }
    }

    fn wait_for_game_exit(&self) -> Result<bool> {
        let game_process = self.targets.game_process;
        let mut snapshot = ProcessSnapshot::refresh();

        while !snapshot.matching(game_process).is_empty() {
            thread::sleep(THREAD_SLEEP_DURATION);
            if self.stopped() {
                return Ok(false);
            }
            snapshot.rerefresh();
        }

        if let Err(e) = RPC.set_idle() {
            warn!("Failed to update Discord RPC: {e}");
            return Err(anyhow!(e));
        }

        Ok(true)
    }
}

pub(super) fn ensure_processes_gone(targets: &ProcessTargets, grace: Duration) -> Result<()> {
    let deadline = Instant::now() + grace;
    let mut snapshot = ProcessSnapshot::refresh();

    while Instant::now() < deadline {
        snapshot.rerefresh();
        if !snapshot.any_matching(targets.processes) && !snapshot.any_in_dir(&targets.win64) {
            return Ok(());
        }

        let launchers: Vec<_> = std::iter::once(&targets.launcher_process)
            .chain(&targets.helper_processes)
            .flat_map(|name| snapshot.matching(name))
            .map(|(pid, process)| (*pid, process))
            .collect();
        if !launchers.is_empty() {
            info!(
                "{} Launcher reopened after exit, closing it",
                targets.game_name
            );
            if let Err(e) = kill_processes(launchers) {
                warn!(
                    "Failed to close the reopened {} Launcher: {e}",
                    targets.game_name
                );
            }
            continue;
        }

        thread::sleep(POST_EXIT_POLL);
    }

    warn!("Processes did not close within {grace:?}. Force killing...");
    kill_targets(targets)
        .map_err(|e| anyhow!("Failed to kill {} processes: {e}", targets.game_name))
}
