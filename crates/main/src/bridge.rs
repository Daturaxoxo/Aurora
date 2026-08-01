use std::path::PathBuf;

use crate::MainWindow;
use anyhow::{anyhow, Result};
use backend::handler::{EngineCommand, EngineEvent, EngineHandler};
use log::*;
use shared::config::{self, key};

pub struct Bridge;

impl Bridge {
    pub fn quick_start() -> Result<()> {
        let handler = EngineHandler::start()?;
        handler
            .cmd_tx
            .send(EngineCommand::Launch(Self::custom_addon_files()))
            .map_err(|e| anyhow!("failed to send launch command: {e}"))?;

        for event in handler.evt_rx {
            match event {
                EngineEvent::EngineReady => {
                    info!("Quick start: engine ready");
                }
                EngineEvent::EngineInitFailed(msg) => {
                    return Err(anyhow!("Quick start failed: engine could not initialise: {msg}"));
                }
                EngineEvent::LaunchSuccess => {
                    info!("Quick start: launcher opened, waiting for NTE to exit");
                }
                EngineEvent::LaunchFailed(msg) => {
                    return Err(anyhow!("Quick start launch failed: {msg}"));
                }
                EngineEvent::GameClosed => {
                    info!("Quick start: game closed and clean-up finished, exiting");
                    return Ok(());
                }
                EngineEvent::Toast { .. } | EngineEvent::GamePathUpdated(_) => {}
            }
        }
        Err(anyhow!("engine event channel closed unexpectedly"))
    }

    fn custom_addon_files() -> Option<Vec<PathBuf>> {
        if !config::get(key::CUSTOM_ADDONS_TOGGLED)
            .as_bool()
            .unwrap_or(false)
        {
            return None;
        }
        let plugin_files = config::get(key::CUSTOM_ADDONS)
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(PathBuf::from)
                    .collect::<Vec<PathBuf>>()
            })
            .unwrap_or_default();

        if plugin_files.is_empty() {
            None
        } else {
            Some(plugin_files)
        }
    }

    pub fn setup(window: &slint::Weak<MainWindow>) {
        let handler = match EngineHandler::start() {
            Ok(h) => h,
            Err(e) => {
                error!("Failed to start engine handler: {e}");
                return;
            }
        };

        if let Some(w) = window.upgrade() {
            w.set_launch_disabled(true);
        }

        let cmd_tx = handler.cmd_tx.clone();
        let w_launch = window.clone();
        if let Some(w) = window.upgrade() {
            w.on_launch_clicked(move || {
                let plugin_files = Self::custom_addon_files();
                debug!("Launching game with plugins: {plugin_files:?}");
                cmd_tx.send(EngineCommand::Launch(plugin_files)).ok();

                let w_inner = w_launch.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(w) = w_inner.upgrade() {
                        w.set_launch_button_text("Launching...".into());
                        w.set_launch_disabled(true);
                    }
                })
                .ok();
            });
        }

        let w = window.clone();
        std::thread::spawn(move || {
            for event in handler.evt_rx {
                let w = w.clone();
                match event {
                    EngineEvent::EngineReady => {
                        let w_ui = w.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_ui.upgrade() {
                                w.set_launch_disabled(false);
                            }
                        })
                        .ok();
                    }
                    EngineEvent::EngineInitFailed(msg) => {
                        error!("Engine failed to initialise: {msg}");
                        Self::show_toast(&w, &format!("Engine error: {msg}\nCheck your game path in Settings."), "error");
                    }
                    EngineEvent::LaunchSuccess => {
                        Self::show_toast(
                            &w,
                            "Launcher opened! Please press \"Play\" on the NTE Launcher",
                            "success",
                        );
                        let w_ui = w.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_ui.upgrade() {
                                w.set_launch_button_text("Running...".into());
                            }
                        })
                        .ok();

                        if config::get(key::UI_MINIMIZATION).as_bool().unwrap_or(true) {
                            crate::classes::tray::activate(&w, true);
                        }
                    }
                    EngineEvent::LaunchFailed(msg) => {
                        Self::show_toast(&w, &msg, "error");
                        let w_ui = w.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_ui.upgrade() {
                                w.set_launch_button_text("Launch".into());
                                w.set_launch_disabled(false);
                            }
                        })
                        .ok();
                    }
                    EngineEvent::GameClosed => {
                        crate::classes::tray::deactivate(&w);
                        let w_ui = w.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_ui.upgrade() {
                                w.set_launch_button_text("Launch".into());
                                w.set_launch_disabled(false);
                            }
                        })
                        .ok();
                        Self::show_toast(&w, "Game closed.", "success");
                    }
                    EngineEvent::GamePathUpdated(path) => {
                        let path_str: String = path.to_string_lossy().into_owned();
                        let w_ui = w.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_ui.upgrade() {
                                w.set_game_directory(path_str.into());
                            }
                        })
                        .ok();
                    }
                    EngineEvent::Toast { text, kind } => {
                        Self::show_toast(&w, &text, &kind);
                    }
                }
            }
        });
    }

    // TODO: Refactor kind to an enum plz
    pub fn show_toast(window: &slint::Weak<MainWindow>, text: &str, kind: &str) {
        let text = text.to_string();
        let kind = kind.to_string();
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                w.set_toast_text(text.into());
                w.set_toast_kind(kind.into());
                w.set_toast_active(true);
            }
        })
        .ok();
    }
}