use super::PakEngine;
use super::files::FileGroup;
use crate::classes::addons::{manifest_path, repair_file};
use anyhow::Result;
use log::*;
use shared::classes::games::pak::Activation;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
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
        self.repair_addon_components();

        let mut missing: Vec<String> = Vec::new();
        let mut report = |name: String| {
            if !missing.contains(&name) {
                missing.push(name);
            }
        };

        for component in self.applicable_components() {
            let needed = match component.activation {
                Activation::Required => true,
                Activation::Addon(key) => self.addon_active(key),
                Activation::Optional | Activation::ChecksumGuard => false,
            };
            if needed && !self.component_source(component).exists() {
                report(component.file_name().to_string());
            }
        }

        for file in self
            .managed_files()
            .into_iter()
            .filter(|f| f.group == FileGroup::LoaderDll)
            .filter(|f| !f.source.exists())
        {
            report(
                file.source
                    .file_name()
                    .map_or_else(|| file.label.clone(), |n| n.to_string_lossy().to_string()),
            );
        }

        Ok(missing)
    }

    pub(super) fn repair_addon_components(&self) {
        for component in self.applicable_components() {
            let Activation::Addon(key) = component.activation else {
                continue;
            };
            if !self.addon_active(key) {
                continue;
            }

            let source = self.component_source(component);
            let Some(folder) = source.parent().filter(|f| manifest_path(f).is_some()) else {
                continue;
            };
            if source.exists() || !first_attempt(&source) {
                continue;
            }

            match repair_file(folder, component.file_name()) {
                Ok(()) => info!("Addon repair: restored '{}'", source.display()),
                Err(e) => warn!(
                    "Addon repair: could not restore '{}': {e}",
                    source.display()
                ),
            }
        }
    }
}
