use std::fs;
use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use log::{error, info, trace, warn};

use super::AuroraEngine;

impl AuroraEngine {
    pub fn sanitize(&self, stop_processes: bool) -> Result<()> {
        info!("Starting system sanitization");
        if stop_processes {
            trace!("Killing processes");
            self.kill_nte_processes()?;
        }

        let handles: Vec<_> = self
            .managed_files()
            .into_iter()
            .map(|f| (f.label, f.destination))
            .chain([
                ("Plugins".to_string(), self.win64.join("Plugins")),
                //("Lua dwmapi.dll".to_string(), self.win64.join("dwmapi.dll")),
                // ("Lua ue4ss folder".to_string(), self.win64.join("ue4ss")),
            ])
            .chain(
                super::everlight::find_logs(&self.win64)
                    .into_iter()
                    .map(|p| ("Everlight log".to_string(), p)),
            )
            .map(|(label, path)| {
                thread::spawn(move || Self::remove_target(&label, &path).map_err(|e| e.to_string()))
            })
            .collect();

        let mut failures: Vec<String> = Vec::new();

        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(e)) => failures.push(e),
                Err(panic) => {
                    error!("Sanitize worker thread panicked: {panic:?}");
                    failures.push(format!("worker thread panicked: {panic:?}"));
                }
            }
        }

        if !failures.is_empty() {
            return Err(anyhow!(
                "Sanitization failed for {} target(s): {}",
                failures.len(),
                failures.join("; ")
            ));
        }

        Ok(())
    }

    fn remove_target(label: &str, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        for attempt in 1..=5 {
            match Self::try_remove(path) {
                Ok(()) => {
                    info!("Removed {label} ({})", path.display());
                    return Ok(());
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
                        return Err(anyhow!("{label} ({}): {e}", path.display()));
                    }
                }
            }
        }

        Ok(())
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
