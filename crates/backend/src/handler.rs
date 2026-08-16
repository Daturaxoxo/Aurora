use crate::engine::AuroraEngine;
use anyhow::{anyhow, Result};
use log::*;
use shared::pathfind::get_game_directory;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock, PoisonError,
    },
    thread,
    time::Duration,
};

pub static ENGINE_CMD_TX: OnceLock<mpsc::Sender<EngineCommand>> = OnceLock::new();
pub static GAME_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn get_tx() -> Result<mpsc::Sender<EngineCommand>> {
    let tx = ENGINE_CMD_TX
        .get()
        .ok_or_else(|| anyhow!("Engine has not been started yet!"))?;
    Ok(tx.clone())
}

pub enum EngineCommand {
    Launch(Option<Vec<PathBuf>>),
    Sanitize,
    Update,
    KillProcesses,
    Validate,
}

pub enum EngineEvent {
    LaunchSuccess,
    LaunchFailed(String),
    GameClosed,
    Toast { text: String, kind: String },
    EverlightFatal(String),
    EverlightTimeout,
    GamePathUpdated(PathBuf),
    ValidationResult { missing: Vec<String> },
    EngineReady,
    EngineInitFailed(String),
}

pub struct EngineHandler {
    pub cmd_tx: mpsc::Sender<EngineCommand>,
    pub evt_rx: mpsc::Receiver<EngineEvent>,
}

const INIT_WARNINGS: &[(&str, &str)] = &[
    (
        "None of the expected launchers were found",
        "Invalid game installation: no game markers were found.",
    ),
    (
        "Game path not found",
        "Your game path couldn't be found, set it manually in the settings.",
    ),
    (
        "Aurora couldn't find the game path",
        "Your game path couldn't be found, set it manually in the settings.",
    ), // dont ask me why there are 2. -daturas
];

pub fn init_warning(msg: &str) -> Option<&'static str> {
    INIT_WARNINGS
        .iter()
        .find(|(needle, _)| msg.contains(needle))
        .map(|(_, warning)| *warning)
}

#[cfg(target_os = "windows")]
fn repair_install(evt_tx: &mpsc::Sender<EngineEvent>) {
    let report = shared::repair::restore_missing_files();

    if !report.restored.is_empty() {
        info!("Repaired {} missing file(s)", report.restored.len());
        evt_tx
            .send(EngineEvent::Toast {
                text: format!("Restored {} missing Aurora file(s).", report.restored.len()),
                kind: "success".to_string(),
            })
            .ok();
    }

    if !report.failed.is_empty() {
        error!(
            "Could not repair {} missing file(s): {}",
            report.failed.len(),
            report.failed.join(", ")
        );
        evt_tx
            .send(EngineEvent::Toast {
                text: format!(
                    "Could not restore {} missing Aurora file(s). Check your connection and antivirus.",
                    report.failed.len()
                ),
                kind: "error".to_string(),
            })
            .ok();
    }
}

impl EngineHandler {
    pub fn start() -> Result<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<EngineCommand>();
        let (evt_tx, evt_rx) = mpsc::channel::<EngineEvent>();

        let _ = ENGINE_CMD_TX.set(cmd_tx.clone());

