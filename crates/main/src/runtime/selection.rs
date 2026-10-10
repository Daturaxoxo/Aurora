use log::*;
use serde_json::Value;
use shared::classes::games::{self, Game, find_game, identity::GameId, registry};
use shared::config::{self, ConfigChange, GlobalKey};
enum Fallback {
    Empty,
    Alias(String),
    Unknown(String),
    EngineLess(GameId),
}

fn fallback_game() -> &'static dyn Game {
    registry()
        .iter()
        .copied()
        .find(|game| game.descriptor().engine.is_some())
        .unwrap_or_else(games::default_game)
}

fn resolve(raw: &Value) -> (&'static dyn Game, Option<Fallback>) {
    let selected = match raw {
        Value::Null => "",
        Value::String(s) => s.as_str(),
        other => return (fallback_game(), Some(Fallback::Unknown(other.to_string()))),
    };
    if selected.trim().is_empty() {
        return (fallback_game(), Some(Fallback::Empty));
    }

    let Some(game) = find_game(selected.trim()) else {
        return (
            fallback_game(),
            Some(Fallback::Unknown(selected.to_string())),
        );
    };
    let id = &game.descriptor().id;
    if game.descriptor().engine.is_none() {
        return (fallback_game(), Some(Fallback::EngineLess(id.clone())));
    }
    if id.as_str() != selected {
        return (game, Some(Fallback::Alias(selected.to_string())));
    }
    (game, None)
}

pub(super) fn select_startup_game() -> &'static dyn Game {
    let raw = match config::read_global(GlobalKey::SelectedGame) {
        Ok(raw) => raw,
        Err(e) => {
            error!("Could not read the selected game, starting with the default one: {e:#}");
            return fallback_game();
        }
    };

    let (game, fallback) = resolve(&raw);
    let id = &game.descriptor().id;
    let Some(fallback) = fallback else {
        info!("Selected game: {id}");
        return game;
    };

    match &fallback {
        Fallback::Empty => info!("No game selected yet, selecting {id}"),
        Fallback::Alias(alias) => info!("'{alias}' is an alias of {id}, selecting {id}"),
        Fallback::Unknown(unknown) => warn!("Unknown selected game {unknown}, selecting {id}"),
        Fallback::EngineLess(other) => {
            warn!("{other} has no mod engine yet, selecting {id}");
        }
    }

    if let Err(e) =
        config::commit_change(&ConfigChange::new().set_global(GlobalKey::SelectedGame, id.as_str()))
    {
        warn!("Could not save {id} as the selected game: {e:#}");
    }
    game
}
