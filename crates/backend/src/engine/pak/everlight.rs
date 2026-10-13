use crate::engine::contract::{EventSink, NotificationKind};
use anyhow::{Context, Result};
use log::*;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const CHKSUM_MARKER: &str = "CHKSUM";
pub(super) const SIGNATURE_FILE_NAME: &str = "everlight.sig";

pub(super) fn install_signature(win64: &Path, signature: &[u8]) -> Result<()> {
    let destination = win64.join(SIGNATURE_FILE_NAME);
    fs::write(&destination, signature)
        .with_context(|| format!("Failed to write {}", destination.display()))?;
    trace!("Wrote {}", destination.display());
    Ok(())
}

pub(super) fn watch(
    win64: &Path,
    content_version: &(dyn Fn() -> Option<String> + Sync),
    ignore_checksum: bool,
    events: &EventSink,
    stop: &AtomicBool,
) {
    thread::scope(|scope| {
        scope.spawn(move || {
            watch_checksum(win64, content_version, ignore_checksum, events, stop);
        });
    });
}

fn watch_checksum(
    win64: &Path,
    content_version: &(dyn Fn() -> Option<String> + Sync),
    ignore_checksum: bool,
    events: &EventSink,
    stop: &AtomicBool,
) {
    if ignore_checksum {
        info!("'Ignore Checksum Match is enabled, not watching for the CHKSUM marker");
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
                let res_version = content_version();
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
                events.notify(NotificationKind::Error, text);
            }
        }

        if stop_requested {
            trace!("CHKSUM watcher stopped, game exited");
            return;
        }

        thread::sleep(POLL_INTERVAL);
    }
}
