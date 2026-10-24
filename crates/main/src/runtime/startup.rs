use super::GameContext;
use anyhow::{Context as _, Result};
use backend::classes::addons;
use shared::config;
use std::sync::Arc;
pub fn migrate() -> Result<()> {
    config::migrate();
    config::migrate_games()
        .context("could not copy the legacy settings into their game profile")?;
    addons::migrate();
    #[cfg(not(target_os = "linux"))]
    migrate_payloads()?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn migrate_payloads() -> Result<()> {
    use anyhow::anyhow;
    use shared::classes::games::{Game, registry, upgrade};

    let bin =
        shared::utils::get_bin_path().ok_or_else(|| anyhow!("could not resolve the Bin path"))?;
    let games: Vec<&dyn Game> = registry()
        .iter()
        .copied()
        .filter(|game| game.descriptor().engine.is_some())
        .collect();
    let committed = upgrade::manifest_layout(&ipc::install_root(), &games);
    let post_update = std::env::args().any(|arg| arg == ipc::POST_UPDATE_ARG);

    let failures: Vec<String> = games
        .iter()
        .filter_map(|game| {
            upgrade::migrate_legacy_runtime_data(*game, &bin, committed, post_update, &[])
                .err()
                .map(|e| format!("{}: {e:#}", game.descriptor().id))
        })
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "legacy payload migration failed ({})",
            failures.join("; ")
        ))
    }
}

pub fn initialize() -> Result<Arc<GameContext>> {
    super::get()?.initialize()
}
