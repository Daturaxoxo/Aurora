use std::path::PathBuf;
use log::*;
use serde_json::Value;
use super::InstallationFacts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartMethodConfirmation {
    pub title: &'static str,
    pub message: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartMethodDescriptor {
    pub id: &'static str,
    pub label: &'static str,
    pub opens_launcher: bool,
    pub confirmation: Option<StartMethodConfirmation>,
}

fn parse_index(raw: &Value) -> Option<usize> {
    raw.as_u64()
        .or_else(|| raw.as_str().and_then(|s| s.parse().ok()))
        .and_then(|i| usize::try_from(i).ok())
}

pub fn select_start_method(
    methods: &[StartMethodDescriptor],
    raw: &Value,
    default: &Value,
) -> Option<(usize, StartMethodDescriptor)> {
    let in_range = |i: &usize| *i < methods.len();
    let index = parse_index(raw).filter(in_range).or_else(|| {
        debug!("Unreadable start_method {raw:?}, using the default {default:?}");
        parse_index(default).filter(in_range)
    });
    let index = index.or_else(|| (!methods.is_empty()).then_some(0))?;
    Some((index, methods[index]))
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
