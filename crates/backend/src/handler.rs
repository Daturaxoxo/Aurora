use std::sync::{Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use anyhow::{Result, anyhow};
use log::*;
use crate::engine::contract::{
    CommandTag, EngineInput, EventSink, InjectedPluginRecord, LaunchInput, NotificationKind,
    OperationOutcome, Readiness, SanitizeInput, SessionExit, SessionHooks, ValidateInput,
    ValidationReport,
};
use crate::engine::{EngineFactory, ModEngine, ModSession, RegistryFactory};

pub enum EngineCommand {
    Configure(EngineInput),
    Launch(LaunchInput),
    Validate(ValidateInput),
    Sanitize(SanitizeInput),
    KillProcesses(CommandTag),
    StopSession(CommandTag),
    Shutdown,
}

#[derive(Debug)]
pub enum EngineEvent {
    Configured(OperationOutcome<Readiness>),
    Launched(OperationOutcome<()>),
    SessionClosed(OperationOutcome<SessionExit>),
    Validated(OperationOutcome<ValidationReport>),
    Sanitized(OperationOutcome<()>),
    Killed(OperationOutcome<()>),
    Notification {
        tag: CommandTag,
        kind: NotificationKind,
        text: String,
    },
    EverlightFatal {
        tag: CommandTag,
        message: String,
    },
    EverlightTimeout {
        tag: CommandTag,
    },
}

impl EngineEvent {
    pub const fn tag(&self) -> &CommandTag {
        match self {
            Self::Configured(o) => &o.tag,
            Self::Launched(o) | Self::Sanitized(o) | Self::Killed(o) => &o.tag,
            Self::SessionClosed(o) => &o.tag,
            Self::Validated(o) => &o.tag,
            Self::Notification { tag, .. }
            | Self::EverlightFatal { tag, .. }
            | Self::EverlightTimeout { tag } => tag,
        }
    }
}

enum WorkerMsg {
    Command(Box<EngineCommand>),
    SessionEnded(CommandTag),
}

pub struct EngineHandle {
    tx: mpsc::Sender<WorkerMsg>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl EngineHandle {
    pub fn spawn() -> (Self, mpsc::Receiver<EngineEvent>) {
        Self::spawn_with(RegistryFactory)
    }

    pub fn spawn_with(factory: impl EngineFactory) -> (Self, mpsc::Receiver<EngineEvent>) {
        let (tx, rx) = mpsc::channel::<WorkerMsg>();
        let (evt_tx, evt_rx) = mpsc::channel::<EngineEvent>();

        let worker = Worker {
            factory,
            events: evt_tx,
            internal: tx.clone(),
            configured: None,
            engine: None,
            session: None,
        };
        let worker = thread::spawn(move || worker.run(&rx));

        (
            Self {
                tx,
                worker: Mutex::new(Some(worker)),
            },
            evt_rx,
        )
    }

    pub fn send(&self, command: EngineCommand) -> Result<()> {
        self.tx
            .send(WorkerMsg::Command(Box::new(command)))
            .map_err(|_| anyhow!("the engine worker has stopped"))
    }

    pub fn shutdown(&self) -> Result<()> {
        let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        else {
            return Ok(());
        };
        self.tx
            .send(WorkerMsg::Command(Box::new(EngineCommand::Shutdown)))
            .ok();
        worker
            .join()
            .map_err(|_| anyhow!("the engine worker panicked"))
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        if let Err(e) = self.shutdown() {
            error!("Engine shutdown failed: {e}");
        }
    }
}

struct ActiveSession {
    tag: CommandTag,
    session: Box<dyn ModSession>,
    records: Vec<InjectedPluginRecord>,
}

struct Worker<F> {
    factory: F,
    events: mpsc::Sender<EngineEvent>,
    internal: mpsc::Sender<WorkerMsg>,
    configured: Option<CommandTag>,
    engine: Option<Box<dyn ModEngine>>,
    session: Option<ActiveSession>,
}

impl<F: EngineFactory> Worker<F> {
    fn run(mut self, rx: &mpsc::Receiver<WorkerMsg>) {
        for msg in rx {
            match msg {
                WorkerMsg::Command(command) if matches!(*command, EngineCommand::Shutdown) => break,
                WorkerMsg::Command(command) => self.handle(*command),
                WorkerMsg::SessionEnded(tag) => {
                    if self.session.as_ref().is_some_and(|s| s.tag == tag) {
                        self.finish_session(true);
                    }
                }
            }
        }

        if let Some(active) = self.session.as_mut() {
            info!("Stopping session {} for shutdown", active.tag);
            if let Err(e) = active.session.request_stop() {
                error!("Could not stop session {}: {e}", active.tag);
            }
            self.finish_session(false);
        }
        trace!("Engine worker stopped");
    }

    fn emit(&self, event: EngineEvent) {
        self.events.send(event).ok();
    }

    fn handle(&mut self, command: EngineCommand) {
        match command {
            EngineCommand::Configure(input) => {
                let outcome = self.configure(&input);
                self.emit(EngineEvent::Configured(outcome));
            }
            EngineCommand::Launch(input) => self.launch(&input),
            EngineCommand::Validate(input) => {
                #[cfg(target_os = "windows")]
                self.repair_install(&input.tag);

                let outcome = match self.engine_for(&input.tag) {
                    Ok(engine) => engine.validate(&input),
                    Err(e) => OperationOutcome::failed(input.tag, e),
                };
                self.emit(EngineEvent::Validated(outcome));
            }
            EngineCommand::Sanitize(input) => {
                let outcome = match self.engine_for(&input.tag) {
                    Ok(engine) => engine.sanitize(&input),
                    Err(e) => OperationOutcome::failed(input.tag, e),
                };
                self.emit(EngineEvent::Sanitized(outcome));
            }
            EngineCommand::KillProcesses(tag) => {
                let outcome = match self.engine_for(&tag) {
                    Ok(engine) => engine.kill_processes(&tag),
                    Err(e) => OperationOutcome::failed(tag, e),
                };
                self.emit(EngineEvent::Killed(outcome));
            }
            EngineCommand::StopSession(tag) => {
                let Some(active) = self.session.as_mut().filter(|s| s.tag == tag) else {
                    warn!("Ignoring a stop for {tag}: it has no running session");
                    return;
                };
                if let Err(e) = active.session.request_stop() {
                    error!("Could not stop session {tag}: {e}");
                }
                self.finish_session(false);
            }
            EngineCommand::Shutdown => unreachable!("handled by run"),
        }
    }

    // TODO(lane B, phase 4): a Configure may carry a candidate tag that runtime has not published yet. only runtime's transition completion should accept replies.
    fn configure(&mut self, input: &EngineInput) -> OperationOutcome<Readiness> {
        let tag = input.tag.clone();
        if let Some(active) = &self.session {
            return OperationOutcome::failed(
                tag,
                anyhow!("cannot reconfigure while session {} is running", active.tag),
            );
        }

        let same_game = self
            .configured
            .as_ref()
            .is_some_and(|c| c.game_id == tag.game_id);
        let reusable = if same_game { self.engine.take() } else { None };
        self.engine = None;
        self.configured = Some(tag.clone());

        if !self.factory.supports(&tag.game_id) {
            return OperationOutcome::failed(
                tag.clone(),
                anyhow!("{} has no mod engine", tag.game_id),
            );
        }
        if input.installation.is_none() {
            info!("{tag} has no installation, the engine is unavailable");
            return OperationOutcome::ok(tag, Readiness::Unavailable);
        }

        let built = match reusable {
            Some(mut engine) => engine.reconfigure(input).map(|()| engine),
            None => self.factory.create(input),
        };
        match built {
            Ok(engine) => {
                self.engine = Some(engine);
                info!("Engine ready for {tag}");
                OperationOutcome::ok(tag, Readiness::Ready)
            }
            Err(e) => {
                error!("Engine for {tag} failed to configure: {e}");
                OperationOutcome::failed(tag, e)
            }
        }
    }

    fn engine_for(&mut self, tag: &CommandTag) -> Result<&mut Box<dyn ModEngine>> {
        match &self.configured {
            Some(current) if current == tag => {}
            Some(current) => {
                return Err(anyhow!(
                    "rejected stale command for {tag}, the engine is configured for {current}"
                ));
            }
            None => return Err(anyhow!("rejected command for {tag}, nothing is configured")),
        }
        self.engine
            .as_mut()
            .ok_or_else(|| anyhow!("Engine not initialized, set a valid game path in settings"))
    }

    fn launch(&mut self, input: &LaunchInput) {
        let tag = input.tag.clone();
        if self.session.is_some() {
            warn!("Launch ignored: a game session is already running");
            self.emit(EngineEvent::Launched(OperationOutcome::failed(
                tag,
                anyhow!("A game session is already running"),
            )));
            return;
        }

        #[cfg(target_os = "windows")]
        self.repair_install(&tag);

        let internal = self.internal.clone();
        let ended_tag = tag.clone();
        let hooks = SessionHooks {
            events: EventSink::new(tag.clone(), self.events.clone()),
            ended: Box::new(move || {
                internal.send(WorkerMsg::SessionEnded(ended_tag)).ok();
            }),
        };

        let outcome = match self.engine_for(&tag) {
            Ok(engine) => engine.launch(input, hooks),
            Err(e) => OperationOutcome::failed(tag.clone(), e),
        };
        let OperationOutcome {
            result, records, ..
        } = outcome;

        let result = result.map(|session| {
            self.session = Some(ActiveSession {
                tag: tag.clone(),
                session,
                records: records.apply(&input.injected_plugins),
            });
        });
        if let Err(e) = &result {
            error!("Inject failed: {e}");
        }
        self.emit(EngineEvent::Launched(OperationOutcome::from_result(
            tag, result, records,
        )));
    }

    fn finish_session(&mut self, cleanup: bool) {
        let Some(mut active) = self.session.take() else {return};
        let mut outcome = active.session.join();

        let exited = matches!(
            outcome.result,
            Ok(SessionExit::GameExited | SessionExit::LauncherAbandoned)
        );
        if cleanup && exited {
            let request = SanitizeInput {
                tag: active.tag.clone(),
                stop_processes: false,
                injected_plugins: outcome.records.apply(&active.records),
            };
            match self.engine_for(&active.tag) {
                Ok(engine) => {
                    let cleaned = engine.sanitize(&request);
                    if let Err(e) = &cleaned.result {
                        error!("Clean-up after {} failed: {e}", active.tag);
                    }
                    outcome.records.extend(cleaned.records);
                }
                Err(e) => error!("Could not clean up after {}: {e}", active.tag),
            }
        }
        if let Err(e) = &outcome.result {
            error!("Monitor failed: {e}");
        }

        self.emit(EngineEvent::SessionClosed(outcome));
    }

    #[cfg(target_os = "windows")]
    fn repair_install(&self, tag: &CommandTag) {
        let events = EventSink::new(tag.clone(), self.events.clone());
        let report = self.factory.repair_install();

        if !report.restored.is_empty() {
            info!("Repaired {} missing file(s)", report.restored.len());
            events.notify(
                NotificationKind::Success,
                format!("Restored {} missing Aurora file(s).", report.restored.len()),
            );
        }

        if !report.failed.is_empty() {
            error!(
                "Could not repair {} missing file(s): {}",
                report.failed.len(),
                report.failed.join(", ")
            );
            events.notify(
                NotificationKind::Error,
                format!(
                    "Could not restore {} missing Aurora file(s). Check your connection and antivirus.",
                    report.failed.len()
                ),
            );
        }
    }
}