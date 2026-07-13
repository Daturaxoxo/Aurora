use std::path::PathBuf;

use anyhow::{anyhow, Result};
use log::*;
use shared::classes::info::{
    paths::{get_version_paths, VersionPaths},
    version::{detect_version, BypassMethod, Version},
    Target,
};
use shared::config::{get, key};
use shared::utils;

pub struct AuroraEngine {
    pub game_path: PathBuf,
    pub crr: bool,
    pub engine_method: BypassMethod,
    pub bin_path: PathBuf,
    pub version: Version,
    pub gpaths: VersionPaths,
    pub win64: PathBuf,
    pub pak_base: PathBuf,
    pub pak_dir: PathBuf,
    pub main_dlls: Vec<String>,
    pub addons_path: PathBuf,
    pub targets: Vec<(Target, PathBuf)>,
    pub(crate) last_addon_warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct DerivedPaths {
    win64: PathBuf,
    pak_base: PathBuf,
    pak_dir: PathBuf,
    main_dlls: Vec<String>,
    targets: Vec<(Target, PathBuf)>,
}

impl AuroraEngine {
    pub fn new(game_path: impl Into<PathBuf>) -> Result<Self> {
        let game_path = game_path.into();

        let crr: bool = get(key::CENSORSHIP_REMOVE)
            .as_bool()
            .ok_or_else(|| anyhow!("Error when reading config: CENSORSHIP_REMOVE"))?;
        trace!("CRR: {crr}");

        let version = detect_version(&game_path)?;
        trace!("Game version: {version}");

        let engine_method_raw = get(key::ENGINE_METHOD)
            .as_i64()
            .ok_or_else(|| anyhow!("Error when reading config: ENGINE_METHOD"))?;
        let engine_method = BypassMethod::from_num(engine_method_raw, version)?;
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

        let gpaths = get_version_paths(&game_path, version, engine_method);
        trace!("Game paths: {gpaths:#?}");
        let derived = Self::derive_paths(&gpaths)?;

        Ok(Self {
            game_path,
            crr,
            engine_method,
            bin_path,
            version,
            gpaths,
            win64: derived.win64,
            pak_base: derived.pak_base.clone(),
            pak_dir: derived.pak_dir,
            main_dlls: derived.main_dlls,
            addons_path,
            targets: derived.targets,
            last_addon_warnings: vec![],
        })
    }

    pub fn reinit(&mut self, game_path: impl Into<PathBuf>) -> Result<()> {
        let game_path = game_path.into();
        info!("Reinitializing engine with path: {}", game_path.display());

        let version = detect_version(&game_path)?;
        let gpaths = get_version_paths(&game_path, version, self.engine_method);
        let derived = Self::derive_paths(&gpaths)?;

        self.game_path = game_path;
        self.version = version;
        self.gpaths = gpaths;
        self.win64 = derived.win64;
        self.pak_base = derived.pak_base.clone();
        self.pak_dir = derived.pak_dir;
        self.main_dlls = derived.main_dlls;
        self.targets = derived.targets;
        self.last_addon_warnings = vec![];

        trace!("Finished reinitializing engine");
        Ok(())
    }

    fn derive_paths(gpaths: &VersionPaths) -> Result<DerivedPaths> {
        let win64 = gpaths.win64.clone();
        let pak_base = gpaths.pak_base.clone();
        let pak_dir = gpaths
            .pak_dir()
            .ok_or_else(|| anyhow!("Engine could not find paks folder: {}", pak_base.display()))?
            .to_path_buf();
        let main_dlls = gpaths.dll_slots.iter().map(|s| s.name.clone()).collect();
        let targets = Self::build_targets(gpaths);

        Ok(DerivedPaths {
            win64,
            pak_base,
            pak_dir,
            main_dlls,
            targets,
        })
    }

    fn build_targets(gpaths: &VersionPaths) -> Vec<(Target, PathBuf)> {
        let win64 = &gpaths.win64;
        if gpaths.version == Version::CN {
            vec![
                (Target::AsiPlugin, gpaths.asi_plugin.clone()),
                (Target::CNntfrmain, win64.join(Target::CNntfrmain.as_file())),
                (Target::Ntfrsub, win64.join(Target::Ntfrsub.as_file())),
                (Target::Cutils, win64.join(Target::Cutils.as_file())),
            ]
        } else {
            vec![
                (Target::AsiPlugin, gpaths.asi_plugin.clone()),
                (Target::GLntfrmain, win64.join(Target::GLntfrmain.as_file())),
                (Target::Cutils, win64.join(Target::Cutils.as_file())),
            ]
        }
    }
}
