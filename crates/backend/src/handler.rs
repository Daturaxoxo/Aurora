use crate::engine::AuroraEngine;
use anyhow::{anyhow, Result};
use log::{error, info};
use shared::pathfind::get_game_directory;
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex, OnceLock},
};

pub static ENGINE_CMD_TX: OnceLock<mpsc::Sender<EngineCommand>> = OnceLock::new();

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
    GamePathUpdated(PathBuf),
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
            let game_path = match get_game_directory() {
                Ok(p) => p,
                Err(e) => {
                    evt_tx
                        .send(EngineEvent::LaunchFailed(format!(
                            "Game path not found: {e}"
                        )))
                        .ok();
                    return;
                }
            };
            evt_tx
                .send(EngineEvent::GamePathUpdated(game_path.clone()))
                .ok();

            let engine = match AuroraEngine::new(&game_path) {
                Ok(e) => Arc::new(Mutex::new(e)),
                Err(e) => {
                    evt_tx.send(EngineEvent::LaunchFailed(e.to_string())).ok();
                    return;
                }
            };

            info!("Game Path: {}", game_path.display());

            for cmd in cmd_rx {
                let engine = engine.clone();
                let evt_tx = evt_tx.clone();
                match cmd {
                    EngineCommand::Launch(custom_files) => {
                        std::thread::spawn(move || {
                            if let Err(e) = engine.lock().unwrap().inject(custom_files) {
                                error!("Inject failed: {e}");
                                evt_tx.send(EngineEvent::LaunchFailed(e.to_string())).ok();
                                engine.lock().unwrap().sanitize(false).ok();
                                return;
                            }
                            evt_tx.send(EngineEvent::LaunchSuccess).ok();

                            let monitor_engine = engine.clone();
                            let monitor_evt_tx = evt_tx.clone();
                            std::thread::spawn(move || {
                                if let Err(e) = monitor_engine.lock().unwrap().monitor() {
                                    error!("Monitor failed: {e}");
                                }
                                monitor_evt_tx.send(EngineEvent::GameClosed).ok();
                            });
                        });
                    }
                    EngineCommand::Sanitize => {
                        std::thread::spawn(move || {
                            if let Err(e) = engine.lock().unwrap().sanitize(true) {
                                error!("Sanitize failed: {e}");
                            }
                        });
                    }
                    EngineCommand::Update => {
                        std::thread::spawn(move || {
                            let game_path = match get_game_directory() {
                                Ok(p) => p,
                                Err(e) => {
                                    error!("Update failed: could not resolve game path: {e}");
                                    return;
                                }
                            };
                            evt_tx
                                .send(EngineEvent::GamePathUpdated(game_path.clone()))
                                .ok();
                            if let Err(e) = engine.lock().unwrap().reinit(&game_path) {
                                error!("Update failed: {e}");
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
                            if let Err(e) = engine.lock().unwrap().validate() {
                                error!("Validate failed: {e}");
                            }
                        });
                    }
                }
            }
        });

        Ok(Self { cmd_tx, evt_rx })
    }
}
