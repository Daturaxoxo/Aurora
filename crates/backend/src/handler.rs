use crate::engine::AuroraEngine;
use anyhow::{anyhow, Result};
use log::{error, info};
use shared::pathfind::get_game_directory;
use std::{
    path::PathBuf,
    sync::{mpsc, OnceLock},
};

pub static ENGINE_CMD_TX: OnceLock<mpsc::Sender<EngineCommand>> = OnceLock::new();

pub fn get_tx() -> Result<mpsc::Sender<EngineCommand>> {
    // Check if the engine was started (OnceLock is populated)
    let tx = ENGINE_CMD_TX
        .get()
        .ok_or_else(|| anyhow!("Engine has not been started yet!"))?;

    Ok(tx.clone())
}

pub enum EngineCommand {
    Launch(Option<Vec<PathBuf>>),
    Sanitize,
    Update,
}

pub enum EngineEvent {
    LaunchSuccess,
    LaunchFailed(String),
    GameClosed,
    Toast { text: String, kind: String },
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

            let mut engine = match AuroraEngine::new(&game_path) {
                Ok(e) => e,
                Err(e) => {
                    evt_tx.send(EngineEvent::LaunchFailed(e.to_string())).ok();
                    return;
                }
            };

            info!("Game Path: {}", game_path.display());

            for cmd in cmd_rx {
                match cmd {
                    EngineCommand::Launch(custom_files) => {
                        if let Err(e) = engine.inject(custom_files) {
                            error!("Inject failed: {e}");
                            evt_tx.send(EngineEvent::LaunchFailed(e.to_string())).ok();
                            engine.sanitize(false).ok();
                            continue;
                        }
                        evt_tx.send(EngineEvent::LaunchSuccess).ok();
                        if let Err(e) = engine.monitor() {
                            error!("Monitor failed: {e}");
                        }
                        evt_tx.send(EngineEvent::GameClosed).ok();
                    }
                    EngineCommand::Sanitize => {
                        if let Err(e) = engine.sanitize(true) {
                            error!("Sanitize failed: {e}");
                        }
                    }
                    EngineCommand::Update => {
                        if let Err(e) = engine.reinit(&game_path) {
                            error!("Update failed: {e}");
                        }
                    }
                }
            }
        });

        Ok(Self { cmd_tx, evt_rx })
    }
}
