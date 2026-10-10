mod selection;
mod services;
pub mod startup;
mod transition;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use anyhow::{Result, anyhow};
use backend::engine::contract::{CommandTag, Readiness, RecordChanges};
use backend::handler::{EngineCommand, EngineEvent, EngineHandle};
use log::*;
use shared::classes::games::{
    Game, InstallationFacts, identity::GameId, payload::ResolvedPayloadLayout,
};
use shared::config::ConfigChange;

use crate::bridge::engine;
pub use transition::{Reconfigured, TransitionKind};
static RUNTIME: OnceLock<GameRuntime> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Starting,
    Open,
    Transitioning,
    Degraded(String),
    ShuttingDown,
}

impl fmt::Display for Admission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Starting => write!(f, "still starting"),
            Self::Open => write!(f, "ready"),
            Self::Transitioning => write!(f, "changing games"),
            Self::Degraded(reason) => write!(f, "in a degraded state ({reason})"),
            Self::ShuttingDown => write!(f, "shutting down"),
        }
    }
}

#[derive(Clone)]
pub struct GameContext {
    game: &'static dyn Game,
    generation: u64,
    installation: Option<InstallationFacts>,
    payload: ResolvedPayloadLayout,
    readiness: Readiness,
}

impl GameContext {
    pub const fn game(&self) -> &'static dyn Game {self.game}
    pub fn id(&self) -> &GameId {&self.game.descriptor().id}
    pub const fn generation(&self) -> u64 {self.generation}

    pub fn tag(&self) -> CommandTag {
        CommandTag {
            game_id: self.id().clone(),
            generation: self.generation,
        }
    }

    pub const fn installation(&self) -> Option<&InstallationFacts> {self.installation.as_ref()}
    pub const fn payload(&self) -> &ResolvedPayloadLayout {&self.payload}

    pub const fn readiness(&self) -> Readiness {self.readiness}
}

impl fmt::Debug for GameContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GameContext")
            .field("tag", &self.tag())
            .field("installation", &self.installation)
            .field("payload", &self.payload.kind())
            .field("readiness", &self.readiness)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct RuntimeSnapshot {
    pub admission: Admission,
    pub game: &'static dyn Game,
    pub context: Option<Arc<GameContext>>,
}

pub enum RuntimeEvent {
    Engine(EngineEvent),
    Committed {
        kind: TransitionKind,
        context: Arc<GameContext>,
        notice: Option<String>,
    },
    Failed {
        kind: TransitionKind,
        error: String,
        degraded: bool,
        context: Option<Arc<GameContext>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Launch,
    Validate,
    Sanitize,
    Kill,
}

struct InFlight {
    operation: Operation,
    running: bool,
    guard: MutationGuard,
}

struct State {
    admission: Admission,
    selected: &'static dyn Game,
    current: Option<Arc<GameContext>>,
    mutations: usize,
    session: bool,
    deferred: Option<ConfigChange>,
}

pub struct GameRuntime {
    engine: EngineHandle,
    state: Mutex<State>,
    drained: Condvar,
    generations: AtomicU64,
    inflight: Mutex<Vec<InFlight>>,
    reply: Mutex<Option<transition::PendingReply>>,
    events: mpsc::Sender<RuntimeEvent>,
    pump: Mutex<Option<JoinHandle<()>>>,
}

pub struct MutationGuard {
    runtime: &'static GameRuntime,
    context: Arc<GameContext>,
    session: bool,
}

impl MutationGuard {
    pub const fn context(&self) -> &Arc<GameContext> {
        &self.context
    }
}

impl Drop for MutationGuard {
    fn drop(&mut self) {
        let mut state = self.runtime.lock_state();
        state.mutations = state.mutations.saturating_sub(1);
        if self.session {state.session = false;}
        drop(state);
        self.runtime.drained.notify_all();
        if self.session {
            self.runtime.resume_deferred();
        }
    }
}

pub fn start() -> Result<mpsc::Receiver<RuntimeEvent>> {
    if RUNTIME.get().is_some() {
        return Err(anyhow!("the game runtime was already started"));
    }

    let selected = selection::select_startup_game();
    let (engine, engine_events) = EngineHandle::spawn();
    let (events, rx) = mpsc::channel::<RuntimeEvent>();
    RUNTIME
        .set(GameRuntime {
            engine,
            state: Mutex::new(State {
                admission: Admission::Starting,
                selected,
                current: None,
                mutations: 0,
                session: false,
                deferred: None,
            }),
            drained: Condvar::new(),
            generations: AtomicU64::new(0),
            inflight: Mutex::new(Vec::new()),
            reply: Mutex::new(None),
            events,
            pump: Mutex::new(None),
        })
        .map_err(|_| anyhow!("the game runtime was already started"))?;

    let runtime = get()?;
    let pump = thread::Builder::new()
        .name("runtime-events".into())
        .spawn(move || {
            for event in engine_events {
                runtime.route(event);
            }
            trace!("Runtime event pump stopped");
        })?;
    *runtime.pump.lock().unwrap_or_else(PoisonError::into_inner) = Some(pump);
    Ok(rx)
}

pub fn get() -> Result<&'static GameRuntime> {
    RUNTIME
        .get()
        .ok_or_else(|| anyhow!("the game runtime has not been started"))
}

