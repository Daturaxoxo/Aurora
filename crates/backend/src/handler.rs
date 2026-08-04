use crate::engine::AuroraEngine;
use anyhow::{anyhow, Result};
use log::{error, info};
use shared::pathfind::get_game_directory;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

pub static ENGINE_CMD_TX: OnceLock<mpsc::Sender<EngineCommand>> = OnceLock::new();

/// True while a launched game session is live, i.e. between the
/// `LaunchSuccess` and `GameClosed` events.
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
    EngineReady,
    EngineInitFailed(String),
}

pub struct EngineHandler {
    pub cmd_tx: mpsc::Sender<EngineCommand>,
    pub evt_rx: mpsc::Receiver<EngineEvent>,
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
                            *engine.lock().unwrap() = Some(e);
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
                        std::thread::spawn(move || {
                            let mut attempts = 0;
                            let result = loop {
                                {
                                    let mut guard = engine.lock().unwrap();
                                    if let Some(e) = guard.as_mut() {
                                        break e.inject(custom_files);
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
                            if let Err(e) = result {
                                error!("Inject failed: {e}");
                                evt_tx.send(EngineEvent::LaunchFailed(e.to_string())).ok();
                                if let Some(e) = engine.lock().unwrap().as_mut() {
                                    e.sanitize(false).ok();
                                }
                                return;
                            }
                            GAME_RUNNING.store(true, Ordering::Relaxed);
                            evt_tx.send(EngineEvent::LaunchSuccess).ok();

                            std::thread::spawn(move || {
                                if let Some(e) = engine.lock().unwrap().as_mut() {
                                    if let Err(e) = e.monitor(evt_tx.clone()) {
                                        error!("Monitor failed: {e}");
                                    }
                                }
                                GAME_RUNNING.store(false, Ordering::Relaxed);
                                evt_tx.send(EngineEvent::GameClosed).ok();
                            });
                        });
                    }
                    EngineCommand::Sanitize => {
                        std::thread::spawn(move || match engine.lock().unwrap().as_mut() {
                            Some(e) => {
                                if let Err(e) = e.sanitize(true) {
                                    error!("Sanitize failed: {e}");
                                }
                            }
                            None => error!("Sanitize failed: engine not initialized"),
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
                                let mut guard = engine.lock().unwrap();
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
                        std::thread::spawn(move || match engine.lock().unwrap().as_mut() {
                            Some(e) => {
                                if let Err(e) = e.validate() {
                                    error!("Validate failed: {e}");
                                }
                            }
                            None => error!("Validate failed: engine not initialized"),
                        });
                    }
                }
            }
        });

        Ok(Self { cmd_tx, evt_rx })
    }
}
