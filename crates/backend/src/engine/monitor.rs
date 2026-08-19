use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use log::*;
use shared::classes::info::NTE_PROCESSES;

use crate::classes::rpc::RPC;
use crate::handler::EngineEvent;

use super::everlight;
use super::process::ProcessSnapshot;
use super::AuroraEngine;

// This is intentionally high, because when the launcher updates and restarts it takes a while.
const LAUNCHER_GRACE_SECS: u32 = 10;

// TODO: Probably want to consider decreasing these at some point
// Is it worth decreasing tho? -Datura
// No it's not, i had to increase it - wapr
const POST_EXIT_KILL_GRACE: Duration = Duration::from_secs(7);
const THREAD_SLEEP_DURATION: Duration = Duration::from_millis(500);

impl AuroraEngine {
    pub fn monitor(&mut self, evt_tx: mpsc::Sender<EngineEvent>) -> Result<()> {
        info!(
            "Helper processes: {}",
            self.gpaths.helper_processes.join(", ")
        );
        info!("Monitoring for NTE, you must press \"Play\" in the launcher!");

        if !self.wait_for_launcher(LAUNCHER_GRACE_SECS)? {
            return Ok(());
        }

        let watcher_stop = Arc::new(AtomicBool::new(false));
        let watcher = {
            let win64 = self.win64.clone();
            let game_path = self.game_path.clone();
            let version = self.version;
            let stop = watcher_stop.clone();
            thread::spawn(move || {
                everlight::watch(&win64, &game_path, version, &evt_tx, &stop);
            })
        };

        let exit_result = self.wait_for_game_exit();
        watcher_stop.store(true, Ordering::Relaxed);
        if watcher.join().is_err() {
            error!("Everlight watcher thread panicked");
        }
        exit_result?;

        info!("NTE was closed, initializing clean-up process...");
        self.cleanup()
    }

    /// Waits for NTE to be gone, then removes the files it was using.
    pub(super) fn cleanup(&self) -> Result<()> {
        if let Err(e) = self.ensure_processes_gone(POST_EXIT_KILL_GRACE) {
            warn!("Could not confirm every NTE process exited: {e}");
        }

        self.sanitize(false)
    }

    fn wait_for_launcher(&self, grace_secs: u32) -> Result<bool> {
        let launcher_process = self.gpaths.launcher_process;
        let game_process = self.gpaths.game_process;
        let mut watch_names = vec![launcher_process];
        watch_names.extend(self.gpaths.helper_processes.iter().copied());
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
            snapshot.rerefresh();

            if !snapshot.matching(game_process).is_empty() {
                info!("NTE process ({game_process}) was detected, game is running.");
                if let Err(e) = RPC.set_ingame() {
                    warn!("Failed to update Discord RPC: {e}");
                }
                return Ok(true);
            }

            if snapshot.any_matching(&watch_names) {
                missing_ticks = 0;
                if !launcher_seen {
                    info!("NTE Launcher activity detected.");
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
                warn!("NTE Launcher process not detected");
            }
            if missing_ticks >= grace_ticks {
                warn!(
                    "NTE Launcher failed to resolve within {grace_secs}s of continuous absence. Aborting monitor."
                );
                self.cleanup()?;
                return Ok(false);
            }
        }
    }

    fn wait_for_game_exit(&self) -> Result<()> {
        let game_process = self.gpaths.game_process;
        let mut snapshot = ProcessSnapshot::refresh();

        while !snapshot.matching(game_process).is_empty() {
            thread::sleep(THREAD_SLEEP_DURATION);
            snapshot.rerefresh();
        }

        if let Err(e) = RPC.set_idle() {
            warn!("Failed to update Discord RPC: {e}");
            return Err(anyhow!(e));
        }

        Ok(())
    }

    fn ensure_processes_gone(&self, grace: Duration) -> Result<()> {
        let deadline = Instant::now() + grace;
        let mut snapshot = ProcessSnapshot::refresh();

        while Instant::now() < deadline {
            snapshot.rerefresh();
            if !snapshot.any_matching(NTE_PROCESSES) && !snapshot.any_in_dir(&self.win64) {
                return Ok(());
            }
            thread::sleep(THREAD_SLEEP_DURATION);
        }

        warn!("Processes did not close within {grace:?}. Force killing...");
        self.kill_nte_processes()
            .map_err(|e| anyhow!("Failed to kill NTE processes: {e}"))
    }
}
