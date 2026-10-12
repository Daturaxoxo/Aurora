pub mod engine;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use crate::classes::pages::addons::AddonsHandler;
use crate::classes::pages::games::GamesHandler;
use crate::classes::pages::modmanager::ModManagerHandler;
use crate::classes::pages::modules::ModulesHandler;
use crate::classes::pages::screenshots::ScreenshotHandler;
use crate::classes::pages::settings::SettingsHandler;
use crate::translations::tr;
use shared::classes::games::identity::GameId;
use crate::runtime::{self, GameContext, RuntimeEvent, TransitionKind, startup};
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

    pub fn quick_start() -> Result<()> {
        let events = runtime::start()?;
        let result = Self::quick_start_session(&events);
        runtime::shutdown();
        result
    }

    fn quick_start_session(events: &mpsc::Receiver<RuntimeEvent>) -> Result<()> {
        let context = startup::initialize()
            .map_err(|e| anyhow!("Quick start failed: engine could not initialise: {e}"))?;
        if context.readiness() != Readiness::Ready {
            return Err(anyhow!(
                "Quick start failed: engine could not initialise: no game installation"
            ));
        }
        info!("Quick start: engine ready for {}", context.tag());
        engine::launch()?;
        Self::quick_start_events(events)
    }

    fn quick_start_events(events: &mpsc::Receiver<RuntimeEvent>) -> Result<()> {
        for event in events {
            let RuntimeEvent::Engine(event) = event else {
                continue;
            };
            match event {
                EngineEvent::Launched(outcome) => match outcome.result {
                    Ok(()) => info!("Quick start: launcher opened, waiting for the game to exit"),
                    Err(e) => return Err(anyhow!("Quick start launch failed: {e}")),
                },
                EngineEvent::SessionClosed(_) => {
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
                EngineEvent::Configured(_)
                | EngineEvent::Validated(_)
                | EngineEvent::Sanitized(_)
                | EngineEvent::Killed(_) => {}
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
        let events = match runtime::start() {
            Ok(events) => events,
            Err(e) => {
                error!("Failed to start the game runtime: {e}");
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
            if let Err(e) = startup::initialize() {
                debug!("Startup preparation failed: {e:#}");
            }
            shared::api::ccu::spawn();

            for event in events {
                match event {
                    RuntimeEvent::Engine(event) => Self::handle_event(&w, event),
                    RuntimeEvent::Committed {
                        kind,
                        context,
                        notice,
                    } => Self::committed(&w, kind, &context, notice),
                    RuntimeEvent::Failed {
                        kind,
                        error,
                        degraded,
                        context,
                    } => Self::failed(&w, kind, &error, degraded, context.as_deref()),
                }
            }
        });
    }

    fn invoke_current(
        window: &slint::Weak<MainWindow>,
        generation: u64,
        f: impl FnOnce(&MainWindow) + Send + 'static,
    ) {
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if !runtime::is_current_generation(generation) {
                debug!("Skipped a UI update for superseded generation #{generation}");
                return;
            }
            if let Some(w) = w.upgrade() {
                f(&w);
            }
        })
        .ok();
    }

    fn committed(
        w: &slint::Weak<MainWindow>,
        kind: TransitionKind,
        context: &Arc<GameContext>,
        notice: Option<String>,
    ) {
        info!("Runtime committed {} ({kind:?})", context.tag());
        if let Some(notice) = notice {
            Self::report_configure(w, Err(anyhow!(notice)));
        }

        let ready = context.readiness() == Readiness::Ready;
        let root = context
            .installation()
            .map(|facts| facts.root().to_string_lossy().into_owned());
        Self::invoke_current(w, context.generation(), move |w| {
            if let Some(root) = root {
                w.set_game_directory(root.into());
            }
            if updater::UpdateHandler::ui_locked() {
                info!("Runtime committed while an update holds the UI lock");
                return;
            }
            w.set_launch_state(LaunchState::Launch);
            w.set_launch_disabled(!ready);
        });

        Self::context_changed(w, kind, context);
        AddonsHandler::context_committed(w, context.id());
    }

    fn context_changed(w: &slint::Weak<MainWindow>, kind: TransitionKind, context: &Arc<GameContext>) {
        static APPLIED: Mutex<Option<(GameId, Option<PathBuf>)>> = Mutex::new(None);

        let applied = (
            context.id().clone(),
            context.installation().map(|i| i.root().to_path_buf()),
        );
        let changed = APPLIED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(applied.clone())
            .is_none_or(|previous| previous != applied);
        let switched = kind == TransitionKind::Switch;

        let ui_context = context.clone();
        Self::invoke_current(w, context.generation(), move |w| {
            GamesHandler::refresh(w);
            SettingsHandler::apply_context(w, &ui_context);
            crate::classes::pages::apply_capabilities(w, ui_context.game());
            if switched {
                w.set_show_menu(false);
                w.set_show_addons(false);
                w.set_show_modules(false);
                w.set_show_lua_scripts(false);
                w.set_show_screenshots(false);
                w.set_show_mod_manager(false);
                w.set_show_gb_browser(false);
                crate::classes::pages::gbbrowser::GbBrowserHandler::reset(w);
            }
        });

        if changed {
            debug!("Reloading the page models for {}", context.tag());
            AddonsHandler::reload(w);
            ModManagerHandler::reload(w);
            ScreenshotHandler::reload(w);
            ModulesHandler::reload(w);
            Self::invoke_current(w, context.generation(), |w| {
                crate::classes::pages::lua::LuaScriptsHandler::reload(w);
            });
        }
        if switched {
            let name = context.game().descriptor().display_name;
            Self::show_toast(w, &tr("toast.game-switched").replace("{0}", name), "success");
        }
    }

    fn failed(
        w: &slint::Weak<MainWindow>,
        kind: TransitionKind,
        error: &str,
        degraded: bool,
        context: Option<&GameContext>,
    ) {
        Self::report_configure(w, Err(anyhow!(error.to_string())));
        slint::invoke_from_event_loop({
            let w = w.clone();
            move || {
                if let Some(w) = w.upgrade() {
                    GamesHandler::refresh(&w);
                }
            }
        })
        .ok();
        let Some(context) = context else {
            if degraded {
                let w = w.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(w) = w.upgrade()
                        && !updater::UpdateHandler::ui_locked()
                    {
                        w.set_launch_disabled(true);
                    }
                })
                .ok();
            }
            return;
        };

        info!("Runtime kept {} after a failed {kind:?}", context.tag());
        let ready = context.readiness() == Readiness::Ready && !degraded;
        let root = context
            .installation()
            .map(|facts| facts.root().to_string_lossy().into_owned());
        Self::invoke_current(w, context.generation(), move |w| {
            if let Some(root) = root {
                w.set_game_directory(root.into());
            }
            if !updater::UpdateHandler::ui_locked() {
                w.set_launch_disabled(!ready);
            }
        });
    }

    fn handle_event(w: &slint::Weak<MainWindow>, event: EngineEvent) {
        let generation = event.tag().generation;
        let w = w.clone();
        match event {
            EngineEvent::Configured(outcome) => {
                warn!("The configure reply for {} reached the bridge", outcome.tag);
            }
            EngineEvent::Launched(outcome) => match outcome.result {
                Ok(()) => {
                    let start_method = runtime::get()
                        .map_or(StartMethod::Manual, |r| engine::start_method(r.snapshot().game));
                    let toast_key = match start_method {
                        StartMethod::Direct => "toast.launch-direct",
                        StartMethod::Manual => "toast.launcher-opened",
                    };
                    Self::show_toast(&w, &crate::translations::tr(toast_key), "success");
                    Self::invoke_current(&w, generation, |w| {
                        w.set_launch_state(LaunchState::Running);
                    });

                    if config::get(key::UI_MINIMIZATION).as_bool().unwrap_or(true) {
                        crate::classes::tray::activate(&w, true);
                    }
                }
                Err(e) => {
                    Self::show_toast(&w, &e.to_string(), "error");
                    Self::invoke_current(&w, generation, |w| {
                        if updater::UpdateHandler::ui_locked() {
                            info!("Launch failed while an update holds the UI lock");
                            return;
                        }
                        w.set_launch_state(LaunchState::Launch);
                        w.set_launch_disabled(false);
                    });
                }
            },
            EngineEvent::SessionClosed(_) => {
                crate::classes::tray::deactivate(&w);
                let w_ui = w.clone();
                slint::invoke_from_event_loop(move || {
                    if let Some(w) = w_ui.upgrade() {
                        ModManagerHandler::game_closed(&w);
                    }
                })
                .ok();
                Self::invoke_current(&w, generation, |w| {
                    if updater::UpdateHandler::ui_locked() {
                        info!("Game closed while an update holds the UI lock");
                        return;
                    }
                    w.set_launch_state(LaunchState::Launch);
                    w.set_launch_disabled(false);
                });
                Self::show_toast(&w, "Game closed.", "success");
            }
            EngineEvent::Validated(outcome) => {
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
