use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use log::*;
use shared::{classes::info::Target, config};

use crate::classes::addons::{CENSORSHIP_DIR, repair_file};
use crate::classes::validate::validate_files;

use super::AuroraEngine;
use super::files::FileGroup;

/// Files we have already tried to fetch this session
static ATTEMPTED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn first_attempt(path: &Path) -> bool {
    ATTEMPTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(path.to_path_buf())
}

impl AuroraEngine {
    pub fn validate(&self) -> Result<Vec<String>> {
        let missing = self.validate_builtins()?;
        if missing.is_empty() {
            info!("Validation passed, all required files are present");
        } else {
            warn!("Validation found missing files: {}", missing.join(", "));
        }
        Ok(missing)
    }

    pub fn validate_builtins(&self) -> Result<Vec<String>> {
        let mut missing = validate_files(
            self.bin_path.clone(),
            vec![Target::AsiPlugin.as_file().to_string()],
        )?;

        for file in self
            .managed_files()
            .into_iter()
            .filter(|f| f.group == FileGroup::LoaderDll)
            .filter(|f| !f.source.exists())
        {
            let name = file
                .source
                .file_name()
                .map_or_else(|| file.label.clone(), |n| n.to_string_lossy().to_string());
            if !missing.contains(&name) {
                missing.push(name);
            }
        }

        self.repair_censorship_files();

        let crr = config::get(config::key::CENSORSHIP_REMOVE)
            .as_bool()
            .unwrap_or(false);
        if crr {
            missing.extend(
                self.targets
                    .iter()
                    .filter(|(t, _)| *t != Target::AsiPlugin)
                    .filter(|(t, _)| !self.asi_source(*t).exists())
                    .map(|(t, _)| t.as_file().to_string()),
            );
        }

        Ok(missing)
    }

    pub(super) fn repair_censorship_files(&self) {
        let enabled = config::get(config::key::CENSORSHIP_REMOVE)
            .as_bool()
            .unwrap_or(false);
        if !enabled {
            return;
        }

        let folder = self.addons_path.join(CENSORSHIP_DIR);
        for (target, _) in self
            .targets
            .iter()
            .filter(|(t, _)| matches!(t, Target::AuroraTf | Target::CNAuroraTF))
        {
            let source = self.asi_source(*target);
            if source.exists() || !first_attempt(&source) {
                continue;
            }

            match repair_file(&folder, target.as_file()) {
                Ok(()) => info!("Addon repair: restored '{}'", source.display()),
                Err(e) => warn!(
                    "Addon repair: could not restore '{}': {e}",
                    source.display()
                ),
            }
        }
    }
}