pub fn is_current_generation(generation: u64) -> bool {
    get().is_ok_and(|runtime| {
        runtime
            .lock_state()
            .current
            .as_ref()
            .is_some_and(|c| c.generation == generation)
    })
}

pub fn shutdown() {
    let Ok(runtime) = get() else { return };
    runtime.lock_state().admission = Admission::ShuttingDown;
    runtime.drained.notify_all();
    if let Err(e) = runtime.engine.shutdown() {
        error!("Engine shutdown failed: {e}");
    }

    let pump = runtime
        .pump
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(pump) = pump
        && pump.join().is_err()
    {
        error!("The runtime event pump panicked");
    }
}

impl GameRuntime {
    fn lock_state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn allocate_generation(&self) -> u64 {
        self.generations.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn snapshot(&self) -> RuntimeSnapshot {
        let state = self.lock_state();
        RuntimeSnapshot {
            admission: state.admission.clone(),
            game: state.selected,
            context: state.current.clone(),
        }
    }

    pub fn is_current(&self, tag: &CommandTag) -> bool {
        self.lock_state()
            .current
            .as_ref()
            .is_some_and(|c| c.tag() == *tag)
    }

    pub fn session_active(&self) -> bool {
        self.lock_state().session
    }

    pub fn try_begin_mutation(&'static self) -> Result<MutationGuard> {
        self.begin(false)
    }

    pub fn wait_for_mutation(&'static self, timeout: Duration) -> Result<MutationGuard> {
        let (state, _) = self
            .drained
            .wait_timeout_while(self.lock_state(), timeout, |s| {
                matches!(s.admission, Admission::Starting | Admission::Transitioning)
            })
            .unwrap_or_else(PoisonError::into_inner);
        drop(state);
        self.begin(false)
    }

    pub fn try_begin_session(&'static self) -> Result<MutationGuard> {
        self.begin(true)
    }

    fn begin(&'static self, session: bool) -> Result<MutationGuard> {
        let mut state = self.lock_state();
        if state.admission != Admission::Open {
            return Err(anyhow!("Aurora is {}, try again shortly", state.admission));
        }
        if session && state.session {
            return Err(anyhow!("A game session is already running"));
        }
        let context = state
            .current
            .clone()
            .ok_or_else(|| anyhow!("Engine not initialized, set a valid game path in settings"))?;

        state.mutations += 1;
        state.session |= session;
        drop(state);
        Ok(MutationGuard {
            runtime: self,
            context,
            session,
        })
    }

    pub fn submit(
        &self,
        operation: Operation,
        guard: MutationGuard,
        command: EngineCommand,
    ) -> Result<()> {
        let tag = guard.context.tag();
        self.inflight
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(InFlight {
                operation,
                running: false,
                guard,
            });

        self.engine.send(command).inspect_err(|_| {
            drop(self.take_inflight(operation, &tag, false));
        })
    }

    fn take_inflight(
        &self,
        operation: Operation,
        tag: &CommandTag,
        running: bool,
    ) -> Option<InFlight> {
        let mut inflight = self.inflight.lock().unwrap_or_else(PoisonError::into_inner);
        let index = inflight.iter().position(|op| {
            op.operation == operation && op.running == running && op.guard.context.tag() == *tag
        })?;
        Some(inflight.remove(index))
    }

    fn route(&'static self, event: EngineEvent) {
        let event = match event {
            EngineEvent::Configured(outcome) => {
                self.deliver_reply(outcome);
                return;
            }
            event => event,
        };

        self.settle(&event);
        if self.is_current(event.tag()) {
            self.events.send(RuntimeEvent::Engine(event)).ok();
        } else {
            debug!("Dropped a stale engine event for {}", event.tag());
        }
    }

    fn settle(&'static self, event: &EngineEvent) {
        let (operation, running, records, finished) = match event {
            EngineEvent::Launched(o) => (Operation::Launch, false, &o.records, o.result.is_err()),
            EngineEvent::SessionClosed(o) => (Operation::Launch, true, &o.records, true),
            EngineEvent::Validated(o) => (Operation::Validate, false, &o.records, true),
            EngineEvent::Sanitized(o) => (Operation::Sanitize, false, &o.records, true),
            EngineEvent::Killed(o) => (Operation::Kill, false, &o.records, true),
            _ => return,
        };
        let tag = event.tag();

        let Some(mut op) = self.take_inflight(operation, tag, running) else {
            if records.is_empty() {
                return;
            }
            warn!("{tag} reported injected plugin changes for an operation Aurora did not track");
            match self.try_begin_mutation() {
                Ok(guard) => self.persist_records(&guard, tag, records),
                Err(e) => error!("Lost the injected plugin changes for {tag}: {e}"),
            }
            return;
        };

        self.persist_records(&op.guard, tag, records);
        if !finished {
            op.running = true;
            self.inflight
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(op);
        }
    }

    fn persist_records(&self, guard: &MutationGuard, tag: &CommandTag, changes: &RecordChanges) {
        if changes.is_empty() {
            return;
        }
        if guard.context.tag() != *tag || !self.is_current(tag) {
            error!(
                "Refusing to save the injected plugins for {tag}: the guard belongs to {}",
                guard.context.tag()
            );
            return;
        }
        engine::persist_records(tag, changes);
    }
}
