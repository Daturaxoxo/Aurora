use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{Result, anyhow};
use log::{error, info, trace, warn};
use shared::classes::info::version::BypassMethod;
use shared::config::{self, key};

use super::AuroraEngine;
const INCOMPATIBLE: &[&str] = &["anticensor"];

fn injected_plugins() -> Vec<PathBuf> {
    config::get(key::INJECTED_PLUGINS)
        .as_array()
        .map(|paths| {
            paths
                .iter()
                .filter_map(|v| v.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

fn remove_incompatible(win64: &Path) -> Vec<(String, PathBuf)> {
    fs::read_dir(win64)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
            {
                return None;
            }

            let name = e.file_name().to_string_lossy().to_lowercase();
            INCOMPATIBLE
                .iter()
                .any(|frag| name.contains(frag))
                .then(|| {
                    (
                        format!("Incompatible {}", e.file_name().to_string_lossy()),
                        path,
                    )
                })
        })
        .collect()
}

fn prune_injected_plugins(injected: &[PathBuf]) {
    let remaining: Vec<String> = injected
        .iter()
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();

    if !remaining.is_empty() {
        warn!(
            "{} injected plugin(s) could not be removed, keeping them in the manifest",
            remaining.len()
        );
    }

    config::set(key::INJECTED_PLUGINS, remaining);
}

impl AuroraEngine {
    pub fn sanitize(&self, stop_processes: bool) -> Result<()> {
        info!("Starting system sanitization");
        if stop_processes {
            trace!("Killing processes");
            self.kill_nte_processes()?;
        }

        let injected = injected_plugins();

        let mut targets: Vec<(String, PathBuf)> = self
            .managed_files()
            .into_iter()
            .map(|f| (f.label, f.destination))
            .chain([
                ("Lua dwmapi.dll".to_string(), self.win64.join("dwmapi.dll")),
                ("Lua ue4ss folder".to_string(), self.win64.join("ue4ss")),
            ])
            .chain(BypassMethod::ALL_DLL_NAMES.iter().flat_map(|dll| {
                [
                    (format!("Wrapper {dll} (root)"), self.game_path.join(dll)),
                    (format!("Wrapper {dll} (bin)"), self.win64.join(dll)),
                ]
            }))
            .chain(
                injected
                    .iter()
                    .map(|p| ("Injected plugin".to_string(), p.clone())),
            )
            .chain(remove_incompatible(&self.win64))
            .collect();
        targets.sort_by(|a, b| a.1.cmp(&b.1));
        targets.dedup_by(|a, b| a.1 == b.1);

        let handles: Vec<_> = targets
            .into_iter()
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

        prune_injected_plugins(&injected);

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
                        let holders = super::locks::holders(path);
                        if !holders.is_empty() {
                            error!("{} is held by: {}", path.display(), holders.join(", "));
                        }
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
