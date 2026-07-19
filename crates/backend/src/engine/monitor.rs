use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use shared::classes::info::NTE_PROCESSES;

use crate::classes::rpc::RPC;

use super::process::ProcessSnapshot;
use super::AuroraEngine;

// This is intentionally high, because when the launcher updates and restarts it takes a while.
const LAUNCHER_GRACE_SECS: u32 = 10;

// TODO: Probably want to consider decreasing these at some point
const POST_EXIT_KILL_GRACE: Duration = Duration::from_secs(5);
const THREAD_SLEEP_DURATION: Duration = Duration::from_millis(500);

impl AuroraEngine {
    pub fn monitor(&mut self) -> Result<()> {
        info!(
            "Helper processes: {}",
            self.gpaths.helper_processes.join(", ")
        );
        info!("Monitoring for NTE, you must press \"Play\" in the launcher!");

        if !self.wait_for_launcher(LAUNCHER_GRACE_SECS)? {
            return Ok(());
        }

        self.wait_for_game_exit()?;

        info!("NTE was closed, initializing clean-up process...");
        self.sanitize(true)?;

        self.ensure_processes_gone(POST_EXIT_KILL_GRACE)
    }

    fn wait_for_launcher(&self, grace_secs: u32) -> Result<bool> {
        let launcher_process = self.gpaths.launcher_process;
        let game_process = self.gpaths.game_process;
        let mut watch_names = vec![launcher_process];
        watch_names.extend(self.gpaths.helper_processes.iter().copied());

        let mut snapshot = ProcessSnapshot::refresh();
        let mut launcher_seen = false;
        let mut missing_ticks = 0u32;

        loop {
            thread::sleep(THREAD_SLEEP_DURATION);
            snapshot.rerefresh();

            if !snapshot.matching(game_process).is_empty() {
                info!("NTE process ({game_process}) was detected, game is running.");
                // TODO:
                error!("UNIMPLEMENTED: on game started");
                RPC.set_ingame()?;
                return Ok(true);
            }

            if snapshot.any_matching(&watch_names) {
                missing_ticks = 0;
                if !launcher_seen {
                    info!("NTE Launcher activity detected.");
                    launcher_seen = true;
                    // TODO:
                    error!("UNIMPLEMENTED: on launcher detected");
                    RPC.set_launching()?;
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
            if missing_ticks >= grace_secs {
                warn!(
                    "NTE Launcher failed to resolve within {grace_secs}s of continuous absence. Aborting monitor."
                );
                self.sanitize(true)?;
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

        RPC.set_idle()
    }

    fn ensure_processes_gone(&self, grace: Duration) -> Result<()> {
        let deadline = Instant::now() + grace;
        let mut snapshot = ProcessSnapshot::refresh();

        while Instant::now() < deadline {
            snapshot.rerefresh();
            if !snapshot.any_matching(NTE_PROCESSES) {
                return Ok(());
            }
            thread::sleep(THREAD_SLEEP_DURATION);
        }

        warn!("Processes did not close within {grace:?}. Force killing...");
        self.kill_nte_processes()
            .map_err(|e| anyhow!("Failed to kill NTE processes: {e}"))
    }
}
