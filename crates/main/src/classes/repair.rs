use crate::bridge::engine;
use crate::runtime;
use anyhow::{Result, anyhow};
use log::*;
use shared::utils::get_cache_dir;

pub struct RepairHandler;

impl RepairHandler {
    // TODO: Display any warnings, done actions, etc in a final window.
    // commit author: alawapr (1 month ago)
    // state of implementation: nothing 💀💀💀
    pub fn repair(validate_files: bool, clean_cache: bool, remove_files: bool) -> Result<()> {
        // held for the whole repair, the game cant be switched or reconfigured until it is done
        let guard = runtime::get()?.try_begin_mutation()?;
        engine::kill()?;
        if validate_files {
            info!("[Repair] Validating files");
            engine::validate()?;

            let game_path = guard
                .context()
                .installation()
                .map(|i| i.root().to_path_buf())
                .ok_or_else(|| anyhow!("Repair could not find the game directory"))?;
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

        drop(guard);
        Ok(())
    }
}
