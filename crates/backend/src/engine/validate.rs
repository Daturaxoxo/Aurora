use anyhow::Result;
use shared::{classes::info::Target, config};

use crate::classes::validate::validate_files;

use super::AuroraEngine;

impl AuroraEngine {
    pub fn validate(&self) -> Result<()> {
        self.validate_builtins()?;
        Ok(())
    }

    pub fn validate_builtins(&self) -> Result<Vec<String>> {
        let mut required = vec![Target::AsiPlugin.as_file().to_string()];
        required.extend(self.main_dlls.clone());

        let mut missing = validate_files(self.bin_path.clone(), required)?;

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
