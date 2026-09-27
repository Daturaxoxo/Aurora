pub mod engine;

use crate::classes::pages::modmanager::ModManagerHandler;
use crate::{LaunchState, MainWindow, PopupDetail, classes::updater};
use anyhow::{Result, anyhow};
use backend::engine::contract::{NotificationKind, Readiness};
use backend::handler::EngineEvent;
use log::*;
use shared::classes::info::version::StartMethod;
use shared::config::{self, key};

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

impl Bridge {
    pub fn game_busy() -> bool {
        engine::session_active()
    }

    // TODO(lane B, b4-3): run the shared startup preparation before launching.
    pub fn quick_start() -> Result<()> {
        let events = engine::start()?;
        engine::configure()
            .map_err(|e| anyhow!("Quick start failed: engine could not initialise: {e}"))?;

        let result = Self::quick_start_events(&events);
        if let Err(e) = engine::shutdown() {
            error!("Quick start: engine shutdown failed: {e}");
        }
        result
    }

    fn quick_start_events(events: &std::sync::mpsc::Receiver<EngineEvent>) -> Result<()> {
        for event in events {
            match event {
                EngineEvent::Configured(outcome) => match outcome.result {
                    Ok(Readiness::Ready) => {
                        info!("Quick start: engine ready");
                        engine::launch()?;
                    }
                    Ok(Readiness::Unavailable) => {
                        return Err(anyhow!(
                            "Quick start failed: engine could not initialise: no game installation"
                        ));
                    }
                    Err(e) => {
                        return Err(anyhow!(
                            "Quick start failed: engine could not initialise: {e}"
                        ));
                    }
                },
                EngineEvent::Launched(outcome) => {
                    engine::persist_records(&outcome.tag, &outcome.records);
                    match outcome.result {
                        Ok(()) => info!("Quick start: launcher opened, waiting for NTE to exit"),
                        Err(e) => return Err(anyhow!("Quick start launch failed: {e}")),
                    }
                }
                EngineEvent::SessionClosed(outcome) => {
                    engine::persist_records(&outcome.tag, &outcome.records);
                    info!("Quick start: game closed and clean-up finished, exiting");
                    return Ok(());
                }
                EngineEvent::Notification { kind, text, .. } => match kind {
                    NotificationKind::Error => error!("Quick start: {text}"),
                    NotificationKind::Warning => warn!("Quick start: {text}"),
                    NotificationKind::Success => info!("Quick start: {text}"),
                },
                EngineEvent::EverlightFatal { message, .. } => {
                    error!("Quick start: Everlight fatal error, game was closed: {message}");
                }
                EngineEvent::EverlightTimeout { .. } => {
                    error!(
                        "Quick start: Everlight produced no log within the timeout, game was \
                         closed. Try switching engine methods in settings."
                    );
                }
                EngineEvent::Validated(_) | EngineEvent::Sanitized(_) | EngineEvent::Killed(_) => {}
            }
        }
        Err(anyhow!("engine event channel closed unexpectedly"))
    }

    pub fn report_configure(window: &slint::Weak<MainWindow>, result: Result<()>) {
        let Err(e) = result else { return };
        let msg = e.to_string();
        if let Some(warning) = init_warning(&msg) {
            warn!("Engine failed to initialise: {msg}");
            Self::show_toast(window, warning, "warning");
        } else {
            error!("Engine failed to initialise: {msg}");
            Self::show_toast(
                window,
                &format!("Engine error: {msg}\nCheck your game path in Settings."),
                "error",
            );
        }
    }

