use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use log::*;

use crate::handler::EngineEvent;

use super::process::{kill_process, ProcessSnapshot};

const LOG_NAME_MARKER: &str = "everlight_engine";
const LOG_EXTENSION: &str = "tmp";
// "YYYY-MM-DD HH:MM:SS"
const TIMESTAMP_LEN: usize = 19;

const LOG_WAIT_TIMEOUT: Duration = Duration::from_secs(45);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EverlightCode {
    /// E100: Fatal error, the engine locked up for this session.
    Fatal,
    /// E150: Non-fatal warning.
    Warning,
    /// E200: Targets patched, mods are loaded in-game.
    Loaded,
    /// E300: Invalid target found, not patched due to security protocols.
    InvalidTarget,
    /// E350: Runtime unobfuscation failed, XOR still continuing.
    XorFailure,
    /// E400: No valid targets found, no mods are loaded in-game.
    NoTargets,
}

impl EverlightCode {
    fn parse(code: &str) -> Option<Self> {
        match code {
            "E100" => Some(Self::Fatal),
            "E150" => Some(Self::Warning),
            "E200" => Some(Self::Loaded),
            "E300" => Some(Self::InvalidTarget),
            "E350" => Some(Self::XorFailure),
            "E400" => Some(Self::NoTargets),
            _ => None,
        }
    }
}

fn parse_line(line: &str) -> Option<(EverlightCode, String)> {
    // {CODE} | {TIMESTAMP} {MESSAGE}
    let (code, rest) = line.split_once(" | ")?;
    let code = EverlightCode::parse(code.trim())?;
    let message = rest.get(TIMESTAMP_LEN..).unwrap_or(rest).trim().to_string();
    Some((code, message))
}

fn is_everlight_log(path: &Path) -> bool {
    let matches_ext = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(LOG_EXTENSION));
    let matches_name = path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(LOG_NAME_MARKER));
    matches_ext && matches_name
}

pub(super) fn find_logs(win64: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(win64) else {
        return vec![];
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_everlight_log(p))
        .collect()
}

fn find_latest_log(win64: &Path) -> Option<PathBuf> {
    find_logs(win64).into_iter().max_by_key(|p| {
        fs::metadata(p)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    })
}

fn kill_game(game_process: &str) {
    let snapshot = ProcessSnapshot::refresh();
    let matches = snapshot.matching(game_process);
    if matches.is_empty() {
        warn!("Wanted to close {game_process}, but it is not running");
        return;
    }
    for (pid, process) in matches {
        kill_process(*pid, process);
    }
}

pub(super) fn watch(
    win64: &Path,
    game_process: &'static str,
    evt_tx: &mpsc::Sender<EngineEvent>,
    stop: &AtomicBool,
) {
    let deadline = Instant::now() + LOG_WAIT_TIMEOUT;
    let log_path = loop {
        if stop.load(Ordering::Relaxed) {
            trace!("Everlight watcher stopped before a log file appeared");
            return;
        }

        if let Some(path) = find_latest_log(win64) {
            info!("Found Everlight log: {}", path.display());
            break path;
        }

        if Instant::now() >= deadline {
            error!(
                "No Everlight log appeared in {} within {}s, closing {game_process}",
                win64.display(),
                LOG_WAIT_TIMEOUT.as_secs()
            );

            kill_game(game_process);
            evt_tx.send(EngineEvent::EverlightTimeout).ok();

            return;
        }
        thread::sleep(POLL_INTERVAL);
    };

    tail_log(&log_path, game_process, evt_tx, stop);
}

fn tail_log(
    log_path: &Path,
    game_process: &'static str,
    evt_tx: &mpsc::Sender<EngineEvent>,
    stop: &AtomicBool,
) {
    let mut offset = 0usize;
    let mut pending: Vec<u8> = Vec::new();

    loop {
        let stop_requested = stop.load(Ordering::Relaxed);

        match fs::read(log_path) {
            Ok(data) => {
                if data.len() < offset {
                    warn!("Everlight log shrank, re-reading from the start");
                    offset = 0;
                    pending.clear();
                }

                if data.len() > offset {
                    pending.extend_from_slice(&data[offset..]);
                    offset = data.len();
                }
            }
            Err(e) => warn!("Could not read Everlight log: {e}"),
        }

        while let Some(idx) = pending.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = pending.drain(..=idx).collect();
            let line = String::from_utf8_lossy(&raw);
            if handle_line(line.trim(), game_process, evt_tx) {
                return;
            }
        }

        if stop_requested {
            // The game exited; flush a possible final line without a newline.
            let line = String::from_utf8_lossy(&pending);
            let line = line.trim();
            if !line.is_empty() {
                handle_line(line, game_process, evt_tx);
            }

            info!("Everlight watcher stopped, game exited");
            return;
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn handle_line(line: &str, game_process: &'static str, evt_tx: &mpsc::Sender<EngineEvent>) -> bool {
    if line.is_empty() {
        return false;
    }

    let Some((code, message)) = parse_line(line) else {
        warn!("Ignoring unrecognized Everlight log line: {line}");
        return false;
    };

    let toast = |text: String, kind: &str| {
        evt_tx
            .send(EngineEvent::Toast {
                text,
                kind: kind.to_string(),
            })
            .ok();
    };

    match code {
        EverlightCode::Fatal => {
            error!("Everlight fatal error, closing {game_process}: {message}");
            kill_game(game_process);
            evt_tx.send(EngineEvent::EverlightFatal(message)).ok();
            return true;
        }
        EverlightCode::Warning => {
            warn!("Everlight warning: {message}");
            toast(format!("Everlight warning: {message}"), "error");
        }
        EverlightCode::Loaded => {
            info!("Everlight loaded successfully: {message}");
            toast(
                "Everlight loaded successfully, mods are active in-game!".to_string(),
                "success",
            );
        }
        EverlightCode::InvalidTarget => {
            warn!("Everlight found an invalid target: {message}");
            toast(
                "Everlight found an invalid target and did not patch it.".to_string(),
                "error",
            );
        }
        EverlightCode::XorFailure => {
            warn!("Everlight failed to unobfuscate at runtime: {message}");
            toast(
                "Everlight failed to unobfuscate at runtime, mods might misbehave.".to_string(),
                "error",
            );
        }
        EverlightCode::NoTargets => {
            warn!("Everlight found no valid targets: {message}");
            toast(
                "Everlight found no valid targets, no mods are loaded in-game.".to_string(),
                "error",
            );
        }
    }
    false
}
