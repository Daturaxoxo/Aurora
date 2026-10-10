use std::fmt;
use std::sync::{Arc, PoisonError, mpsc};
use std::thread;
use std::time::Duration;
use anyhow::{Result, anyhow};
use backend::engine::contract::{CommandTag, OperationOutcome, Readiness};
use backend::handler::EngineCommand;
use log::*;
use shared::classes::games::{
    Game, InstallationFacts, find_game,
    identity::GameId,
    locate::{find_installation_root, mirror_legacy_game_path, stored_installation_root},
};
use shared::config::{
    ConfigChange, ConfigUndo, GlobalKey, ProfileKey, ProfilePatch, commit_change, rollback_change,
};
use super::{Admission, GameContext, GameRuntime, RuntimeEvent, services};
use crate::bridge::engine;
const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionKind {
    Startup,
    Switch,
    Reconfigure,
}

impl fmt::Display for TransitionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Startup => write!(f, "start the game runtime"),
            Self::Switch => write!(f, "switch games"),
            Self::Reconfigure => write!(f, "reconfigure the game"),
        }
    }
}

#[derive(Debug)]
pub enum Reconfigured {
    Applied(Arc<GameContext>),
    Deferred,
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Admit,
    Drain,
    Allocate,
    Persist,
    Discover,
    Payload,
    Configure,
}

pub(super) struct PendingReply {
    tag: CommandTag,
    tx: mpsc::SyncSender<OperationOutcome<Readiness>>,
}

struct Request {
    kind: TransitionKind,
    target: Option<&'static dyn Game>,
    change: ConfigChange,
}

struct Previous {
    admission: Admission,
    selected: &'static dyn Game,
    context: Option<Arc<GameContext>>,
}

#[derive(Default)]
struct Work {
    undo: Vec<ConfigUndo>,
    engine_touched: bool,
}

struct Candidate {
    context: GameContext,
    notice: Option<String>,
}

enum Admitted {
    Run(Previous),
    Deferred,
}

impl GameRuntime {
    pub(super) fn initialize(&'static self) -> Result<Arc<GameContext>> {
        self.transition(&Request {
            kind: TransitionKind::Startup,
            target: None,
            change: ConfigChange::new(),
        })
        .map(|applied| match applied {
            Reconfigured::Applied(context) => context,
            Reconfigured::Deferred => unreachable!("startup is never deferred"),
        })
    }

    #[allow(dead_code)]
    pub fn switch(&'static self, id: &GameId) -> Result<Arc<GameContext>> {
        let game = find_game(id.as_str())
            .filter(|game| game.descriptor().id == *id)
            .ok_or_else(|| anyhow!("{id} is not a registered game"))?;

        self.transition(&Request {
            kind: TransitionKind::Switch,
            target: Some(game),
            change: ConfigChange::new().set_global(GlobalKey::SelectedGame, id.as_str()),
        })
        .map(|applied| match applied {
            Reconfigured::Applied(context) => context,
            Reconfigured::Deferred => unreachable!("a switch is never deferred"),
        })
    }

    pub fn reconfigure(&'static self, change: ConfigChange) -> Result<Reconfigured> {
        self.transition(&Request {
            kind: TransitionKind::Reconfigure,
            target: None,
            change,
        })
    }

    fn transition(&'static self, request: &Request) -> Result<Reconfigured> {
        let previous = match self.admit(request) {
            Ok(Admitted::Run(previous)) => previous,
            Ok(Admitted::Deferred) => return Ok(Reconfigured::Deferred),
            Err(e) => {
                warn!(
                    "Could not {}, failed at {:?}: {e}",
                    request.kind,
                    Step::Admit
                );
                self.events
                    .send(RuntimeEvent::Failed {
                        kind: request.kind,
                        error: e.to_string(),
                        degraded: false,
                        context: None,
                    })
                    .ok();
                return Err(e);
            }
        };

        let mut work = Work::default();
        match self.prepare(request, &previous, &mut work) {
            Ok(candidate) => Ok(Reconfigured::Applied(self.commit(request.kind, candidate))),
            Err((step, e)) => Err(self.abandon(request.kind, previous, &work, step, e)),
        }
    }

