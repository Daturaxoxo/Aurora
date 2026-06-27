use crate::engine::AuroraEngine;
use anyhow::Result;
use log::{error, info};
use shared::pathfind::get_game_directory;
use std::sync::mpsc;

pub enum EngineCommand {
    Launch,
    Sanitize,
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

            let mut engine = match AuroraEngine::new(game_path.to_str().unwrap_or_default()) {
                Ok(e) => e,
                Err(e) => {
                    evt_tx.send(EngineEvent::LaunchFailed(e.to_string())).ok();
                    return;
                }
            };

            info!("Game Path: {}", game_path.display());

            for cmd in cmd_rx {
                match cmd {
                    EngineCommand::Launch => {
                        if let Err(e) = engine.inject() {
                            error!("Inject failed: {e}");
                            evt_tx.send(EngineEvent::LaunchFailed(e.to_string())).ok();
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
                }
            }
        });

        Ok(Self { cmd_tx, evt_rx })
    }
}