        std::thread::spawn(move || {
            let engine: Arc<Mutex<Option<AuroraEngine>>> = Arc::new(Mutex::new(None));

            match get_game_directory() {
                Ok(game_path) => {
                    evt_tx
                        .send(EngineEvent::GamePathUpdated(game_path.clone()))
                        .ok();
                    match AuroraEngine::new(&game_path) {
                        Ok(e) => {
                            info!("Game Path: {}", game_path.display());
                            *engine.lock().unwrap_or_else(PoisonError::into_inner) = Some(e);
                            evt_tx.send(EngineEvent::EngineReady).ok();
                        }
                        Err(e) => {
                            evt_tx
                                .send(EngineEvent::EngineInitFailed(e.to_string()))
                                .ok();
                        }
                    }
                }
                Err(e) => {
                    evt_tx
                        .send(EngineEvent::EngineInitFailed(format!(
                            "Game path not found: {e}"
                        )))
                        .ok();
                }
            }

            for cmd in cmd_rx {
                let engine = engine.clone();
                let evt_tx = evt_tx.clone();
                match cmd {
                    EngineCommand::Launch(custom_files) => {
                        if GAME_RUNNING
                            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                            .is_err()
                        {
                            warn!("Launch ignored: a game session is already running");
                            evt_tx
                                .send(EngineEvent::LaunchFailed(
                                    "A game session is already running".to_string(),
                                ))
                                .ok();
                            continue;
                        }
                        std::thread::spawn(move || {
                            // TODO: Repairing is only possible on Windows because
                            // Linux's manifest doesn't include anything from Bin/,
                            // it's only the AppImage
                            #[cfg(target_os = "windows")]
                            repair_install(&evt_tx);

                            let mut attempts = 0;
                            let result = loop {
                                {
                                    let mut guard =
                                        engine.lock().unwrap_or_else(PoisonError::into_inner);
                                    if let Some(e) = guard.as_mut() {
                                        break match e.inject(custom_files) {
                                            Ok(()) => Ok(e.clone()),
                                            Err(err) => Err(err),
                                        };
                                    }
                                }
                                if attempts >= 10 {
                                    break Err(anyhow!(
                                        "Engine not initialized, set a valid game path in settings"
                                    ));
                                }
                                attempts += 1;
                                thread::sleep(Duration::from_millis(500));
                            };
                            let mut session = match result {
                                Ok(session) => session,
                                Err(e) => {
                                    error!("Inject failed: {e}");
                                    evt_tx.send(EngineEvent::LaunchFailed(e.to_string())).ok();
                                    if let Some(e) = engine
                                        .lock()
                                        .unwrap_or_else(PoisonError::into_inner)
                                        .as_mut()
                                    {
                                        e.sanitize(false).ok();
                                    }
                                    GAME_RUNNING.store(false, Ordering::SeqCst);
                                    return;
                                }
                            };
                            evt_tx.send(EngineEvent::LaunchSuccess).ok();

                            if let Err(e) = session.monitor(evt_tx.clone()) {
                                error!("Monitor failed: {e}");
                            }
                            GAME_RUNNING.store(false, Ordering::SeqCst);
                            evt_tx.send(EngineEvent::GameClosed).ok();
                        });
                    }
                    EngineCommand::Sanitize => {
                        std::thread::spawn(move || {
                            match engine
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .as_mut()
                            {
                                Some(e) => {
                                    if let Err(e) = e.sanitize(true) {
                                        error!("Sanitize failed: {e}");
                                    }
                                }
                                None => error!("Sanitize failed: engine not initialized"),
                            }
                        });
                    }
                    EngineCommand::Update => {
                        std::thread::spawn(move || {
                            let game_path = match get_game_directory() {
                                Ok(p) => p,
                                Err(e) => {
                                    error!("Update failed: could not resolve game path: {e}");
                                    evt_tx
                                        .send(EngineEvent::EngineInitFailed(format!(
                                            "Game path not found: {e}"
                                        )))
                                        .ok();
                                    return;
                                }
                            };
                            evt_tx
                                .send(EngineEvent::GamePathUpdated(game_path.clone()))
                                .ok();
                            #[allow(clippy::option_if_let_else)]
                            let result = {
                                let mut guard =
                                    engine.lock().unwrap_or_else(PoisonError::into_inner);
                                if let Some(e) = guard.as_mut() {
                                    e.reinit(&game_path)
                                } else {
                                    AuroraEngine::new(&game_path).map(|e| {
                                        info!("Game Path: {}", game_path.display());
                                        *guard = Some(e);
                                    })
                                }
                            };
                            match result {
                                Ok(()) => {
                                    evt_tx.send(EngineEvent::EngineReady).ok();
                                }
                                Err(e) => {
                                    error!("Update failed: {e}");
                                    evt_tx
                                        .send(EngineEvent::EngineInitFailed(e.to_string()))
                                        .ok();
                                }
                            }
                        });
                    }
                    EngineCommand::KillProcesses => {
                        if let Err(e) = crate::engine::process::kill_nte_processes_standalone() {
                            error!("Failed to send kill process command: {e}");
                        }
                    }
                    EngineCommand::Validate => {
                        std::thread::spawn(move || {
                            // TODO: Repairing is only possible on Windows because
                            // Linux's manifest doesn't include anything from Bin/,
                            // it's only the AppImage
                            #[cfg(target_os = "windows")]
                            repair_install(&evt_tx);

                            if let Some(e) = engine
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .as_mut()
                            {
                                match e.validate() {
                                    Ok(missing) => {
                                        evt_tx.send(EngineEvent::ValidationResult { missing }).ok();
                                    }
                                    Err(e) => {
                                        error!("Validate failed: {e}");
                                        evt_tx
                                            .send(EngineEvent::Toast {
                                                text: format!("Validation failed: {e}"),
                                                kind: "error".to_string(),
                                            })
                                            .ok();
                                    }
                                }
                            } else {
                                error!("Validate failed: engine not initialized");
                                evt_tx
                                    .send(EngineEvent::Toast {
                                        text: "Validation failed: engine not initialized"
                                            .to_string(),
                                        kind: "error".to_string(),
                                    })
                                    .ok();
                            }
                        });
                    }
                }
            }
        });

        Ok(Self { cmd_tx, evt_rx })
    }
}
