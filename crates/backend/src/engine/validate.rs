use anyhow::Result;
use shared::classes::info::Target;

use crate::classes::validate::validate_builtins;

use super::AuroraEngine;

impl AuroraEngine {
    // TODO: This isn't used anywhere, would have to see where it's used
    // in the python version to see if it's dead code or not.
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

        validate_builtins(self.bin_path.clone(), required)
    }
}
