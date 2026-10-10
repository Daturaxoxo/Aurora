use std::path::{Path, PathBuf};
use anyhow::{Context as _, Result, anyhow};
use log::*;
use super::{Game, default_game, find_game};
use crate::{
    config::{self, GlobalKey, ProfileKey, ProfilePatch, key},
    pathfind::{DiscoveryInputs, SelectedRoot, discover, select_configured, select_root},
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

pub fn stored_installation_root(game: &dyn Game) -> Result<Option<PathBuf>> {
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
    config::update_profile(id, &ProfilePatch::new().set(ProfileKey::GamePath, value))
        .with_context(|| format!("could not save the {id} game path"))?;

    mirror_legacy_game_path(game, root);
    Ok(())
}

pub fn mirror_legacy_game_path(game: &dyn Game, root: &Path) {
    let id = &game.descriptor().id;
    if id != &default_game().descriptor().id {
        return;
    }

    let value = root.to_string_lossy().into_owned();
    if !config::modify(|data| {
        data.insert(key::GAME_PATH.to_string(), value.into());
    }) {
        warn!("Could not mirror the {id} game path into the legacy key");
    }
}

pub fn find_installation_root(
    game: &dyn Game,
    configured: Option<&Path>,
    strict: bool,
) -> Result<Option<SelectedRoot>> {
    match configured {
        Some(path) => {
            if let Some(selected) = select_configured(game, path) {
                return Ok(Some(selected));
            }
            if strict {
                return Err(anyhow!("{} is not a game installation", path.display()));
            }
            warn!(
                "Stored game directory {} is not a valid install; searching for one",
                path.display()
            );
        }
        None => debug!("No game directory stored yet; searching for one"),
    }

    let instant = std::time::Instant::now();
    let candidates = discover(game, &DiscoveryInputs::observe_host(game))?;
    let found = select_root(game, None, &candidates)?;
    info!(
        "Discovery found {} candidate(s) in {:?}",
        candidates.len(),
        instant.elapsed()
    );
    Ok(found)
}

pub fn locate_installation_root(game: &dyn Game) -> Result<PathBuf> {
    let configured = stored_installation_root(game)?;
    let selected = find_installation_root(game, configured.as_deref(), false)?
        .ok_or_else(|| anyhow!("Game directory not found"))?;

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
