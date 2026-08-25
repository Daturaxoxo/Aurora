use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::classes::pages::modmanager::ModManagerHandler;
use crate::{LaunchState, MainWindow, PopupDetail, classes::updater};
use anyhow::{Result, anyhow};
use backend::handler::{self, EngineCommand, EngineEvent, EngineHandler};
use log::*;
use shared::config::{self, key};

#[derive(Default)]
pub struct PopupSpec {
    pub id: String,
    pub title: String,
    pub message: String,
    pub kind: String,
    pub subtitle: String,
    pub subject: String,
    pub subject_note: String,
    pub details: Vec<(String, String)>,
    pub notice: String,
    pub confirm_label: String,
    pub escape_locked: bool,
}

impl PopupSpec {
    pub fn apply(self, w: &MainWindow) {
        let details: Vec<PopupDetail> = self
            .details
            .into_iter()
            .map(|(label, value)| PopupDetail {
                label: label.into(),
                value: value.into(),
            })
            .collect();

        w.set_popup_id(self.id.into());
        w.set_popup_title(self.title.into());
        w.set_popup_message(self.message.into());
        w.set_popup_kind(if self.kind.is_empty() {
            "info".into()
        } else {
            self.kind.into()
        });
        w.set_popup_subtitle(self.subtitle.into());
        w.set_popup_subject(self.subject.into());
        w.set_popup_subject_note(self.subject_note.into());
        w.set_popup_details(slint::ModelRc::new(slint::VecModel::from(details)));
        w.set_popup_notice(self.notice.into());
        w.set_popup_confirm_label(self.confirm_label.into());
        w.set_popup_escape_locked(self.escape_locked);
        w.set_popup_confirm_delay(0);
        w.set_popup_required_count(0);
        w.set_popup_checkboxes(slint::ModelRc::default());
        w.set_popup_active(true);
    }
}

pub struct Bridge;

static GAME_BUSY: AtomicBool = AtomicBool::new(false);

impl Bridge {
    pub fn game_busy() -> bool {
        GAME_BUSY.load(Ordering::SeqCst)
    }

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
                    return Err(anyhow!(
                        "Quick start failed: engine could not initialise: {msg}"
                    ));
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
                EngineEvent::EverlightFatal(msg) => {
                    error!("Quick start: Everlight fatal error, game was closed: {msg}");
                }
                EngineEvent::EverlightTimeout => {
                    error!(
                        "Quick start: Everlight produced no log within the timeout, game was \
                         closed. Try switching engine methods in settings."
                    );
                }
                EngineEvent::ValidationResult { missing } => {
                    if missing.is_empty() {
                        info!("Quick start: validation passed, all required files are present");
                    } else {
                        error!(
                            "Quick start: validation found missing files: {}",
                            missing.join(", ")
                        );
                    }
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
                GAME_BUSY.store(true, Ordering::SeqCst);
                cmd_tx.send(EngineCommand::Launch(plugin_files)).ok();

                let w_inner = w_launch.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(w) = w_inner.upgrade() {
                        w.set_launch_state(LaunchState::Launching);
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
                            if updater::UpdateHandler::ui_locked() {
                                info!("Engine ready while an update holds the UI lock");
                                return;
                            }
                            if let Some(w) = w_ui.upgrade() {
                                w.set_launch_disabled(false);
                            }
                        })
                        .ok();
                    }
                    EngineEvent::EngineInitFailed(msg) => {
                        if let Some(warning) = handler::init_warning(&msg) {
                            warn!("Engine failed to initialise: {msg}");
                            Self::show_toast(&w, warning, "warning");
                        } else {
                            error!("Engine failed to initialise: {msg}");
                            Self::show_toast(
                                &w,
                                &format!("Engine error: {msg}\nCheck your game path in Settings."),
                                "error",
                            );
                        }
                    }
                    EngineEvent::LaunchSuccess => {
                        Self::show_toast(
                            &w,
                            &crate::translations::tr("toast.launcher-opened"),
                            "success",
                        );
                        let w_ui = w.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(w) = w_ui.upgrade() {
                                w.set_launch_state(LaunchState::Running);
                            }
                        })
                        .ok();

                        if config::get(key::UI_MINIMIZATION).as_bool().unwrap_or(true) {
                            crate::classes::tray::activate(&w, true);
                        }
                    }
                    EngineEvent::LaunchFailed(msg) => {
                        GAME_BUSY.store(false, Ordering::SeqCst);
                        Self::show_toast(&w, &msg, "error");
                        let w_ui = w.clone();
                        slint::invoke_from_event_loop(move || {
                            if updater::UpdateHandler::ui_locked() {
                                info!("Launch failed while an update holds the UI lock");
                                return;
                            }
                            if let Some(w) = w_ui.upgrade() {
                                w.set_launch_state(LaunchState::Launch);
                                w.set_launch_disabled(false);
                            }
                        })
                        .ok();
                    }
                    EngineEvent::GameClosed => {
                        GAME_BUSY.store(false, Ordering::SeqCst);
                        crate::classes::tray::deactivate(&w);
                        let w_ui = w.clone();
                        slint::invoke_from_event_loop(move || {
                            let Some(w) = w_ui.upgrade() else { return };
                            ModManagerHandler::game_closed(&w);
                            if updater::UpdateHandler::ui_locked() {
                                info!("Game closed while an update holds the UI lock");
                                return;
                            }
                            w.set_launch_state(LaunchState::Launch);
                            w.set_launch_disabled(false);
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
                    EngineEvent::ValidationResult { missing } => {
                        if missing.is_empty() {
                            info!("Validation passed, all required files are present");
                            Self::show_toast(
                                &w,
                                "Validation passed! All required files are present.",
                                "success",
                            );
                        } else {
                            error!("Validation found missing files: {}", missing.join(", "));
                            Self::show_toast(
                                &w,
                                &format!("Missing required files:\n{}", missing.join("\n")),
                                "error",
                            );
                        }
                    }
                    EngineEvent::Toast { text, kind } => {
                        Self::show_toast(&w, &text, &kind);
                    }
                    EngineEvent::EverlightFatal(msg) => {
                        Self::show_popup(
                            &w,
                            "everlight-fatal",
                            "Everlight fatal error",
                            &format!(
                                "Everlight ran into a fatal error and cannot continue this \
                                 session:\n\n{msg}\n\nThe game has been closed."
                            ),
                        );
                    }
                    EngineEvent::EverlightTimeout => {
                        Self::show_popup(
                            &w,
                            "everlight-timeout",
                            "Everlight did not start",
                            "Everlight did not produce a log file within 45 seconds, so the game \
                             has been closed.\n\nTry switching engine methods in Settings.",
                        );
                    }
                }
            }
        });
    }

    pub fn show_popup(window: &slint::Weak<MainWindow>, id: &str, title: &str, message: &str) {
        Self::show_popup_spec(
            window,
            PopupSpec {
                id: id.to_owned(),
                title: title.to_owned(),
                message: message.to_owned(),
                ..PopupSpec::default()
            },
        );
    }

    pub fn show_popup_spec(window: &slint::Weak<MainWindow>, spec: PopupSpec) {
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                spec.apply(&w);
            }
        })
        .ok();
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
