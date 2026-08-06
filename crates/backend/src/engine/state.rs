use std::path::PathBuf;

use anyhow::{anyhow, Result};
use log::*;
use shared::classes::info::{
    paths::{get_version_paths, VersionPaths},
    version::{detect_distribution, detect_version, BypassMethod, Distribution, Version},
    Target,
};
use shared::config::{get, key};
use shared::utils;

use super::files::FileGroup;
use super::process::{set_kill_snapshot, KillSnapshot};

#[derive(Clone)]
pub struct AuroraEngine {
    pub game_path: PathBuf,
    pub engine_method: BypassMethod,
    pub bin_path: PathBuf,
    pub version: Version,
    pub gpaths: VersionPaths,
    pub win64: PathBuf,
    pub pak_base: PathBuf,
    pub pak_dir: PathBuf,
    pub addons_path: PathBuf,
    pub targets: Vec<(Target, PathBuf)>,
    pub distribution: Distribution,
    pub(crate) last_addon_warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct DerivedPaths {
    win64: PathBuf,
    pak_base: PathBuf,
    pak_dir: PathBuf,
    targets: Vec<(Target, PathBuf)>,
}

impl AuroraEngine {
    pub fn new(game_path: impl Into<PathBuf>) -> Result<Self> {
        let game_path = game_path.into();

        let version = detect_version(&game_path)?;
        trace!("Game version: {version}");

        let distribution = detect_distribution(&game_path);
        trace!("Distribution: {distribution:?}");

        let engine_method_raw = match get(key::ENGINE_METHOD).as_i64() {
            Some(v) => v,
            None => get(key::ENGINE_METHOD)
                .as_str()
                .unwrap_or("0")
                .parse::<i64>()?,
        };
        let engine_method = BypassMethod::from_num(engine_method_raw)?;
        trace!("Engine method: {engine_method}");

        let bin_path =
            utils::get_bin_path().ok_or_else(|| anyhow!("Could not resolve bin path"))?;
        trace!("Bin path: {}", bin_path.display());
        if !bin_path.exists() {
            return Err(anyhow!(
                "Engine could not find bin folder: {}",
                bin_path.display()
            ));
        }
        let addons_path = bin_path.join("Addons");

        let gpaths = get_version_paths(&game_path, version, distribution, engine_method);
        trace!("Game paths: {gpaths:#?}");
        let derived = Self::derive_paths(&gpaths)?;

        let engine = Self {
            game_path,
            engine_method,
            bin_path,
            version,
            gpaths,
            win64: derived.win64,
            pak_base: derived.pak_base.clone(),
            pak_dir: derived.pak_dir,
            addons_path,
            targets: derived.targets,
            distribution,
            last_addon_warnings: vec![],
        };
        engine.publish_kill_snapshot();
        Ok(engine)
    }

    pub fn reinit(&mut self, game_path: impl Into<PathBuf>) -> Result<()> {
        let game_path = game_path.into();
        info!("Reinitializing engine with path: {}", game_path.display());

        let engine_method_raw = match get(key::ENGINE_METHOD).as_i64() {
            Some(v) => v,
            None => get(key::ENGINE_METHOD)
                .as_str()
                .unwrap_or("0")
                .parse::<i64>()?,
        };
        let engine_method = BypassMethod::from_num(engine_method_raw)?;
        trace!("Engine method: {engine_method}");

        let version = detect_version(&game_path)?;
        let distribution = detect_distribution(&game_path);
        let gpaths = get_version_paths(&game_path, version, distribution, engine_method);
        let derived = Self::derive_paths(&gpaths)?;

        self.game_path = game_path;
        self.engine_method = engine_method;
        self.version = version;
        self.gpaths = gpaths;
        self.win64 = derived.win64;
        self.pak_base.clone_from(&derived.pak_base);
        self.pak_dir = derived.pak_dir;
        self.targets = derived.targets;
        self.distribution = distribution;
        self.last_addon_warnings = vec![];

        trace!("Finished reinitializing engine");
        self.publish_kill_snapshot();
        Ok(())
    }

    fn publish_kill_snapshot(&self) {
        let loader_dlls = self
            .managed_files()
            .into_iter()
            .filter(|f| f.group == FileGroup::LoaderDll)
            .map(|f| (f.label, f.destination))
            .collect();

        set_kill_snapshot(KillSnapshot {
            launcher_process: self.gpaths.launcher_process,
            game_process: self.gpaths.game_process,
            helper_processes: self.gpaths.helper_processes.clone(),
            loader_dlls,
        });
    }

    fn derive_paths(gpaths: &VersionPaths) -> Result<DerivedPaths> {
        let win64 = gpaths.win64.clone();
        let pak_base = gpaths.pak_base.clone();
        let pak_dir = gpaths
            .pak_dir()
            .ok_or_else(|| anyhow!("Engine could not find paks folder: {}", pak_base.display()))?
            .to_path_buf();
        let targets = vec![
            (Target::AsiPlugin, gpaths.asi_plugin.clone()),
            (Target::AuroraTf, win64.join(Target::AuroraTf.as_file())),
            (Target::Cutils, win64.join(Target::Cutils.as_file())),
        ];

        Ok(DerivedPaths {
            win64,
            pak_base,
            pak_dir,
            targets,
        })
    }
}
