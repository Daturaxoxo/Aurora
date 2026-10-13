use super::InstallationFacts;
use anyhow::{Result, anyhow};
use std::path::PathBuf;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallDir {
    GameRoot,
    Binaries,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    Required,
    Optional,
    Addon(&'static str),
    ChecksumGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineComponent {
    pub source: &'static str,
    pub install: InstallDir,
    pub activation: Activation,
    pub variants: &'static [&'static str],
}

impl EngineComponent {
    pub const fn new(source: &'static str, activation: Activation) -> Self {
        Self {
            source,
            install: InstallDir::Binaries,
            activation,
            variants: &[],
        }
    }

    pub const fn required(source: &'static str) -> Self {
        Self::new(source, Activation::Required)
    }

    pub const fn optional(source: &'static str) -> Self {
        Self::new(source, Activation::Optional)
    }

    pub const fn addon(source: &'static str, config_key: &'static str) -> Self {
        Self::new(source, Activation::Addon(config_key))
    }

    #[must_use]
    pub const fn installed_to(self, install: InstallDir) -> Self {
        Self { install, ..self }
    }

    #[must_use]
    pub const fn only_on(self, variants: &'static [&'static str]) -> Self {
        Self { variants, ..self }
    }

    pub fn applies_to(&self, installation: &InstallationFacts) -> bool {
        self.variants.is_empty() || self.variants.contains(&installation.variant())
    }

    pub fn file_name(&self) -> &'static str {
        self.source
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(self.source)
    }
}
pub const EVERLIGHT: EngineComponent = EngineComponent::required("Everlight.asi");
pub const CHKSUM: EngineComponent =
    EngineComponent::new("Plugins/chksum.asi", Activation::ChecksumGuard);
pub const IPC: EngineComponent = EngineComponent::optional("Plugins/ipc.asi");
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineLoader {
    pub dll: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PakLayout {
    pub binaries: PathBuf,
    pub paks: PathBuf,
    pub mods: PathBuf,
    pub launcher_process: &'static str,
    pub game_process: &'static str,
    pub helper_processes: Vec<&'static str>,
}

pub trait PakSupport: Send + Sync {
    fn layout(&self, installation: &InstallationFacts) -> PakLayout;
    fn signature(&self) -> &'static [u8];
    fn loaders(&self) -> &[EngineLoader];
    fn loader(&self, installation: &InstallationFacts, engine_method: i64) -> Result<&'static str> {
        let _ = installation;
        usize::try_from(engine_method)
            .ok()
            .and_then(|index| self.loaders().get(index).map(|loader| loader.dll))
            .ok_or_else(|| anyhow!("Invalid engine method: {engine_method}"))
    }
    fn components(&self) -> &[EngineComponent];
    fn incompatible_files(&self) -> &[&'static str] {
        &[]
    }
    fn flagged_paks(&self) -> &[&'static str] {
        &[]
    }
    fn content_version(&self, installation: &InstallationFacts) -> Option<String> {
        let _ = installation;
        None
    }
}
