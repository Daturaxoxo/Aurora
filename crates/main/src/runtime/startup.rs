use super::GameContext;
use anyhow::Result;
use backend::classes::addons;
use log::*;
use shared::config;
use std::sync::Arc;
pub fn migrate() {
    config::migrate();
    if let Err(e) = config::migrate_games() {
        error!("Could not copy the legacy settings into their game profile: {e}");
    }
    addons::migrate();
}
pub fn initialize() -> Result<Arc<GameContext>> {
    super::get()?.initialize()
}
