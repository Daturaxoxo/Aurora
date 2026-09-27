use std::path::PathBuf;

use super::InstallationFacts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartMethodDescriptor {
    pub id: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchPlatform {
    Windows,
    Linux,
}

impl LaunchPlatform {
    pub const fn host() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Windows
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LaunchRequest<'a> {
    pub installation: &'a InstallationFacts,
    pub start_method: &'a str,
    pub platform: LaunchPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatRequirement {
    Proton,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    pub executable: PathBuf,
    pub working_directory: PathBuf,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub compatibility: Option<CompatRequirement>,
    pub dll_overrides: Vec<String>,
}