    fn admit(&self, request: &Request) -> Result<Admitted> {
        let mut state = self.lock_state();
        let busy = state.session
            || matches!(
                state.admission,
                Admission::Starting | Admission::Transitioning
            );

        match (&state.admission, request.kind) {
            (Admission::Starting, TransitionKind::Startup) => {}
            (Admission::ShuttingDown, _) | (_, TransitionKind::Startup) => {
                return Err(anyhow!("Aurora is {}", state.admission));
            }
            (_, TransitionKind::Reconfigure) if busy => {
                info!("Aurora is busy, the game will be reconfigured once it is free");
                let change = request.change.clone();
                state.deferred = Some(match state.deferred.take() {
                    Some(earlier) => earlier.then(change),
                    None => change,
                });
                return Ok(Admitted::Deferred);
            }
            (_, TransitionKind::Switch) if state.session => {
                return Err(anyhow!("Close the game before switching games"));
            }
            (Admission::Starting | Admission::Transitioning, _) => {
                return Err(anyhow!("Aurora is {}", state.admission));
            }
            _ => {}
        }

        let previous = Previous {
            admission: state.admission.clone(),
            selected: state.selected,
            context: state.current.clone(),
        };
        state.admission = Admission::Transitioning;
        drop(state);
        Ok(Admitted::Run(previous))
    }

    fn prepare(
        &'static self,
        request: &Request,
        previous: &Previous,
        work: &mut Work,
    ) -> std::result::Result<Candidate, (Step, anyhow::Error)> {
        self.drain().map_err(|e| (Step::Drain, e))?;
        let (game, generation) = self
            .allocate(request, previous)
            .map_err(|e| (Step::Allocate, e))?;
        let tag = CommandTag {
            game_id: game.descriptor().id.clone(),
            generation,
        };
        info!("Preparing {tag} to {}", request.kind);

        Self::persist(request, work).map_err(|e| (Step::Persist, e))?;
        let (installation, notice) =
            Self::discover(game, request, &tag, work).map_err(|e| (Step::Discover, e))?;
        let payload = engine::payload(game).map_err(|e| (Step::Payload, e))?;

        let mut context = GameContext {
            game,
            generation,
            installation,
            payload,
            readiness: Readiness::Unavailable,
        };
        work.engine_touched = true;
        context.readiness = self
            .configure_engine(&context)
            .map_err(|e| (Step::Configure, e))?;

        Ok(Candidate { context, notice })
    }

    fn drain(&self) -> Result<()> {
        let (state, wait) = self
            .drained
            .wait_timeout_while(self.lock_state(), DRAIN_TIMEOUT, |s| s.mutations > 0)
            .unwrap_or_else(PoisonError::into_inner);
        if wait.timed_out() && state.mutations > 0 {
            return Err(anyhow!(
                "{} operation(s) are still running, try again once they finish",
                state.mutations
            ));
        }
        Ok(())
    }

    fn allocate(&self, request: &Request, previous: &Previous) -> Result<(&'static dyn Game, u64)> {
        let game = request.target.unwrap_or(previous.selected);
        let d = game.descriptor();
        if d.engine.is_none() {
            return Err(anyhow!("{} has no mod engine", d.display_name));
        }
        Ok((game, self.allocate_generation()))
    }

    fn persist(request: &Request, work: &mut Work) -> Result<()> {
        if !request.change.is_empty() {
            work.undo.push(commit_change(&request.change)?);
        }
        Ok(())
    }

    fn discover(
        game: &'static dyn Game,
        request: &Request,
        tag: &CommandTag,
        work: &mut Work,
    ) -> Result<(Option<InstallationFacts>, Option<String>)> {
        let id = &game.descriptor().id;
        let strict = request.change.touches_profile(id, ProfileKey::GamePath);
        let configured = stored_installation_root(game)?;

        trace!("Discovering the installation for {tag}");
        let Some(selected) = find_installation_root(game, configured.as_deref(), strict)? else {
            info!("{tag} has no installation");
            return Ok((None, Some("Game path not found".to_string())));
        };
        info!(
            "Using game directory {} ({:?}) for {tag}",
            selected.root.display(),
            selected.provenance
        );

        if configured.as_deref() != Some(selected.root.as_path()) {
            let root = selected.root.to_string_lossy().into_owned();
            work.undo.push(commit_change(
                &ConfigChange::new()
                    .profile(id, ProfilePatch::new().set(ProfileKey::GamePath, root)),
            )?);
        }

        match game.inspect_installation(&selected.root) {
            Ok(facts) => Ok((Some(facts), None)),
            Err(e) if strict => Err(e),
            Err(e) => {
                warn!("{tag} has an installation Aurora cannot use: {e}");
                Ok((None, Some(e.to_string())))
            }
        }
    }

