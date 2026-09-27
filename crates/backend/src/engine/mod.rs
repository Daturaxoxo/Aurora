pub mod contract;
mod lua;
pub mod pak;

use anyhow::{Result, anyhow};
use shared::classes::games::{EngineKind, identity::GameId, registry};

use contract::{
    CommandTag, EngineInput, LaunchInput, OperationOutcome, SanitizeInput, SessionExit,
    SessionHooks, ValidateInput, ValidationReport,
};
pub use pak::PakEngine;

pub trait ModEngine: Send {
    fn reconfigure(&mut self, input: &EngineInput) -> Result<()>;
    fn validate(&mut self, request: &ValidateInput) -> OperationOutcome<ValidationReport>;
    fn launch(
        &mut self,
        request: &LaunchInput,
        hooks: SessionHooks,
    ) -> OperationOutcome<Box<dyn ModSession>>;
    fn sanitize(&mut self, request: &SanitizeInput) -> OperationOutcome<()>;
    fn kill_processes(&mut self, tag: &CommandTag) -> OperationOutcome<()>;
}

pub trait ModSession: Send {
    fn request_stop(&mut self) -> Result<()>;
    fn join(&mut self) -> OperationOutcome<SessionExit>;
}

pub trait EngineFactory: Send + 'static {
    fn supports(&self, game: &GameId) -> bool;
    fn create(&self, input: &EngineInput) -> Result<Box<dyn ModEngine>>;
}

pub struct RegistryFactory;

impl EngineFactory for RegistryFactory {
    fn supports(&self, game: &GameId) -> bool {
        supports_game(game)
    }

    fn create(&self, input: &EngineInput) -> Result<Box<dyn ModEngine>> {
        create_engine(input)
    }
}

fn engine_kind(game: &GameId) -> Option<EngineKind> {
    registry()
        .iter()
        .find(|g| g.descriptor().id == *game)
        .and_then(|g| g.descriptor().engine)
}

pub fn supports_game(game: &GameId) -> bool {
    engine_kind(game).is_some()
}

pub fn create_engine(input: &EngineInput) -> Result<Box<dyn ModEngine>> {
    let game = &input.tag.game_id;
    match engine_kind(game) {
        Some(EngineKind::Pak) => Ok(Box::new(PakEngine::new(input)?)),
        None => Err(anyhow!("{game} has no mod engine")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_games_with_an_engine_are_supported() {
        for game in registry() {
            let d = game.descriptor();
            assert_eq!(supports_game(&d.id), d.engine.is_some(), "{}", d.id);
        }
    }

    #[test]
    fn unregistered_game_is_unsupported() {
        let id = GameId::parse("jngiusdngu").unwrap();
        assert!(!supports_game(&id));
    }
}
