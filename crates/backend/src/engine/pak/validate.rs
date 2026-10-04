use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use log::*;
use shared::{classes::info::Target, config::key};

use crate::classes::addons::{CENSORSHIP_DIR, repair_file};
use crate::classes::validate::validate_files;

use super::PakEngine;
use super::files::FileGroup;

/// Files we have already tried to fetch this session
static ATTEMPTED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn first_attempt(path: &Path) -> bool {
    ATTEMPTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(path.to_path_buf())
}

impl PakEngine {
    pub fn validate_files(&self) -> Result<Vec<String>> {
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
            self.payload.root().to_path_buf(),
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

        if self.deployment.addon_enabled(key::CENSORSHIP_REMOVE) {
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
        if !self.deployment.addon_enabled(key::CENSORSHIP_REMOVE) {
            return;
        }

        let folder = self.payload.addons().join(CENSORSHIP_DIR);
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