    fn configure_engine(&self, context: &GameContext) -> Result<Readiness> {
        let tag = context.tag();
        let input = engine::engine_input(
            context.game,
            tag.clone(),
            context.installation.clone(),
            context.payload.clone(),
        )?;

        let (tx, rx) = mpsc::sync_channel(1);
        *self.reply.lock().unwrap_or_else(PoisonError::into_inner) = Some(PendingReply {
            tag: tag.clone(),
            tx,
        });

        let reply = self
            .engine
            .send(EngineCommand::Configure(input))
            .and_then(|()| {
                rx.recv_timeout(CONFIGURE_TIMEOUT)
                    .map_err(|_| anyhow!("the engine did not answer within {CONFIGURE_TIMEOUT:?}"))
            });
        self.reply
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();

        let readiness = reply?.result?;
        info!("Engine for {tag} is {readiness:?}");
        Ok(readiness)
    }

    pub(super) fn deliver_reply(&self, outcome: OperationOutcome<Readiness>) {
        let pending = self.reply.lock().unwrap_or_else(PoisonError::into_inner);
        match pending.as_ref() {
            Some(pending) if pending.tag == outcome.tag => {
                pending.tx.try_send(outcome).ok();
            }
            _ => debug!(
                "Dropped the configure reply for {}, no transition is waiting on it",
                outcome.tag
            ),
        }
    }

    fn commit(&'static self, kind: TransitionKind, candidate: Candidate) -> Arc<GameContext> {
        let context = Arc::new(candidate.context);
        if let Some(installation) = &context.installation {
            mirror_legacy_game_path(context.game, installation.root());
        }
        {
            let mut state = self.lock_state();
            state.current = Some(context.clone());
            state.selected = context.game;
            state.admission = Admission::Open;
        }
        services::publish(&context);
        info!("Committed {}", context.tag());

        self.events
            .send(RuntimeEvent::Committed {
                kind,
                context: context.clone(),
                notice: candidate.notice,
            })
            .ok();
        self.resume_deferred();
        context
    }

    fn abandon(
        &'static self,
        kind: TransitionKind,
        previous: Previous,
        work: &Work,
        step: Step,
        error: anyhow::Error,
    ) -> anyhow::Error {
        warn!("Could not {kind}, failed at {step:?}: {error:#}. Rolling back");

        let mut problems = Vec::new();
        for undo in work.undo.iter().rev() {
            if let Err(e) = rollback_change(undo) {
                problems.push(format!("the settings could not be restored: {e:#}"));
            }
        }

        let mut restored = previous.context.clone();
        if work.engine_touched
            && let Some(context) = &previous.context
        {
            match self.restore(context) {
                Ok(context) => restored = Some(context),
                Err(e) => problems.push(format!("the engine could not be restored: {e:#}")),
            }
        }

        let admission = if !problems.is_empty() {
            Admission::Degraded(problems.join("; "))
        } else if previous.context.is_none() {
            Admission::Degraded(format!("{error:#}"))
        } else {
            previous.admission
        };
        let degraded = matches!(admission, Admission::Degraded(_));
        if degraded {
            error!("The game runtime is {admission}");
        }
        {
            let mut state = self.lock_state();
            state.admission = admission;
            state.selected = previous.selected;
            state.current.clone_from(&restored);
        }
        if let Some(context) = &restored
            && previous.context.as_ref().map(|c| c.generation) != Some(context.generation)
        {
            services::publish(context);
        }

        self.events
            .send(RuntimeEvent::Failed {
                kind,
                error: format!("{error:#}"),
                degraded,
                context: restored,
            })
            .ok();
        self.resume_deferred();
        error
    }

    fn restore(&self, previous: &GameContext) -> Result<Arc<GameContext>> {
        let mut context = previous.clone();
        context.generation = self.allocate_generation();
        context.readiness = self.configure_engine(&context)?;
        info!("Restored {} after the rollback", context.tag());
        Ok(Arc::new(context))
    }

    pub(super) fn resume_deferred(&'static self) {
        let change = {
            let mut state = self.lock_state();
            let free = !state.session
                && matches!(state.admission, Admission::Open | Admission::Degraded(_));
            if !free {
                return;
            }
            state.deferred.take()
        };

        let Some(change) = change else { return };
        info!("Applying the deferred reconfigure");
        let spawned = thread::Builder::new()
            .name("runtime-reconfigure".into())
            .spawn(move || {
                if let Err(e) = self.reconfigure(change) {
                    error!("Deferred reconfigure failed: {e:#}");
                }
            });
        if let Err(e) = spawned {
            error!("Could not start the deferred reconfigure: {e}");
        }
    }
}
