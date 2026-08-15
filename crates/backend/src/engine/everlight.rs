use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use log::*;
use shared::classes::info::{patcher, version::Version};

use crate::handler::EngineEvent;

const POLL_INTERVAL: Duration = Duration::from_millis(500);

const CHKSUM_MARKER: &str = "CHKSUM";

pub(super) fn watch(
    win64: &Path,
    game_path: &Path,
    version: Version,
    evt_tx: &mpsc::Sender<EngineEvent>,
    stop: &AtomicBool,
) {
    thread::scope(|scope| {
        let chksum_tx = evt_tx.clone();
        scope.spawn(move || watch_checksum(win64, game_path, version, &chksum_tx, stop));
    });
}

fn watch_checksum(
    win64: &Path,
    game_path: &Path,
    version: Version,
    evt_tx: &mpsc::Sender<EngineEvent>,
    stop: &AtomicBool,
) {
    if checksum_ignored() {
        info!("'Ignore Checksum Matching' is enabled, not watching for the CHKSUM marker");
        return;
    }

    let marker = win64.join(CHKSUM_MARKER);
    let mut warned = false;

    loop {
        let stop_requested = stop.load(Ordering::Relaxed);

        if marker.is_file() {
            match fs::remove_file(&marker) {
                Ok(()) => info!("Removed CHKSUM marker: {}", marker.display()),
                Err(e) => warn!("Failed to remove CHKSUM marker {}: {e}", marker.display()),
            }

            if !warned {
                warned = true;
                let res_version = patcher::res_version(game_path, version);
                error!(
                    "Game version mismatch on ResVersion {}, CHKSUM marker found in {}",
                    res_version.as_deref().unwrap_or("<unknown>"),
                    win64.display()
                );
                let text = res_version.as_deref().map_or_else(
                    || "Game version mismatch, wait for Aurora to update".to_string(),
                    |res_version| {
                        format!(
                            "Game version mismatch (game is on {res_version}), wait for Aurora to update"
                        )
                    },
                );
                evt_tx
                    .send(EngineEvent::Toast {
                        text,
                        kind: "error".to_string(),
                    })
                    .ok();
            }
        }

        if stop_requested {
            trace!("CHKSUM watcher stopped, game exited");
            return;
        }

        thread::sleep(POLL_INTERVAL);
    }
}

pub(super) fn checksum_ignored() -> bool {
    shared::config::get(shared::config::key::IGNORE_CHECKSUM)
        .as_bool()
        .unwrap_or(false)
}
