use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use log::{error, info, trace, warn};

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
        targets.push(("Plugins".to_string(), self.win64.join("Plugins")));
        targets.push(("Lua dwmapi.dll".to_string(), self.win64.join("dwmapi.dll")));
        targets.push(("Lua ue4ss folder".to_string(), self.win64.join("ue4ss")));

        let handles: Vec<_> = targets
            .into_iter()
            .map(|(label, path)| thread::spawn(move || Self::remove_target(&label, &path)))
            .collect();

        for handle in handles {
            if let Err(panic) = handle.join() {
                error!("Sanitize worker thread panicked: {panic:?}");
            }
        }

        Ok(())
    }

    fn remove_target(label: &str, path: &Path) {
        if !path.exists() {
            return;
        }

        for attempt in 1..=5 {
            match Self::try_remove(path) {
                Ok(()) => {
                    info!("Removed {label} ({})", path.display());
                    return;
                }
                Err(e) => {
                    if attempt < 5 {
                        warn!(
                            "Failed to remove {} (attempt {attempt}/5): {e}. Retrying in {}s...",
                            path.display(),
                            Duration::from_secs(1).as_secs()
                        );
                        thread::sleep(Duration::from_secs(1));
                    } else {
                        error!(
                            "Failed to remove {} after 5 attempts, giving up: {e}",
                            path.display()
                        );
                    }
                }
            }
        }
    }

    fn try_remove(path: &Path) -> std::io::Result<()> {
        if path.is_file() {
            let metadata = fs::metadata(path)?;
            let mut perms = metadata.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            fs::set_permissions(path, perms)?;

            fs::remove_file(path)
        } else if path.is_dir() || path.is_symlink() {
            fs::remove_dir_all(path)
        } else {
            Ok(())
        }
    }
}