use super::PakEngine;
use crate::classes::addons::overlay;
use crate::classes::addons::pak;
use log::*;
use shared::classes::games::capabilities::{OverlayAddon, PakAddon};
use shared::classes::games::identity::SafeRelativePath;
use shared::classes::games::pak::{Activation, EngineComponent, InstallDir};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileGroup {
    LoaderDll,
    Component,
    PakAddon,
    OverlayAddon,
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

impl PakEngine {
    pub fn managed_files(&self) -> Vec<ManagedFile> {
        let mut files = self.loader_dll_files();
        files.extend(self.component_files());
        files.extend(self.pak_addon_files());
        files.extend(self.overlay_addon_files());
        files
    }

    pub(super) fn overlay_addons(&self) -> &'static [OverlayAddon] {
        self.game
            .addons()
            .map_or(&[], |addons| addons.overlay_addons())
    }

    fn overlay_addon_files(&self) -> Vec<ManagedFile> {
        self.overlay_addons()
            .iter()
            .flat_map(|addon| {
                let enabled = self.addon_active(addon.config_key);

                overlay::entries(self.payload.addons(), addon)
                    .into_iter()
                    .map(move |entry| ManagedFile {
                        label: entry.name.clone(),
                        destination: self.layout.binaries.join(&entry.name),
                        source: entry.source,
                        required: false,
                        enabled: enabled && !entry.disabled,
                        group: FileGroup::OverlayAddon,
                        addon: Some(addon.folder.to_string()),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub(super) fn loader_targets(&self, loader: &str) -> [(String, PathBuf); 2] {
        [
            (format!("{loader}:root"), self.game_path.join(loader)),
            (format!("{loader}:bin"), self.layout.binaries.join(loader)),
        ]
    }

    fn loader_dll_files(&self) -> Vec<ManagedFile> {
        self.loader_targets(self.loader)
            .into_iter()
            .map(|(label, destination)| {
                debug!(
                    "Adding loader DLL target: {} -> {}",
                    label,
                    destination.display()
                );
                ManagedFile {
                    label,
                    source: self.payload.wrappers().join(self.loader),
                    destination,
                    required: true,
                    enabled: true,
                    group: FileGroup::LoaderDll,
                    addon: None,
                }
            })
            .collect()
    }

    pub(super) fn component_source(&self, component: &EngineComponent) -> PathBuf {
        SafeRelativePath::parse(component.source).map_or_else(
            |_| self.payload.root().join(component.source),
            |path| path.resolve_under(self.payload.root()),
        )
    }

    fn install_dir(&self, install: InstallDir) -> &Path {
        match install {
            InstallDir::GameRoot => &self.game_path,
            InstallDir::Binaries => &self.layout.binaries,
        }
    }

    pub(super) fn applicable_components(&self) -> impl Iterator<Item = &'static EngineComponent> {
        self.pak
            .components()
            .iter()
            .filter(|c| c.applies_to(&self.installation))
    }

    pub(super) fn addon_active(&self, config_key: &str) -> bool {
        self.deployment.addon_enabled(config_key) && !self.addon_unavailable(config_key)
    }

    fn component_files(&self) -> Vec<ManagedFile> {
        self.pak
            .components()
            .iter()
            .map(|component| {
                let source = self.component_source(component);
                let applies = component.applies_to(&self.installation);
                let enabled = applies
                    && match component.activation {
                        Activation::Required => true,
                        Activation::Optional => source.exists(),
                        Activation::ChecksumGuard => {
                            !self.settings.ignore_checksum && source.exists()
                        }
                        Activation::Addon(key) => self.addon_active(key) && source.exists(),
                    };
                ManagedFile {
                    label: component.file_name().to_string(),
                    destination: self
                        .install_dir(component.install)
                        .join(component.file_name()),
                    source,
                    required: applies && component.activation == Activation::Required,
                    enabled,
                    group: FileGroup::Component,
                    addon: None,
                }
            })
            .collect()
    }

    fn addon_unavailable(&self, config_key: &str) -> bool {
        let unavailable = self
            .game
            .addons()
            .is_some_and(|addons| !addons.is_available(config_key, Some(&self.installation)));
        if unavailable {
            info!(
                "Addon '{config_key}' is unavailable on {}, skipping",
                self.installation.variant()
            );
        }
        unavailable
    }

    fn pak_addons(&self) -> &'static [PakAddon] {
        self.game.addons().map_or(&[], |addons| addons.pak_addons())
    }

    fn pak_addon_files(&self) -> Vec<ManagedFile> {
        let mut files = Vec::new();
        for addon in self.pak_addons() {
            let enabled = self.addon_active(addon.config_key);
            let source_dir = self.payload.addons().join(addon.folder);

            for base_name in addon.base_names {
                files.extend(pak::file_names(base_name).map(|file_name| ManagedFile {
                    label: file_name.clone(),
                    source: source_dir.join(&file_name),
                    destination: self.layout.paks.join(&file_name),
                    required: false,
                    enabled,
                    group: FileGroup::PakAddon,
                    addon: Some((*base_name).to_string()),
                }));
            }
        }
        files
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
