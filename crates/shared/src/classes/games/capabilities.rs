use super::{
    InstallationFacts,
    launch::{LaunchPlan, LaunchRequest, StartMethodDescriptor},
    platform::PlatformPaths,
};

use anyhow::{Result, anyhow};
use log::*;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Enabled,
    Disabled,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModNaming {
    pub extensions: &'static [&'static str],
    pub toggle_extensions: &'static [&'static str],
    pub disabled_suffix: &'static str,
    pub display_suffixes: &'static [&'static str],
}

impl ModNaming {
    fn has_extension(name: &str, extensions: &[&str]) -> bool {
        Path::new(name)
            .extension()
            .is_some_and(|ext| extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
    }

    pub fn classify(&self, name: &str) -> FileState {
        if Self::has_extension(name, self.extensions) {
            return FileState::Enabled;
        }
        match name.strip_suffix(self.disabled_suffix) {
            Some(base) if Self::has_extension(base, self.extensions) => FileState::Disabled,
            _ => FileState::Other,
        }
    }

    pub fn toggled(&self, name: &str, enabled: bool) -> Option<String> {
        match (self.classify(name), enabled) {
            (FileState::Enabled, false) if Self::has_extension(name, self.toggle_extensions) => {
                Some(format!("{name}{}", self.disabled_suffix))
            }
            (FileState::Disabled, true) => {
                name.strip_suffix(self.disabled_suffix).map(str::to_string)
            }
            _ => None,
        }
    }

    pub fn validate(&self, path: &Path) -> Result<()> {
        let name = path.file_name().map_or_default(|n| n.to_string_lossy());
        if self.classify(&name) == FileState::Other {
            return Err(anyhow!(
                "'{name}' is not a supported file, expected {}",
                self.extensions
                    .iter()
                    .map(|e| format!(".{e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(())
    }

    pub fn display_name<'a>(&self, name: &'a str) -> &'a str {
        self.display_suffixes
            .iter()
            .find_map(|suffix| name.strip_suffix(suffix))
            .unwrap_or(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModLayout {
    pub roots: Vec<PathBuf>,
    pub naming: ModNaming,
}

pub trait ModManagerSupport: Send + Sync {
    fn layout(&self, game_root: &Path) -> ModLayout;
    fn naming(&self) -> &ModNaming;

    fn classify_name(&self, name: &str) -> FileState {
        self.naming().classify(name)
    }
    fn toggle_name(&self, name: &str, enabled: bool) -> Option<String> {
        self.naming().toggled(name, enabled)
    }
    fn validate_entry(&self, path: &Path) -> Result<()> {
        self.naming().validate(path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleLayout {
    pub storage: PathBuf,
    pub naming: ModNaming,
}

pub trait ModuleSupport: Send + Sync {
    fn naming(&self) -> &ModNaming;

    fn layout(&self, storage: &Path) -> ModuleLayout {
        ModuleLayout {
            storage: storage.to_path_buf(),
            naming: *self.naming(),
        }
    }

    fn validate_selection(&self, selection: &[PathBuf]) -> Vec<PathBuf> {
        selection
            .iter()
            .filter(|path| {
                let checked = self.naming().validate(path).and_then(|()| {
                    if path.is_file() {
                        Ok(())
                    } else {
                        Err(anyhow!("{} does not exist", path.display()))
                    }
                });
                checked
                    .inspect_err(|e| warn!("Skipping module {}: {e}", path.display()))
                    .is_ok()
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaDeployment {
    pub label: &'static str,
    pub source: PathBuf,
    pub destination: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaLayout {
    pub runtime: PathBuf,
    pub marker: PathBuf,
    pub scripts: PathBuf,
    pub settings: PathBuf,
    pub deploy: Vec<LuaDeployment>,
}

impl LuaLayout {
    pub fn installed(&self) -> bool {
        self.marker.is_file()
    }
}

pub trait LuaSupport: Send + Sync {
    fn layout(&self, payload_root: &Path, game_root: Option<&Path>) -> LuaLayout;
    fn runtime_url(&self) -> &str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharacterCatalogue {
    pub assets: &'static str,
    pub aliases: &'static [(&'static str, &'static str)],
    pub hidden: &'static [&'static str],
    pub renamed: &'static [(&'static str, &'static str)],
}

pub trait CharacterSupport: Send + Sync {
    fn catalogue(&self) -> &CharacterCatalogue;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteCharacter {
    pub name: &'static str,
    pub category: u32,
}

pub trait GameBananaSupport: Send + Sync {
    fn remote_game_id(&self) -> u32;
    fn characters(&self) -> &[RemoteCharacter] {
        &[]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScreenshotSourceKind {
    Game,
    Pictures,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotNesting {
    Flat,
    PerPlayer { subdir: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenshotSource {
    pub kind: ScreenshotSourceKind,
    pub dir: PathBuf,
    pub nesting: ScreenshotNesting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenshotMutation {
    Delete,
    Rename,
}

pub trait ScreenshotSupport: Send + Sync {
    fn sources(
        &self,
        installation: Option<&InstallationFacts>,
        platform: &PlatformPaths,
    ) -> Vec<ScreenshotSource>;
    fn ignored_directories(&self) -> &[&'static str] {
        &[]
    }
    fn permits(&self, mutation: ScreenshotMutation) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IniSetting {
    pub file: &'static str,
    pub section: &'static str,
    pub key: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleSetting {
    pub ini: IniSetting,
    pub min: f64,
    pub max: f64,
}

pub trait UnrealSupport: Send + Sync {
    fn config_locations(&self, platform: &PlatformPaths) -> Vec<PathBuf>;
    fn application_scale(&self) -> Option<ScaleSetting> {
        None
    }
}

pub trait LaunchArgumentsSupport: Send + Sync {
    fn catalogue(&self) -> &'static str;
}
