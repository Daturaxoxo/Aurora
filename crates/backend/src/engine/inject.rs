use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use log::*;
use shared::classes::info::version::StartMethod;
use shared::config::{self, key};

use crate::classes::validate::ensure_dir;
use crate::engine::files::{FileGroup, ManagedFile, group_by_addon};
use crate::engine::lua::LuaManager;

use super::AuroraEngine;

impl AuroraEngine {
    pub fn inject(&mut self, custom_files: Option<Vec<PathBuf>>) -> Result<()> {
        info!("Injecting into NTE...");
        info!("Game path:  {}", self.game_path.display());
        info!("Bin path:   {}", self.bin_path.display());
        info!("Mods path:  {}", self.pak_base.display());

        self.repair_censorship_files();

        let files = self.managed_files();
        Self::check_required(&files)?;

        self.cleanup()?;

        Self::copy_non_addon_files(&files)?;
        self.copy_pak_addons(&files)?;

        if LuaManager::exists(&self.bin_path) {
            info!(
                "UE4SS Lua runtime detected in {}, copying into {}",
                self.bin_path.join("Lua").display(),
                self.win64.display()
            );
            LuaManager::setup(&self.bin_path, &self.win64)?;
        }

        let mut injected = match custom_files {
            Some(custom_files) => self.copy_custom_files(&custom_files)?,
            None => vec![],
        };
        injected.extend(self.copy_chksum_plugin()?);
        Self::record_injected_plugins(&injected);

        self.launch_game()
    }

    fn check_required(files: &[ManagedFile]) -> Result<()> {
        for f in files.iter().filter(|f| f.required) {
            if !f.source.exists() {
                error!(
                    "Missing required Bin file, the following file is required for Aurora to function properly and could not be restored: {}",
                    f.source.display()
                );
                return Err(anyhow!("Missing required Bin file"));
            }
        }
        Ok(())
    }

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
                let source = entries
                    .first()
                    .with_context(|| "Failed to get first pak addon")?
                    .source
                    .display();

                if missing.len() == entries.len() {
                    debug!("PAK Addon '{addon}' is enabled but not installed ({source}), skipping");
                    continue;
                }

                let msg = format!(
                    "PAK Addon '{addon}'. Path: {source}. Missing required files: {}",
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

    fn copy_custom_files(&self, custom_files: &[PathBuf]) -> Result<Vec<PathBuf>> {
        let mut copied = Vec::with_capacity(custom_files.len());

        for file in custom_files {
            info!(
                "Copying custom file {} to {}",
                file.display(),
                self.win64.display()
            );
            let file_name = file
                .file_name()
                .ok_or_else(|| anyhow!("Failed to get file name"))?;
            let destination = self.win64.join(file_name);
            if let Err(e) = fs::copy(file, &destination) {
                error!("Failed to copy custom file {}: {e}", file.display());
                return Err(anyhow!(
                    "Failed to copy custom file {}: {e}",
                    file.display()
                ));
            }
            copied.push(destination);
        }
        Ok(copied)
    }

    fn copy_chksum_plugin(&self) -> Result<Option<PathBuf>> {
        if super::everlight::checksum_ignored() {
            info!("'Ignore Checksum Matching' is enabled, skipping chksum.asi");
            return Ok(None);
        }

        let source = self.bin_path.join("Plugins").join("chksum.asi");
        if !source.exists() {
            warn!(
                "chksum.asi is missing from {}, launching without it",
                source.display()
            );
            return Ok(None);
        }

        let destination = self.win64.join("chksum.asi");

        fs::copy(&source, &destination).map_err(|e| {
            error!(
                "Failed to copy {} to {}: {e}",
                source.display(),
                destination.display()
            );
            anyhow!("Failed to copy chksum.asi: {e}")
        })?;
        trace!("Copied {} to {}", source.display(), destination.display());
        Ok(Some(destination))
    }

    fn record_injected_plugins(destinations: &[PathBuf]) {
        if destinations.is_empty() {
            return;
        }

        let mut paths: Vec<String> = config::get(key::INJECTED_PLUGINS)
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str())
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();

        for destination in destinations {
            let path = destination.to_string_lossy().into_owned();
            if !paths.contains(&path) {
                paths.push(path);
            }
        }

        trace!("Recording {} injected plugin(s)", destinations.len());
        config::set(key::INJECTED_PLUGINS, paths);
    }

    fn launch_game(&self) -> Result<()> {
        let launcher_exe = self.game_path.join(self.gpaths.launcher_process);
        let start_method = StartMethod::from_config();

        let mut args = self.gpaths.launch_args.clone();
        args.extend_from_slice(start_method.launch_args());

        info!("Launching NTE: {}", launcher_exe.display());
        info!("Distribution: {}", self.distribution);
        info!("Start method: {start_method}");
        debug!("Launch arguments: {args:?}");

        #[cfg(target_os = "linux")]
        {
            crate::classes::linux::launch_via_proton(&launcher_exe, &args)?;
            Ok(())
        }

        #[cfg(not(target_os = "linux"))]
        {
            std::process::Command::new(&launcher_exe)
                .args(&args)
                .spawn()
                .map_err(|e| anyhow!("Failed to launch NTE: {e}"))?;
            Ok(())
        }
    }
}
