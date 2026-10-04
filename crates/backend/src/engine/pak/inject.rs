use std::fs;
use anyhow::{Context, Result, anyhow};
use log::*;
use crate::classes::validate::ensure_dir;
use crate::engine::contract::{InjectedPluginRecord, LaunchInput, ModuleSelection, RecordChanges};
use crate::engine::lua::LuaManager;
use super::PakEngine;
use super::files::{FileGroup, ManagedFile, group_by_addon};
const PLUGINS: &[&str] = &["chksum.asi", "ipc.asi"];

impl PakEngine {
    pub fn inject(&mut self, request: &LaunchInput, records: &mut RecordChanges) -> Result<()> {
        info!("Injecting into NTE...");
        info!("Game path:  {}", self.game_path.display());
        info!("Payload:    {}", self.payload.root().display());
        info!("Mods path:  {}", self.pak_base.display());
        info!("Distribution: {}", self.distribution);

        self.deployment = request.deployment.clone();
        self.repair_censorship_files();

        let files = self.managed_files();
        Self::check_required(&files)?;

        let (cleaned, removed) = self.cleanup(&request.injected_plugins);
        records.extend(removed);
        cleaned?;

        Self::copy_non_addon_files(&files)?;
        super::everlight::install_signature(&self.win64)?;
        self.copy_pak_addons(&files)?;
        self.copy_overlay_addons(&files, records);

        if let Some(lua) = self.payload.lua() {
            info!(
                "UE4SS Lua runtime detected in {}, copying into {}",
                lua.display(),
                self.win64.display()
            );
            LuaManager::setup(lua, &self.win64)?;
        }

        self.copy_custom_files(&request.deployment.modules, records)?;
        self.copy_plugins(records)?;

        Self::launch_game(request)
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
            .filter(|f| {
                f.enabled && !matches!(f.group, FileGroup::PakAddon | FileGroup::OverlayAddon)
            })
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

    fn copy_overlay_addons(&mut self, files: &[ManagedFile], records: &mut RecordChanges) {
        let mut copied = Vec::new();

        for f in files
            .iter()
            .filter(|f| f.group == FileGroup::OverlayAddon && f.enabled)
        {
            if !f.source.exists() {continue}

            if f.destination.exists() {
                let msg = format!(
                    "Overlay addon '{}': '{}' already exists in the game folder, leaving it untouched",
                    f.addon.as_deref().unwrap_or_default(),
                    f.label
                );
                warn!("{msg}");
                self.last_addon_warnings.push(msg);
                continue;
            }

            let result = if f.source.is_dir() {
                crate::engine::lua::copy_dir_all(&f.source, &f.destination)
            } else {
                fs::copy(&f.source, &f.destination)
                    .map(|_| ())
                    .map_err(Into::into)
            };

            if f.destination.exists() {
                copied.push(f.destination.clone());
            }

            match result {
                Ok(()) => trace!(
                    "Overlay addon: copied {} to {}",
                    f.source.display(),
                    f.destination.display()
                ),
                Err(e) => {
                    let msg = format!(
                        "Overlay addon '{}': could not copy '{}': {e}",
                        f.addon.as_deref().unwrap_or_default(),
                        f.label
                    );
                    error!("{msg}");
                    self.last_addon_warnings.push(msg);
                }
            }
        }

        if !copied.is_empty() {
            info!("Copied {} overlay addon file(s) to Win64", copied.len());
            for path in copied {
                records.add(InjectedPluginRecord { path });
            }
        }
    }

    fn copy_custom_files(
        &self,
        modules: &[ModuleSelection],
        records: &mut RecordChanges,
    ) -> Result<()> {
        for file in modules.iter().filter(|m| m.enabled).map(|m| &m.path) {
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
            records.add(InjectedPluginRecord { path: destination });
        }
        Ok(())
    }

    fn copy_plugins(&self, records: &mut RecordChanges) -> Result<()> {
        for &plugin in PLUGINS {
            if plugin == "chksum.asi" && self.settings.ignore_checksum {
                info!("'Ignore Checksum Matching' is enabled, skipping {plugin}");
                continue;
            }

            let source = self.payload.plugins().join(plugin);
            if !source.exists() {
                warn!(
                    "{plugin} is missing from {}, launching without it",
                    source.display()
                );
                continue;
            }

            let destination = self.win64.join(plugin);
            fs::copy(&source, &destination).map_err(|e| {
                error!(
                    "Failed to copy {} to {}: {e}",
                    source.display(),
                    destination.display()
                );
                anyhow!("Failed to copy {plugin}: {e}")
            })?;
            trace!("Copied {} to {}", source.display(), destination.display());
            records.add(InjectedPluginRecord { path: destination });
        }

        Ok(())
    }

    fn launch_game(request: &LaunchInput) -> Result<()> {
        let plan = &request.plan;
        info!("Launching NTE: {}", plan.executable.display());
        debug!("Launch arguments: {:?}", plan.arguments);

        #[cfg(target_os = "linux")]
        {
            let steam = shared::classes::games::find_game(request.tag.game_id.as_str())
                .and_then(|game| game.descriptor().steam);
            crate::classes::linux::launch_via_proton(request, steam.as_ref())?;
            Ok(())
        }

        #[cfg(not(target_os = "linux"))]
        {
            std::process::Command::new(&plan.executable)
                .args(&plan.arguments)
                .current_dir(&plan.working_directory)
                .envs(plan.environment.iter().map(|(k, v)| (k, v)))
                .spawn()
                .map_err(|e| anyhow!("Failed to launch NTE: {e}"))?;
            Ok(())
        }
    }
}
