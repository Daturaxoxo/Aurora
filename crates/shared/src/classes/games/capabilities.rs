use super::{
    InstallationFacts,
    launch::{LaunchPlan, LaunchRequest, StartMethodDescriptor},
};

use anyhow::Result;
use std::path::{Path, PathBuf};

pub trait LauncherSupport: Send + Sync {
    fn start_methods(&self) -> &[StartMethodDescriptor];
    fn launch_plan(&self, request: &LaunchRequest<'_>) -> Result<LaunchPlan>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShippedAddon {
    pub name: &'static str,
    pub config_key: &'static str,
    pub legacy_names: &'static [&'static str],
}

impl ShippedAddon {
    pub fn matches(&self, name: &str) -> bool {
        self.name == name || self.legacy_names.contains(&name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayAddon {
    pub config_key: &'static str,
    pub folder: &'static str,
    pub persist: &'static [&'static str],
}

pub trait AddonSupport: Send + Sync {
    fn shipped_addons(&self) -> &[ShippedAddon];
    fn is_available(&self, config_key: &str, installation: Option<&InstallationFacts>) -> bool;
    fn overlay_addons(&self) -> &[OverlayAddon] {
        &[]
    }
}

// TODO(phase 5, a5-1): add extensions, disabled-name rules and validate_entry
pub trait ModManagerSupport: Send + Sync {
    fn mod_roots(&self, game_root: &Path) -> Vec<PathBuf>;
}
