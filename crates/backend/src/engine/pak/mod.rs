mod everlight;
mod files;
mod inject;
mod locks;
mod monitor;
mod process;
mod sanitize;
mod session;
mod state;
mod validate;
use anyhow::Result;
use log::*;
use super::contract::{
    CommandTag, EngineInput, InjectedPluginRecord, LaunchInput, OperationOutcome, RecordChanges,
    SanitizeInput, SessionHooks, ValidateInput, ValidationReport,
};
use super::{ModEngine, ModSession};
pub use session::PakSession;
pub use state::PakEngine;

impl PakEngine {
    fn cleanup(&self, injected: &[InjectedPluginRecord]) -> (Result<()>, RecordChanges) {
        if let Err(e) =
            monitor::ensure_processes_gone(&self.process_targets(), monitor::POST_EXIT_KILL_GRACE)
        {
            warn!("Could not confirm every NTE process exited: {e}");
        }

        self.sanitize_files(false, injected)
    }
}

impl ModEngine for PakEngine {
    fn reconfigure(&mut self, input: &EngineInput) -> Result<()> {
        info!("Reconfiguring the PAK engine for {}", input.tag);
        *self = Self::new(input)?;
        Ok(())
    }

    fn validate(&mut self, request: &ValidateInput) -> OperationOutcome<ValidationReport> {
        self.deployment = request.deployment.clone();
        let result = self
            .validate_files()
            .map(|missing| ValidationReport { missing });
        OperationOutcome::from_result(request.tag.clone(), result, RecordChanges::default())
    }

    fn launch(
        &mut self,
        request: &LaunchInput,
        hooks: SessionHooks,
    ) -> OperationOutcome<Box<dyn ModSession>> {
        let mut records = RecordChanges::default();
        let result = self
            .inject(request, &mut records)
            .map(|()| Box::new(PakSession::start(self, hooks)) as Box<dyn ModSession>);
        OperationOutcome::from_result(request.tag.clone(), result, records)
    }

    fn sanitize(&mut self, request: &SanitizeInput) -> OperationOutcome<()> {
        let (result, records) =
            self.sanitize_files(request.stop_processes, &request.injected_plugins);
        OperationOutcome::from_result(request.tag.clone(), result, records)
    }

    fn kill_processes(&mut self, tag: &CommandTag) -> OperationOutcome<()> {
        OperationOutcome::from_result(
            tag.clone(),
            self.kill_nte_processes(),
            RecordChanges::default(),
        )
    }
}
