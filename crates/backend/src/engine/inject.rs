use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use log::*;

use crate::classes::validate::ensure_dir;
use crate::engine::files::{group_by_addon, FileGroup, ManagedFile};

use super::AuroraEngine;

impl AuroraEngine {
    pub fn inject(&mut self, custom_files: Option<Vec<PathBuf>>) -> Result<()> {
        info!("Injecting into NTE...");
        info!("Game path:  {}", self.game_path.display());
        info!("Bin path:   {}", self.bin_path.display());
        info!("Mods path:  {}", self.pak_base.display());

        let files = self.managed_files();
        Self::check_required(&files)?;

        self.sanitize(true)?;

        Self::copy_non_addon_files(&files)?;
        self.copy_pak_addons(&files)?;

        if let Some(custom_files) = custom_files {
            self.copy_custom_files(&custom_files)?;
        }

        self.launch_game()
    }

    fn check_required(files: &[ManagedFile]) -> Result<()> {
        for f in files.iter().filter(|f| f.required) {
            if !f.source.exists() {
                // TODO: Maybe we could instead try to redownload any missing files?
                error!(
                    "Missing required Bin file, the following file is required for Aurora to function properly: {}",
                    f.source.display()
                );
                return Err(anyhow!("Missing required Bin file"));
            }
        }
        Ok(())
    }

    /// Copies every enabled loader-DLL / signature-bypass file. PAK
    /// addons are handled separately by `copy_pak_addons` since they need
    /// per-addon atomicity (see `group_by_addon`), not a flat parallel copy.
    fn copy_non_addon_files(files: &[ManagedFile]) -> Result<()> {
        let to_copy: Vec<&ManagedFile> = files
            .iter()
            .filter(|f| f.enabled && f.group != FileGroup::PakAddon)
            .collect();

        for f in &to_copy {
            if let Some(parent) = f.destination.parent() {
                ensure_dir(&parent.to_path_buf())?;
            }
        }

        info!("Copying {} file(s) to game directories...", to_copy.len());

        to_copy.into_iter().try_for_each(|f| -> Result<()> {
            fs::copy(&f.source, &f.destination).map_err(|e| {
                error!(
                    "Failed to copy {} to {}: {e}",
                    f.source.display(),
                    f.destination.display()
                );
                e
            })?;
            trace!(
                "Copied {} to {}",
                f.source.display(),
                f.destination.display()
            );
            Ok(())
        })
    }

    fn copy_pak_addons(&mut self, files: &[ManagedFile]) -> Result<()> {
        let mut warnings = vec![];

        for (addon, entries) in group_by_addon(files) {
            let enabled = entries.first().is_some_and(|f| f.enabled);
            if !enabled {
                continue;
            }

            let missing: Vec<&str> = entries
                .iter()
                .filter(|f| !f.source.exists())
                .map(|f| f.label.as_str())
                .collect();

            if !missing.is_empty() {
                let msg = format!(
                    "PAK Addon '{addon}'. Path: {}. Missing required files: {}",
                    entries
                        .first()
                        .with_context(|| "Failed to get first pak addon")?
                        .source
                        .display(),
                    missing.join(", ")
                );
                error!("{msg}");
                warnings.push(msg);
                continue;
            }

            for f in &entries {
                fs::copy(&f.source, &f.destination)?;
            }
            info!("PAK Addon '{addon}': copied successfully");
        }

        self.last_addon_warnings = warnings;
        Ok(())
    }

    fn copy_custom_files(&self, custom_files: &[PathBuf]) -> Result<()> {
        let dst_dir = self.win64.join("Plugins");
        if !dst_dir.exists() {
            fs::create_dir(&dst_dir)?;
        }

        for file in custom_files {
            info!(
                "Copying custom file {} to {}",
                file.display(),
                dst_dir.display()
            );
            let file_name = file
                .file_name()
                .ok_or_else(|| anyhow!("Failed to get file name"))?;
            if let Err(e) = fs::copy(file, dst_dir.join(file_name)) {
                error!("Failed to copy custom file {}: {e}", file.display());
                return Err(anyhow!(
                    "Failed to copy custom file {}: {e}",
                    file.display()
                ));
            }
        }
        Ok(())
    }

    fn launch_game(&self) -> Result<()> {
        let launcher_exe = self.game_path.join(self.gpaths.launcher_process);
        info!("Launching NTE: {}", launcher_exe.display());
        std::process::Command::new(&launcher_exe)
            .spawn()
            .map_err(|e| anyhow!("Failed to launch NTE: {e}"))?;
        Ok(())
    }
}
