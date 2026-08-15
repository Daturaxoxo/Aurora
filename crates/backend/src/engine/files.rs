use std::collections::BTreeMap;
use std::path::PathBuf;

use log::*;
use shared::classes::info::{addons, Target};
use shared::config::{get, key};

use crate::classes::addons::pak::PakAddon;
use crate::classes::addons::CENSORSHIP_DIR;

use super::AuroraEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileGroup {
    LoaderDll,
    SignatureBypass,
    PakAddon,
}

#[derive(Debug, Clone)]
pub struct ManagedFile {
    pub label: String,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub required: bool,
    pub enabled: bool,
    pub group: FileGroup,
    pub addon: Option<String>,
}

impl AuroraEngine {
    pub fn managed_files(&self) -> Vec<ManagedFile> {
        let mut files = self.loader_dll_files();
        files.extend(self.signature_bypass_files());
        files.extend(self.pak_addon_files());
        files
    }

    fn loader_dll_files(&self) -> Vec<ManagedFile> {
        self.gpaths
            .all_dll_targets()
            .into_iter()
            .filter_map(|(label, destination)| {
                let name = if let Some(n) = destination.file_name() {
                    n.to_owned()
                } else {
                    error!("DLL target has no file name: {}", destination.display());
                    return None;
                };
                debug!(
                    "Adding loader DLL target: {} -> {}",
                    label,
                    destination.display()
                );
                Some(ManagedFile {
                    label,
                    source: self.bin_path.join("Wrappers").join(&name),
                    destination,
                    required: true,
                    enabled: true,
                    group: FileGroup::LoaderDll,
                    addon: None,
                })
            })
            .collect()
    }

    pub(super) fn asi_source(&self, target: Target) -> PathBuf {
        match target {
            Target::AuroraTf | Target::CNAuroraTF => {
                self.addons_path.join(CENSORSHIP_DIR).join(target.as_file())
            }
            Target::AsiPlugin | Target::Cutils => self.bin_path.join(target.as_file()),
        }
    }

    fn signature_bypass_files(&self) -> Vec<ManagedFile> {
        let crr = get(key::CENSORSHIP_REMOVE).as_bool().unwrap_or(false)
            && !self.addon_unavailable(key::CENSORSHIP_REMOVE);

        self.targets
            .iter()
            .map(|(target, destination)| {
                let is_asi_plugin = *target == Target::AsiPlugin;
                let source = self.asi_source(*target);
                let censorship_active = crr && source.exists();
                if crr && !is_asi_plugin && !source.exists() {
                    debug!(
                        "Censorship remover is enabled but '{}' is not installed; skipping.",
                        source.display()
                    );
                }
                ManagedFile {
                    label: target.as_file().to_string(),
                    source,
                    destination: destination.clone(),
                    required: is_asi_plugin,
                    enabled: is_asi_plugin || censorship_active,
                    group: FileGroup::SignatureBypass,
                    addon: None,
                }
            })
            .collect()
    }

    fn addon_unavailable(&self, config_key: &str) -> bool {
        let unavailable = addons::is_unavailable(config_key, self.version);
        if unavailable {
            info!(
                "Addon '{config_key}' is unavailable on {}, skipping",
                self.version
            );
        }
        unavailable
    }

    fn pak_addon_files(&self) -> Vec<ManagedFile> {
        PakAddon::get_pak_addons()
            .into_iter()
            .flat_map(|addon| {
                let enabled = get(&addon.config_key).as_bool().unwrap_or_else(|| {
                    // TODO: old behaviour aborted injection entirely in this case,
                    // imo warning without failing is better @daturas
                    warn!(
                        "Could not read config key '{}' for PAK addon '{}', treating it as disabled",
                        addon.config_key, addon.base_name
                    );
                    false
                }) && !self.addon_unavailable(&addon.config_key);

                addon
                    .resolve(&self.pak_dir)
                    .into_iter()
                    .map(move |resolved| ManagedFile {
                        label: resolved.file_name.clone(),
                        source: self.addons_path.join(resolved.to_folder_name()).join(&resolved.file_name),
                        destination: resolved.path,
                        required: false,
                        enabled,
                        group: FileGroup::PakAddon,
                        addon: Some(addon.base_name.clone()),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

pub(super) fn group_by_addon(files: &[ManagedFile]) -> BTreeMap<&str, Vec<&ManagedFile>> {
    let mut groups: BTreeMap<&str, Vec<&ManagedFile>> = BTreeMap::new();
    for f in files.iter().filter(|f| f.group == FileGroup::PakAddon) {
        if let Some(addon) = &f.addon {
            groups.entry(addon.as_str()).or_default().push(f);
        }
    }
    groups
}
