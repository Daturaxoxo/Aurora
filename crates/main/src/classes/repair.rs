use crate::bridge::engine;
use crate::runtime;
use anyhow::{Result, anyhow};
use log::*;
use shared::utils::get_cache_dir;

pub struct RepairHandler;

#[derive(Default)]
pub struct RepairReport {
    pub done: Vec<String>,
    pub warnings: Vec<String>,
}

impl RepairReport {
    pub fn summary(&self) -> String {
        let mut lines: Vec<String> = self.done.iter().map(|d| format!("- {d}")).collect();
        if !self.warnings.is_empty() {
            lines.push(String::new());
            lines.push("Warnings:".to_owned());
            lines.extend(self.warnings.iter().map(|w| format!("- {w}")));
        }
        if lines.is_empty() {
            "Nothing was selected, so nothing was changed.".to_owned()
        } else {
            lines.join(
                "
",
            )
        }
    }
}

impl RepairHandler {
    pub fn repair(
        validate_files: bool,
        clean_cache: bool,
        remove_files: bool,
    ) -> Result<RepairReport> {
        let guard = runtime::get()?.try_begin_mutation()?;
        let mut report = RepairReport::default();
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
            report.done.push("Validated the game files".to_owned());
        }

        if remove_files {
            info!("[Repair] Removing files");
            engine::sanitize()?;
            report.done.push("Removed leftover files".to_owned());
        }

        if clean_cache {
            info!("[Repair] Cleaning cache");
            match std::fs::remove_dir_all(get_cache_dir()) {
                Ok(()) => report.done.push("Cleaned the cache".to_owned()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    report.done.push("The cache was already empty".to_owned());
                }
                Err(e) => {
                    warn!("[Repair] Could not clean the cache: {e}");
                    report
                        .warnings
                        .push(format!("Could not clean the cache: {e}"));
                }
            }
        }

        drop(guard);
        Ok(report)
    }
}
