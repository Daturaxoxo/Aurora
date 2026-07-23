use anyhow::Result;
use shared::classes::info::Target;

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
        if self.crr {
            required.extend(
                self.targets
                    .iter()
                    .filter(|(t, _)| *t != Target::AsiPlugin)
                    .map(|(t, _)| t.as_file().to_string()),
            );
        }

        validate_files(self.bin_path.clone(), required)
    }
}
