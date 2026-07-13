use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use log::{error, info, trace};

use super::AuroraEngine;

impl AuroraEngine {
    pub fn sanitize(&self, stop_processes: bool) -> Result<()> {
        info!("Starting system sanitization");
        if stop_processes {
            trace!("Killing processes");
            self.kill_nte_processes()?;
        }

        let mut targets: Vec<(String, PathBuf)> = self
            .managed_files()
            .into_iter()
            .map(|f| (f.label, f.destination))
            .collect();
        targets.push((
            "AuroraThirdParty".to_string(),
            self.win64.join("AuroraThirdParty"),
        ));

        for (label, path) in targets {
            Self::remove_target(&label, &path);
        }

        Ok(())
    }

    fn remove_target(label: &str, path: &Path) {
        if !path.exists() {
            return;
        }

        if path.is_file() {
            let Ok(metadata) = fs::metadata(path) else {
                error!("Failed to read metadata for {}", path.display());
                return;
            };
            let mut perms = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            if let Err(e) = fs::set_permissions(path, perms) {
                error!("Failed to set permissions for {}: {e}", path.display());
                return;
            }

            match fs::remove_file(path) {
                Ok(()) => info!("Removed {label} ({})", path.display()),
                Err(e) => error!("Failed to remove {}: {e}", path.display()),
            }
        } else if path.is_dir() || path.is_symlink() {
            match fs::remove_dir_all(path) {
                Ok(()) => info!("Removed {label} ({})", path.display()),
                Err(e) => error!("Failed to remove {}: {e}", path.display()),
            }
        }
    }
}
