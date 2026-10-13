use std::path::PathBuf;

use anyhow::{Result, anyhow};
use log::*;
use shared::classes::games::{
    Game, InstallationFacts,
    capabilities::LuaLayout,
    find_game,
    pak::{PakLayout, PakSupport},
    payload::ResolvedPayloadLayout,
};

use super::files::FileGroup;
use super::process::ProcessTargets;
use crate::engine::contract::{DeploymentSelection, EngineInput, EngineSettings};

pub struct PakEngine {
    pub game: &'static dyn Game,
    pub pak: &'static dyn PakSupport,
    pub installation: InstallationFacts,
    pub game_path: PathBuf,
    pub payload: ResolvedPayloadLayout,
    pub lua: Option<LuaLayout>,
    pub layout: PakLayout,
    pub loader: &'static str,
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
        let pak = game.pak().ok_or_else(|| {
            anyhow!(
                "{} does not support the pak engine",
                game.descriptor().display_name
            )
        })?;
        trace!("Game variant: {}", installation.variant());
        trace!("Distribution: {}", installation.distribution());

        let loader = pak.loader(installation, input.settings.engine_method)?;
        trace!("Engine loader: {loader}");
        trace!("Payload: {}", input.payload.root().display());

        let layout = pak.layout(installation);
        trace!("Game layout: {layout:#?}");

        let lua = game
            .lua()
            .map(|lua| lua.layout(input.payload.root(), Some(&game_path)));

        info!("Game Path: {}", game_path.display());
        Ok(Self {
            game,
            pak,
            installation: installation.clone(),
            game_path,
            payload: input.payload.clone(),
            lua,
            layout,
            loader,
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
            launcher_process: self.layout.launcher_process,
            game_process: self.layout.game_process,
            helper_processes: self.layout.helper_processes.clone(),
            win64: self.layout.binaries.clone(),
            loader_dlls,
        }
    }
}
