use super::{
    InstallationFacts,
    launch::{LaunchPlan, LaunchRequest, StartMethodDescriptor},
};

use anyhow::Result;

pub trait LauncherSupport: Send + Sync {
    fn start_methods(&self) -> &[StartMethodDescriptor];
    fn launch_plan(&self, request: &LaunchRequest<'_>) -> Result<LaunchPlan>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShippedAddon {
    pub name: &'static str,
    pub config_key: &'static str,
}

pub trait AddonSupport: Send + Sync {
    fn shipped_addons(&self) -> &[ShippedAddon];
    fn is_available(&self, config_key: &str, installation: Option<&InstallationFacts>) -> bool;
}
