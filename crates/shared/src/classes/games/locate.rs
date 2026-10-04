use std::path::{Path, PathBuf};
use anyhow::{Context as _, Result, anyhow};
use log::*;
use super::{Game, default_game, find_game};
use crate::{
    config::{self, GlobalKey, ProfileKey, ProfilePatch, key},
    pathfind::{DiscoveryInputs, discover, select_configured, select_root},
};

pub fn transitional_selected_game() -> &'static dyn Game {
    let selected = config::read_global(GlobalKey::SelectedGame)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default();

    if selected.is_empty() {
        return default_game();
    }
    find_game(&selected).unwrap_or_else(|| {
        warn!("Unknown selected game '{selected}'; using the default game");
        default_game()
    })
}

fn configured_root(game: &dyn Game) -> Result<Option<PathBuf>> {
    let profile = config::read_profile(&game.descriptor().id)?;
    Ok(profile
        .get(ProfileKey::GamePath)
        .and_then(|value| value.as_str())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from))
}

pub fn store_installation_root(game: &dyn Game, root: &Path) -> Result<()> {
    let value = root.to_string_lossy().into_owned();
    let id = &game.descriptor().id;
    config::update_profile(
        id,
        &ProfilePatch::new().set(ProfileKey::GamePath, value.clone()),
    )
    .with_context(|| format!("could not save the {id} game path"))?;

    if id == &default_game().descriptor().id
        && !config::modify(|data| {
            data.insert(key::GAME_PATH.to_string(), value.into());
        })
    {
        warn!("Could not mirror the {id} game path into the legacy key");
    }
    Ok(())
}

pub fn locate_installation_root(game: &dyn Game) -> Result<PathBuf> {
    let configured = configured_root(game)?;

    let selected = configured.as_deref().map_or_else(
        || {
            debug!("No game directory stored yet; searching for one");
            None
        },
        |path| {
            let selected = select_configured(game, path);
            if selected.is_none() {
                warn!(
                    "Stored game directory {} is not a valid install; searching for one",
                    path.display()
                );
            }
            selected
        },
    );

    let selected = if let Some(selected) = selected {
        selected
    } else {
        let instant = std::time::Instant::now();
        let candidates = discover(game, &DiscoveryInputs::observe_host(game))?;
        let found = select_root(game, None, &candidates)?;
        info!(
            "Discovery found {} candidate(s) in {:?}",
            candidates.len(),
            instant.elapsed()
        );
        found.ok_or_else(|| anyhow!("Game directory not found"))?
    };

    info!(
        "Using game directory {} ({:?})",
        selected.root.display(),
        selected.provenance
    );
    if configured.as_deref() != Some(selected.root.as_path()) {
        store_installation_root(game, &selected.root)?;
    }
    Ok(selected.root)
}
