use std::path::PathBuf;

use anyhow::{Result, anyhow};
use log::*;
use shared::classes::games::{
    Game,
    capabilities::LuaLayout,
    find_game,
    nte::{
        Target,
        paths::{VersionPaths, get_version_paths},
        version::{BypassMethod, Distribution, Version},
    },
    payload::ResolvedPayloadLayout,
};

use super::files::FileGroup;
use super::process::ProcessTargets;
use crate::engine::contract::{DeploymentSelection, EngineInput, EngineSettings};

pub struct PakEngine {
    pub game: &'static dyn Game,
    pub game_path: PathBuf,
    pub engine_method: BypassMethod,
    pub payload: ResolvedPayloadLayout,
    pub lua: Option<LuaLayout>,
    pub version: Version,
    pub gpaths: VersionPaths,
    pub win64: PathBuf,
    pub pak_base: PathBuf,
    pub pak_dir: PathBuf,
    pub targets: Vec<(Target, PathBuf)>,
    pub distribution: Distribution,
    pub settings: EngineSettings,
    pub(crate) deployment: DeploymentSelection,
    pub(crate) last_addon_warnings: Vec<String>,
}

impl PakEngine {
    pub fn new(input: &EngineInput) -> Result<Self> {
        let installation = input
            .installation
            .as_ref()
            .ok_or_else(|| anyhow!("{} has no game installation", input.tag))?;
        let game_path = installation.root().to_path_buf();
        let game = find_game(input.tag.game_id.as_str())
            .ok_or_else(|| anyhow!("{} is not a registered game", input.tag.game_id))?;

        let version = Version::from_key(installation.variant())
            .ok_or_else(|| anyhow!("unknown game version '{}'", installation.variant()))?;
        trace!("Game version: {version}");

        let distribution =
            Distribution::from_key(installation.distribution()).ok_or_else(|| {
                anyhow!(
                    "unknown game distribution '{}'",
                    installation.distribution()
                )
            })?;
        trace!("Distribution: {distribution:?}");

        let engine_method = BypassMethod::resolve(input.settings.engine_method, version)?;
        trace!("Engine method: {engine_method}");
        trace!("Payload: {}", input.payload.root().display());

        let gpaths = get_version_paths(&game_path, version, distribution, engine_method);
        trace!("Game paths: {gpaths:#?}");

        let win64 = gpaths.win64.clone();
        let pak_base = gpaths.pak_base.clone();
        let pak_dir = gpaths
            .pak_dir()
            .ok_or_else(|| anyhow!("Engine could not find paks folder: {}", pak_base.display()))?
            .to_path_buf();
        let tf = if version == Version::CN {
            Target::CNAuroraTF
        } else {
            Target::AuroraTf
        };
        let targets = vec![
            (Target::AsiPlugin, gpaths.asi_plugin.clone()),
            (tf, win64.join(tf.as_file())),
            (Target::Cutils, win64.join(Target::Cutils.as_file())),
        ];

        let lua = game
            .lua()
            .map(|lua| lua.layout(input.payload.root(), Some(&game_path)));

        info!("Game Path: {}", game_path.display());
        Ok(Self {
            game,
            game_path,
            engine_method,
            payload: input.payload.clone(),
            lua,
            version,
            gpaths,
            win64,
            pak_base,
            pak_dir,
            targets,
            distribution,
            settings: input.settings.clone(),
            deployment: input.deployment.clone(),
            last_addon_warnings: vec![],
        })
    }

    pub(super) fn process_targets(&self) -> ProcessTargets {
        let loader_dlls = self
            .managed_files()
            .into_iter()
            .filter(|f| f.group == FileGroup::LoaderDll)
            .map(|f| (f.label, f.destination))
            .collect();

        let descriptor = self.game.descriptor();
        ProcessTargets {
            game_name: descriptor.display_name,
            processes: descriptor.processes,
            launcher_process: self.gpaths.launcher_process,
            game_process: self.gpaths.game_process,
            helper_processes: self.gpaths.helper_processes.clone(),
            win64: self.win64.clone(),
            loader_dlls,
        }
    }
}
