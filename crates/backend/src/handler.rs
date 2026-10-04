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

    // TODO(lane B, phase 4): a Configure may carry a candidate tag that runtime has not
    // published yet. Only runtime's transition completion should accept its reply.
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
        let Some(mut active) = self.session.take() else {
            return;
        };
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use shared::classes::games::{
        Game as _, InstallationFacts,
        identity::GameId,
        launch::LaunchPlan,
        nte::NTE,
        payload::{PayloadLayoutKind, ResolvedPayloadLayout},
    };

    use super::*;
    use crate::engine::contract::{DeploymentSelection, EngineSettings, RecordChanges};

    const WAIT: Duration = Duration::from_secs(5);

    /// What the fake engines did, shared with the test after the factory
    /// moves into the worker.
    #[derive(Default)]
    struct Probe {
        log: Mutex<Vec<String>>,
        ended: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        exit: Mutex<Option<mpsc::Sender<SessionExit>>>,
    }

    impl Probe {
        fn record(&self, entry: String) {
            self.log
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(entry);
        }

        fn log(&self) -> Vec<String> {
            self.log
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        /// Simulates the game closing on its own.
        fn game_exits(&self) {
            let exit = self.exit.lock().unwrap().take().expect("a running session");
            exit.send(SessionExit::GameExited).unwrap();
            let ended = self.ended.lock().unwrap().take().expect("session hooks");
            ended();
        }
    }

    struct FakeFactory {
        supported: Vec<GameId>,
        probe: Arc<Probe>,
    }

    impl EngineFactory for FakeFactory {
        fn supports(&self, game: &GameId) -> bool {
            self.supported.contains(game)
        }

        fn create(&self, input: &EngineInput) -> Result<Box<dyn ModEngine>> {
            self.probe.record(format!("create {}", input.tag));
            Ok(Box::new(FakeEngine {
                probe: self.probe.clone(),
            }))
        }

        #[cfg(target_os = "windows")]
        fn repair_install(&self) -> shared::repair::RepairReport {
            shared::repair::RepairReport::default()
        }
    }

    struct FakeEngine {
        probe: Arc<Probe>,
    }

    fn plugin() -> InjectedPluginRecord {
        InjectedPluginRecord {
            path: PathBuf::from("Win64").join("plugin.asi"),
        }
    }

    impl ModEngine for FakeEngine {
        fn reconfigure(&mut self, input: &EngineInput) -> Result<()> {
            self.probe.record(format!("reconfigure {}", input.tag));
            Ok(())
        }

        fn validate(&mut self, request: &ValidateInput) -> OperationOutcome<ValidationReport> {
            self.probe.record(format!("validate {}", request.tag));
            OperationOutcome::ok(request.tag.clone(), ValidationReport::default())
        }

        fn launch(
            &mut self,
            request: &LaunchInput,
            hooks: SessionHooks,
        ) -> OperationOutcome<Box<dyn ModSession>> {
            self.probe.record(format!("launch {}", request.tag));
            let (exit_tx, exit_rx) = mpsc::channel();
            *self.probe.exit.lock().unwrap() = Some(exit_tx.clone());
            *self.probe.ended.lock().unwrap() = Some(hooks.ended);

            let mut records = RecordChanges::default();
            records.add(plugin());
            let session: Box<dyn ModSession> = Box::new(FakeSession {
                tag: request.tag.clone(),
                probe: self.probe.clone(),
                stop: exit_tx,
                exit: exit_rx,
            });
            OperationOutcome::ok(request.tag.clone(), session).with_records(records)
        }

        fn sanitize(&mut self, request: &SanitizeInput) -> OperationOutcome<()> {
            let plugins: Vec<String> = request
                .injected_plugins
                .iter()
                .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            self.probe
                .record(format!("sanitize {} {}", request.tag, plugins.join(",")));
            let mut records = RecordChanges::default();
            request
                .injected_plugins
                .iter()
                .cloned()
                .for_each(|r| records.remove(r));
            OperationOutcome::ok(request.tag.clone(), ()).with_records(records)
        }

        fn kill_processes(&mut self, tag: &CommandTag) -> OperationOutcome<()> {
            self.probe.record(format!("kill {tag}"));
            OperationOutcome::ok(tag.clone(), ())
        }
    }

    struct FakeSession {
        tag: CommandTag,
        probe: Arc<Probe>,
        stop: mpsc::Sender<SessionExit>,
        exit: mpsc::Receiver<SessionExit>,
    }

    impl ModSession for FakeSession {
        fn request_stop(&mut self) -> Result<()> {
            self.probe.record(format!("stop {}", self.tag));
            self.stop.send(SessionExit::Stopped).ok();
            Ok(())
        }

        /// Blocks like a real monitor until the game exits or a stop arrives.
        fn join(&mut self) -> OperationOutcome<SessionExit> {
            self.probe.record(format!("join {}", self.tag));
            match self.exit.recv_timeout(WAIT) {
                Ok(exit) => OperationOutcome::ok(self.tag.clone(), exit),
                Err(e) => OperationOutcome::failed(self.tag.clone(), anyhow!("join hung: {e}")),
            }
        }
    }

    struct Fixture {
        handle: EngineHandle,
        events: mpsc::Receiver<EngineEvent>,
        probe: Arc<Probe>,
        payload: ResolvedPayloadLayout,
        root: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn game_id(raw: &str) -> GameId {
        GameId::parse(raw).unwrap()
    }

    fn tag(game: &str, generation: u64) -> CommandTag {
        CommandTag {
            game_id: game_id(game),
            generation,
        }
    }

    /// A complete flat payload for the registered game, so the layout is
    /// built through its only constructor.
    fn payload_under(root: &Path) -> ResolvedPayloadLayout {
        for file in &NTE.descriptor().payload_files {
            let path = file.resolve_under(root);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"payload").unwrap();
        }
        for dir in ["Wrappers", "Plugins", "Addons"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        ResolvedPayloadLayout::inspect(&*NTE, root, PayloadLayoutKind::LegacyFlat).unwrap()
    }

    fn fixture(label: &str, supported: &[&str]) -> Fixture {
        let root =
            std::env::temp_dir().join(format!("aurora-handler-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let payload = payload_under(&root.join("Bin"));
        let probe = Arc::new(Probe::default());
        let (handle, events) = EngineHandle::spawn_with(FakeFactory {
            supported: supported.iter().map(|id| game_id(id)).collect(),
            probe: probe.clone(),
        });
        Fixture {
            handle,
            events,
            probe,
            payload,
            root,
        }
    }

    impl Fixture {
        fn input(&self, tag: &CommandTag, installed: bool) -> EngineInput {
            EngineInput {
                tag: tag.clone(),
                installation: installed.then(|| {
                    InstallationFacts::new(self.root.join("Game"), "global", "standalone")
                }),
                settings: EngineSettings {
                    engine_method: 0,
                    ignore_checksum: false,
                },
                payload: self.payload.clone(),
                deployment: DeploymentSelection::default(),
                injected_plugins: Vec::new(),
            }
        }

        fn launch_input(&self, tag: &CommandTag) -> LaunchInput {
            LaunchInput {
                tag: tag.clone(),
                plan: LaunchPlan {
                    executable: self.root.join("Game").join("Game.exe"),
                    working_directory: self.root.join("Game"),
                    arguments: Vec::new(),
                    environment: Vec::new(),
                    compatibility: None,
                    dll_overrides: Vec::new(),
                },
                deployment: DeploymentSelection::default(),
                injected_plugins: Vec::new(),
                proton_args: String::new(),
                proton_version: String::new(),
                proton_custom_path: String::new(),
            }
        }

        fn send(&self, command: EngineCommand) {
            self.handle.send(command).unwrap();
        }

        fn next(&self) -> EngineEvent {
            self.events
                .recv_timeout(WAIT)
                .expect("the worker should answer")
        }

        fn configure(&self, tag: &CommandTag, installed: bool) -> OperationOutcome<Readiness> {
            self.send(EngineCommand::Configure(self.input(tag, installed)));
            match self.next() {
                EngineEvent::Configured(outcome) => outcome,
                other => panic!("expected Configured, got {other:?}"),
            }
        }

        fn launch(&self, tag: &CommandTag) -> OperationOutcome<()> {
            self.send(EngineCommand::Launch(self.launch_input(tag)));
            match self.next() {
                EngineEvent::Launched(outcome) => outcome,
                other => panic!("expected Launched, got {other:?}"),
            }
        }

        fn session_closed(&self) -> OperationOutcome<SessionExit> {
            match self.next() {
                EngineEvent::SessionClosed(outcome) => outcome,
                other => panic!("expected SessionClosed, got {other:?}"),
            }
        }

        /// Proves nothing else is queued: the worker answers commands in
        /// order, so the next event must be the reply to this probe.
        fn assert_quiet(&self) {
            let probe = tag("quiet", u64::MAX);
            self.send(EngineCommand::KillProcesses(probe.clone()));
            match self.next() {
                EngineEvent::Killed(outcome) => assert_eq!(outcome.tag, probe),
                other => panic!("expected only the probe's reply, got {other:?}"),
            }
        }
    }

    fn error_text<T>(outcome: &OperationOutcome<T>) -> String {
        match &outcome.result {
            Ok(_) => panic!("expected a failure for {}", outcome.tag),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn fake_engine_trace_covers_a_whole_session() {
        let f = fixture("trace", &["nte"]);
        let t = tag("nte", 1);

        let configured = f.configure(&t, true);
        assert_eq!(configured.tag, t);
        assert_eq!(configured.result.unwrap(), Readiness::Ready);

        f.send(EngineCommand::Validate(ValidateInput {
            tag: t.clone(),
            deployment: DeploymentSelection::default(),
        }));
        match f.next() {
            EngineEvent::Validated(outcome) => assert!(outcome.result.is_ok()),
            other => panic!("expected Validated, got {other:?}"),
        }

        let launched = f.launch(&t);
        assert!(launched.result.is_ok());
        assert_eq!(launched.records.added, [plugin()]);

        f.probe.game_exits();
        let closed = f.session_closed();
        assert_eq!(closed.tag, t);
        assert_eq!(closed.result.unwrap(), SessionExit::GameExited);
        assert_eq!(closed.records.removed, [plugin()]);

        assert_eq!(
            f.probe.log(),
            [
                "create nte#1",
                "validate nte#1",
                "launch nte#1",
                "join nte#1",
                "sanitize nte#1 plugin.asi",
            ]
        );
        f.assert_quiet();
    }

    #[test]
    fn a_game_without_an_engine_is_rejected() {
        let f = fixture("engineless", &["nte"]);
        let t = tag("noengine", 1);

        let configured = f.configure(&t, true);
        assert!(error_text(&configured).contains("has no mod engine"));

        let launched = f.launch(&t);
        assert!(error_text(&launched).contains("Engine not initialized"));
        assert!(f.probe.log().is_empty(), "{:?}", f.probe.log());
    }

    #[test]
    fn a_missing_installation_leaves_the_engine_unavailable() {
        let f = fixture("uninstalled", &["nte"]);
        let t = tag("nte", 1);

        assert_eq!(
            f.configure(&t, false).result.unwrap(),
            Readiness::Unavailable
        );
        assert!(error_text(&f.launch(&t)).contains("Engine not initialized"));
        assert!(f.probe.log().is_empty(), "{:?}", f.probe.log());
    }

    #[test]
    fn stale_commands_are_rejected_without_reaching_the_engine() {
        let f = fixture("stale", &["nte", "other"]);
        let old = tag("nte", 1);
        let current = tag("nte", 2);

        f.configure(&old, true).result.unwrap();
        f.configure(&current, true).result.unwrap();

        f.send(EngineCommand::Validate(ValidateInput {
            tag: old.clone(),
            deployment: DeploymentSelection::default(),
        }));
        match f.next() {
            EngineEvent::Validated(outcome) => {
                assert_eq!(outcome.tag, old);
                assert!(error_text(&outcome).contains("stale"));
            }
            other => panic!("expected Validated, got {other:?}"),
        }
        assert!(error_text(&f.launch(&old)).contains("stale"));

        f.send(EngineCommand::StopSession(old));
        f.assert_quiet();

        // The same game reuses its engine; another game gets a fresh one.
        f.configure(&tag("other", 3), true).result.unwrap();
        assert_eq!(
            f.probe.log(),
            ["create nte#1", "reconfigure nte#2", "create other#3"]
        );
    }

    #[test]
    fn a_running_session_blocks_relaunch_and_reconfigure() {
        let f = fixture("busy", &["nte"]);
        let t = tag("nte", 1);
        f.configure(&t, true).result.unwrap();
        f.launch(&t).result.unwrap();

        assert!(error_text(&f.launch(&t)).contains("already running"));
        assert!(error_text(&f.configure(&tag("nte", 2), true)).contains("cannot reconfigure"));

        f.send(EngineCommand::StopSession(t));
        assert_eq!(f.session_closed().result.unwrap(), SessionExit::Stopped);
    }

    #[test]
    fn stop_joins_the_session_without_cleaning_up() {
        let f = fixture("stop", &["nte"]);
        let t = tag("nte", 1);
        f.configure(&t, true).result.unwrap();
        f.launch(&t).result.unwrap();

        f.send(EngineCommand::StopSession(tag("nte", 9)));
        f.send(EngineCommand::StopSession(t.clone()));
        let closed = f.session_closed();
        assert_eq!(closed.tag, t);
        assert_eq!(closed.result.unwrap(), SessionExit::Stopped);

        assert_eq!(
            f.probe.log(),
            ["create nte#1", "launch nte#1", "stop nte#1", "join nte#1"]
        );
        f.assert_quiet();
    }

    #[test]
    fn a_late_session_end_after_stop_is_ignored() {
        let f = fixture("late-end", &["nte"]);
        let t = tag("nte", 1);
        f.configure(&t, true).result.unwrap();
        f.launch(&t).result.unwrap();

        let ended = f.probe.ended.lock().unwrap().take().unwrap();
        f.send(EngineCommand::StopSession(t));
        f.session_closed().result.unwrap();

        ended();
        f.assert_quiet();
    }

    #[test]
    fn shutdown_stops_and_joins_a_running_session() {
        let f = fixture("shutdown", &["nte"]);
        let t = tag("nte", 1);
        f.configure(&t, true).result.unwrap();
        f.launch(&t).result.unwrap();

        let (done_tx, done_rx) = mpsc::channel();
        thread::scope(|scope| {
            scope.spawn(|| done_tx.send(f.handle.shutdown()).unwrap());
            done_rx
                .recv_timeout(WAIT)
                .expect("shutdown deadlocked")
                .unwrap();
        });

        assert_eq!(f.session_closed().result.unwrap(), SessionExit::Stopped);
        assert!(f.handle.send(EngineCommand::Shutdown).is_err());
        assert!(f.handle.shutdown().is_ok(), "a second shutdown is a no-op");
    }
}
