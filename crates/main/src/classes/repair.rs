use anyhow::{anyhow, Result};
use backend::handler::EngineCommand;
use log::*;
use shared::{
    pathfind,
    utils::{get_cache_dir, get_config_cache_dir},
};

pub struct RepairHandler;

impl RepairHandler {
    // TODO: Display any warnings, done actions, etc in a final window.
    pub fn repair(validate_files: bool, clean_cache: bool, remove_files: bool) -> Result<()> {
        let engine_handler = backend::handler::get_tx()?;
        engine_handler.send(EngineCommand::KillProcesses)?;
        if validate_files {
            trace!("[Repair] Validating files");
            engine_handler.send(EngineCommand::Validate)?;

            let res = pathfind::get_game_directory();
            if res.is_err() && res.is_ok_and(|p| p.is_empty()) {
                return Err(anyhow!("Game directory not found"));
            }
        }

        if remove_files {
            trace!("[Repair] Removing files");
            // TODO: Doesn't check for old files
            engine_handler.send(EngineCommand::Sanitize)?;
        }

        if clean_cache {
            trace!("[Repair] Cleaning cache");
            std::fs::remove_dir_all(get_cache_dir())?;
            std::fs::remove_dir_all(get_config_cache_dir())?;
        }

        Ok(())
    }
}
