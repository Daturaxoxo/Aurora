use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use anyhow::{Result, anyhow};

use super::PakEngine;
use super::monitor::Monitor;
use crate::engine::ModSession;
use crate::engine::contract::{
    CommandTag, OperationOutcome, RecordChanges, SessionExit, SessionHooks,
};

pub struct PakSession {
    tag: CommandTag,
    stop: Arc<AtomicBool>,
    monitor: Option<JoinHandle<Result<SessionExit>>>,
}

impl PakSession {
    pub(super) fn start(engine: &PakEngine, hooks: SessionHooks) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let tag = hooks.events.tag().clone();
        let monitor = Monitor {
            targets: engine.process_targets(),
            game_path: engine.game_path.clone(),
            version: engine.version,
            ignore_checksum: engine.settings.ignore_checksum,
            events: hooks.events,
            stop: stop.clone(),
        };
        let ended = hooks.ended;
        let monitor = thread::spawn(move || {
            let exit = monitor.run();
            ended();
            exit
        });

        Self {
            tag,
            stop,
            monitor: Some(monitor),
        }
    }
}

impl ModSession for PakSession {
    fn request_stop(&mut self) -> Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn join(&mut self) -> OperationOutcome<SessionExit> {
        let result = match self.monitor.take() {
            Some(monitor) => monitor
                .join()
                .unwrap_or_else(|_| Err(anyhow!("session monitor panicked"))),
            None => Err(anyhow!("session {} was already joined", self.tag)),
        };
        OperationOutcome::from_result(self.tag.clone(), result, RecordChanges::default())
    }
}
