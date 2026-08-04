use anyhow::Result;
use log::*;
use shared::{classes::info::Target, config};

use crate::classes::validate::validate_files;

use super::files::FileGroup;
use super::AuroraEngine;

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
}