    pub fn setup(window: &slint::Weak<MainWindow>) {
        let events = match engine::start() {
            Ok(events) => events,
            Err(e) => {
                error!("Failed to start engine handler: {e}");
                return;
            }
        };

        if let Some(w) = window.upgrade() {
            w.set_launch_disabled(true);
        }

        let w_launch = window.clone();
        if let Some(w) = window.upgrade() {
            w.on_launch_clicked(move || {
                if let Err(e) = engine::launch() {
                    error!("Launch failed: {e}");
                    Self::show_toast(&w_launch, &e.to_string(), "error");
                    return;
                }

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
            Self::report_configure(&w, engine::configure());
            if let Some(root) = engine::installation_root() {
                let path_str: String = root.to_string_lossy().into_owned();
                let w = w.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(w) = w.upgrade() {
                        w.set_game_directory(path_str.into());
                    }
                })
                .ok();
            }

            // TODO(lane B, b4-5): drop events whose tag is no longer the runtime's current one.
            for event in events {
                Self::handle_event(&w, event);
            }
        });
    }

    fn handle_event(w: &slint::Weak<MainWindow>, event: EngineEvent) {
        let w = w.clone();
        match event {
            EngineEvent::Configured(outcome) => match outcome.result {
                Ok(Readiness::Ready) => {
                    slint::invoke_from_event_loop(move || {
                        if updater::UpdateHandler::ui_locked() {
                            info!("Engine ready while an update holds the UI lock");
                            return;
                        }
                        if let Some(w) = w.upgrade() {
                            w.set_launch_disabled(false);
                        }
                    })
                    .ok();
                }
                Ok(Readiness::Unavailable) => {
                    info!("Engine for {} is unavailable", outcome.tag);
                }
                Err(e) => Self::report_configure(&w, Err(e)),
            },
            EngineEvent::Launched(outcome) => {
                engine::persist_records(&outcome.tag, &outcome.records);
                match outcome.result {
                    Ok(()) => {
                        let toast_key = match StartMethod::decode(&config::get(key::START_METHOD)) {
                            StartMethod::Direct => "toast.launch-direct",
                            StartMethod::Manual => "toast.launcher-opened",
                        };
                        Self::show_toast(&w, &crate::translations::tr(toast_key), "success");
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
                    Err(e) => {
                        if engine::session_ended() {
                            Self::report_configure(&w, engine::configure());
                        }
                        Self::show_toast(&w, &e.to_string(), "error");
                        slint::invoke_from_event_loop(move || {
                            if updater::UpdateHandler::ui_locked() {
                                info!("Launch failed while an update holds the UI lock");
                                return;
                            }
                            if let Some(w) = w.upgrade() {
                                w.set_launch_state(LaunchState::Launch);
                                w.set_launch_disabled(false);
                            }
                        })
                        .ok();
                    }
                }
            }
            EngineEvent::SessionClosed(outcome) => {
                engine::persist_records(&outcome.tag, &outcome.records);
                if engine::session_ended() {
                    Self::report_configure(&w, engine::configure());
                }
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
            EngineEvent::Validated(outcome) => {
                engine::persist_records(&outcome.tag, &outcome.records);
                match outcome.result {
                    Ok(report) if report.missing.is_empty() => {
                        info!("Validation passed, all required files are present");
                        Self::show_toast(
                            &w,
                            "Validation passed! All required files are present.",
                            "success",
                        );
                    }
                    Ok(report) => {
                        error!(
                            "Validation found missing files: {}",
                            report.missing.join(", ")
                        );
                        Self::show_toast(
                            &w,
                            &format!("Missing required files:\n{}", report.missing.join("\n")),
                            "error",
                        );
                    }
                    Err(e) => {
                        error!("Validate failed: {e}");
                        Self::show_toast(&w, &format!("Validation failed: {e}"), "error");
                    }
                }
            }
            EngineEvent::Sanitized(outcome) => {
                engine::persist_records(&outcome.tag, &outcome.records);
                if let Err(e) = outcome.result {
                    error!("Sanitize failed: {e}");
                }
            }
            EngineEvent::Killed(outcome) => {
                if let Err(e) = outcome.result {
                    error!("Failed to kill the game processes: {e}");
                }
            }
            EngineEvent::Notification { kind, text, .. } => {
                Self::show_toast(&w, &text, kind.as_str());
            }
            EngineEvent::EverlightFatal { message, .. } => {
                Self::show_popup(
                    &w,
                    "everlight-fatal",
                    "Everlight fatal error",
                    &format!(
                        "Everlight ran into a fatal error and cannot continue this \
                         session:\n\n{message}\n\nThe game has been closed."
                    ),
                );
            }
            EngineEvent::EverlightTimeout { .. } => {
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
