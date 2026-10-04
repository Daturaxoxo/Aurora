use crate::bridge::engine;
use anyhow::{Context, Result};
use log::*;
use shared::{
    classes::games::locate::{locate_installation_root, transitional_selected_game},
    utils::get_cache_dir,
};

pub struct RepairHandler;

impl RepairHandler {
    // TODO: Display any warnings, done actions, etc in a final window.
    // commit author: alawapr (1 month ago)
    // state of implementation: nothing 💀💀💀
    pub fn repair(validate_files: bool, clean_cache: bool, remove_files: bool) -> Result<()> {
        engine::kill()?;
        if validate_files {
            info!("[Repair] Validating files");
            engine::validate()?;

            let game_path = locate_installation_root(transitional_selected_game())
                .context("Repair could not find the game directory")?;
            trace!("[Repair] Game directory: {}", game_path.display());
        }

        if remove_files {
            info!("[Repair] Removing files");
            engine::sanitize()?;
        }

        if clean_cache {
            info!("[Repair] Cleaning cache");
            std::fs::remove_dir_all(get_cache_dir())?;
        }

        Ok(())
    }
}
