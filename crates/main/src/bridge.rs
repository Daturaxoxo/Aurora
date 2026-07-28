use std::path::PathBuf;

use crate::MainWindow;
use backend::handler::{EngineCommand, EngineEvent, EngineHandler};
use log::*;
use shared::config::{self, key};

pub struct Bridge;

impl Bridge {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let handler = match EngineHandler::start() {
            Ok(h) => h,
            Err(e) => {
                error!("Failed to start engine handler: {e}");
                return;
            }
        };

        let cmd_tx = handler.cmd_tx.clone();
        let w_launch = window.clone();
        if let Some(w) = window.upgrade() {
            w.on_launch_clicked(move || {
                if config::get(key::CUSTOM_ADDONS_TOGGLED)
                    .as_bool()
                    .unwrap_or(false)
                {
                    let plugin_files = config::get(key::CUSTOM_ADDONS)
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(PathBuf::from)
                        .collect::<Vec<PathBuf>>();

                    let plugin_files = if plugin_files.is_empty() {
                        None
                    } else {
                        Some(plugin_files)
                    };
                    debug!("Launching game with plugins: {plugin_files:?}");
                    cmd_tx.send(EngineCommand::Launch(plugin_files)).ok();
                } else {
                    cmd_tx.send(EngineCommand::Launch(None)).ok();
                }

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
