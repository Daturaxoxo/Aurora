use std::fs::OpenOptions;
use std::path::PathBuf;
use std::thread::{self};
use std::time::{Duration, Instant};
use std::{env, fs};

use anyhow::{anyhow, Result};
use log::*;
use rayon::prelude::*;
use shared::classes::info::{
    detect_version, get_version_paths, BypassMethod, Target, Version, VersionPaths, CLIENT_PAK_DIR,
    NTE_PROCESSES,
};
use shared::config::{get, key};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::classes::addons::PakAddon;
use crate::classes::validate::{ensure_dir, validate_builtins};

pub struct AuroraEngine {
    pub game_path: PathBuf,
    pub crr: bool,
    pub ndl: bool,
    pub engine_method: BypassMethod,
    pub bin_path: PathBuf,
    pub version: Version,
    pub gpaths: VersionPaths,
    pub win64: PathBuf,
    pub pak_base: PathBuf,
    pub mod_folder: PathBuf,
    pub pak_dir: PathBuf,
    pub main_dlls: Vec<String>,
    pub builtins_path: PathBuf,
    pub targets: Vec<(Target, PathBuf)>,
    pub ndl_targets: Vec<PakAddon>,
    last_addon_warnings: Vec<String>,
}

impl AuroraEngine {
    pub fn new(game_path: impl Into<PathBuf>) -> Result<Self> {
        let game_path = game_path.into();

        let app_dir = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_else(|| env::current_dir().unwrap_or_default());
        trace!("App dir: {}", app_dir.display());

        let crr: bool = get(key::CENSORSHIP_REMOVE)
            .as_bool()
            .ok_or_else(|| anyhow!("Error when reading config: CENSORSHIP_REMOVE"))?;
        trace!("CRR: {crr}");

        let ndl: bool = get(key::NO_DRIVE_LINE)
            .as_bool()
            .ok_or_else(|| anyhow!("Error when reading config: NO_DRIVE_LINE"))?;
        trace!("NDL: {ndl}");

        let version = detect_version(&game_path)?;
        trace!("Game version: {version}");

        let engine_method = get(key::ENGINE_METHOD)
            .as_i64()
            .ok_or_else(|| anyhow!("Error when reading config: ENGINE_METHOD"))?;
        let engine_method = BypassMethod::from_num(engine_method, version)?;
        trace!("Engine method: {engine_method}");

        let bin_path = app_dir.join("Bin");
        trace!("Bin path: {}", bin_path.display());
        if !bin_path.exists() {
            return Err(anyhow!(
                "Engine could not find bin folder: {}",
                bin_path.display()
            ));
        }

        let gpaths = get_version_paths(&game_path, version, engine_method);
        trace!("Game paths: {gpaths:#?}");
        let win64 = gpaths.win64.clone();
        let pak_base = gpaths.pak_base.clone();
        let mod_folder = game_path.join(CLIENT_PAK_DIR);
        let pak_parent = pak_base
            .parent()
            .ok_or_else(|| anyhow!("Engine could not find paks folder: {}", pak_base.display()))?
            .to_path_buf();
        let main_dlls: Vec<String> = gpaths.dll_slots.iter().map(|s| s.name.clone()).collect();
        trace!("Main DLLs: {}", main_dlls.join(", "));
        let builtins_path = bin_path.join("Builtins");

        let targets = if version == Version::CN {
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
        };
        for t in &targets {
            trace!("Target: {}", t.1.display());
        }

        let mut ndl_targets = vec![];
        for addon in PakAddon::get_pak_addons() {
            for file_name in addon.files() {
                ndl_targets.push(PakAddon::new(
                    file_name.clone(),
                    pak_parent.join(file_name).to_str().unwrap().to_string(),
                ));
            }
        }

        Ok(Self {
            game_path,
            crr,
            ndl,
            engine_method,
            bin_path,
            version,
            gpaths,
            win64,
            pak_base,
            mod_folder,
            pak_dir: pak_parent,
            main_dlls,
            builtins_path,
            targets,
            ndl_targets,
            last_addon_warnings: vec![],
        })
    }

    pub fn kill_nte_processes(&self) -> Result<()> {
        let mut system = System::new();

        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
        );

        let mut targets = vec![self.gpaths.launcher_process, self.gpaths.game_process];
        targets.extend(self.gpaths.helper_processes.clone());
        trace!("Processes to kill: {}", targets.join(", "));

        let processes = targets
            .iter()
            .map(|t| {
                system
                    .processes()
                    .iter()
                    .filter(|p| {
                        if p.1.exe().is_none() || p.1.exe().unwrap().is_empty() {
                            return false;
                        }
                        p.1.exe()
                            .unwrap()
                            .file_name()
                            .ok_or_else(|| anyhow!("Error getting file name"))
                            .unwrap()
                            == *t
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        for process in processes {
            for p in process {
                let name = p.1.exe().unwrap().display();
                trace!("Killing process {name}");
                if p.1.kill() {
                    info!("Process {name} killed");
                } else {
                    error!("Process {name} could not be killed");
                }
            }
        }

        for (k, dll_path) in self.gpaths.all_dll_targets() {
            trace!("Checking {}", dll_path.display());
            if dll_path.exists() {
                trace!("{} exists", dll_path.display());
                for _ in 0..5 {
                    // check if file is in use
                    match OpenOptions::new().write(true).open(&dll_path) {
                        Ok(_) => {
                            trace!("{k} is not locked");
                            break;
                        }
                        Err(e) => {
                            trace!("{k} is locked: {e}");
                            warn!("{k} is still locked, Aurora Engine is waiting...");
                            std::thread::sleep(Duration::from_millis(300));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn validate_builtins(&self) -> Result<Vec<String>> {
        let mut required = vec!["ausigbp.asi".to_string()];
        required.extend(self.main_dlls.clone());
        if self.crr {
            for t in &self.targets {
                if t.0 == Target::AsiPlugin {
                    continue;
                }
                required.push(t.0.as_file().to_string());
            }
        }

        validate_builtins(self.bin_path.clone(), required)
    }

    pub fn sanitize(&self, stop_processes: bool) -> Result<()> {
        info!("Starting system sanitization");
        if stop_processes {
            trace!("Killing processes");
            self.kill_nte_processes()?;
        }

        let mut all_targets = vec![];
        let dll_targets = self.gpaths.all_dll_targets();
        all_targets.extend(dll_targets);
        all_targets.extend(
            self.targets
                .iter()
                .cloned()
                .map(|t| (t.0.as_file().to_string(), t.1)),
        );
        all_targets.extend(
            self.ndl_targets
                .iter()
                .cloned()
                .map(|t| (t.config_key, t.base_name.into())),
        );
        for t in all_targets {
            let key = t.0;
            let path = t.1;

            if !path.exists() {
                continue;
            }

            if path.is_file() {
                let mut perms = fs::metadata(&path)?.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(false);
                fs::set_permissions(&path, perms)?;
                fs::remove_file(&path)?;
                info!("Removed {} ({})", key, path.display());
            } else if path.is_dir() || path.is_symlink() {
                fs::remove_dir_all(&path)?;
                info!("Removed {} ({})", key, path.display());
            }
        }

        Ok(())
    }

    pub fn reinit(&mut self, game_path: impl Into<PathBuf>) -> Result<()> {
        let game_path = game_path.into();
        info!("Reinitializing engine with path: {}", game_path.display());

        let new_version = detect_version(&game_path)?;
        let new_gpaths = get_version_paths(&game_path, new_version, self.engine_method);

        self.game_path = game_path;
        self.version = new_version;
        self.gpaths = new_gpaths.clone();
        self.win64 = new_gpaths.win64;
        self.pak_base = new_gpaths.pak_base;
        self.mod_folder = self.game_path.join(CLIENT_PAK_DIR);
        self.pak_dir = self
            .pak_base
            .parent()
            .ok_or_else(|| anyhow!("Pak base has no parent"))?
            .to_path_buf();
        self.main_dlls = new_gpaths
            .dll_slots
            .iter()
            .map(|d| d.name.clone())
            .collect();
        self.last_addon_warnings = vec![];

        let targets = if self.version == Version::CN {
            vec![
                (Target::AsiPlugin, self.gpaths.asi_plugin.clone()),
                (
                    Target::CNntfrmain,
                    self.win64.join(Target::CNntfrmain.as_file()),
                ),
                (Target::Ntfrsub, self.win64.join(Target::Ntfrsub.as_file())),
                (Target::Cutils, self.win64.join(Target::Cutils.as_file())),
            ]
        } else {
            vec![
                (Target::AsiPlugin, self.gpaths.asi_plugin.clone()),
                (
                    Target::GLntfrmain,
                    self.win64.join(Target::GLntfrmain.as_file()),
                ),
                (Target::Cutils, self.win64.join(Target::Cutils.as_file())),
            ]
        };
        self.targets = targets;

        let mut ndl_targets = vec![];
        for addon in PakAddon::get_pak_addons() {
            for file_name in addon.files() {
                ndl_targets.push(PakAddon::new(
                    file_name.clone(),
                    self.pak_base
                        .parent()
                        .ok_or_else(|| anyhow!("Pak base has no parent"))?
                        .join(file_name)
                        .to_str()
                        .ok_or_else(|| anyhow!("Failed to get file name"))?
                        .to_string(),
                ));
            }
        }

        self.ndl_targets = ndl_targets;

        Ok(())
    }

    pub fn inject(&mut self) -> Result<()> {
        info!("Injecting into NTE...");
        info!("Game path:  {}", self.game_path.display());
        info!("Bin path:   {}", self.bin_path.display());
        info!("Mods path:  {}", self.mod_folder.display());

        let mut req_bin = vec![self.bin_path.join(Target::AsiPlugin.as_file())];
        for dll in &self.main_dlls {
            req_bin.push(self.bin_path.join(dll));
        }

        if self.crr {
            for t in &self.targets {
                if t.0 != Target::AsiPlugin {
                    req_bin.push(self.bin_path.join(t.0.as_file()));
                }
            }
        }

        for file in req_bin {
            if !file.exists() {
                // TODO: Maybe we could instead try to redownload any missing files?
                error!("Missing required Bin file, the following file is required for Aurora to function properly: {}", file.display());
                return Err(anyhow!("Missing required Bin file"));
            }
        }

        self.sanitize(true)?;

        info!(
            "Copying loader DLL(s) {} to game directories...",
            self.main_dlls.join(", ")
        );
        let mut copies = vec![];
        for (_, dst_path) in self.gpaths.all_dll_targets() {
            let src = self.bin_path.join(
                dst_path
                    .file_name()
                    .ok_or_else(|| anyhow!("Failed to get file name"))?,
            );
            ensure_dir(
                dst_path
                    .parent()
                    .ok_or_else(|| anyhow!("Failed to get parent"))?
                    .to_path_buf(),
            )?;
            copies.push((src, dst_path));
        }

        copies
            .into_par_iter()
            .try_for_each(|(src, dst)| -> Result<()> {
                fs::copy(&src, &dst).map_err(|e| {
                    error!(
                        "Failed to copy {} to {}: {}",
                        src.display(),
                        dst.display(),
                        e
                    );
                    e
                })?;
                trace!("Copied {} to {}", src.display(), dst.display());
                Ok(())
            })?;

        info!("Initializing Signature Bypasser...");

        let dst = &self
            .targets
            .iter()
            .find(|t| t.0 == Target::AsiPlugin)
            .ok_or_else(|| anyhow!("Failed to find AsiPlugin"))?
            .1;
        fs::copy(self.bin_path.join(Target::AsiPlugin.as_file()), dst)?;
        trace!(
            "Copied {} to {}",
            self.bin_path.join(Target::AsiPlugin.as_file()).display(),
            dst.display()
        );

        // Censorship remover
        if self.crr {
            info!("Censorship Remover is enabled, copying censorship patching files.");
            for (key, dst_path) in &self.targets {
                if key == &Target::AsiPlugin {
                    continue;
                }
                fs::copy(self.bin_path.join(key.as_file()), dst_path)?;
                trace!(
                    "Copied {} to {}",
                    self.bin_path.join(key.as_file()).display(),
                    dst_path.display()
                );
                info!("Copied censorship-remover files");
            }
        }

        // TODO: seems like dead code that does nothing in the python version? @daturas
        // let seen_folders = vec![];
        // let folders = vec![];
        // let mod_folder_entries = Walk::new(&self.mod_folder, None)?
        //     .follow_links(false)
        //     .collect()?;
        // for dir in mod_folder_entries.dirs() {
        //     for file in mod_folder_entries.files() {
        //         if !file.ends_with(".pak") {
        //             continue;
        //         }

        //         trace!("Found mod file: {} in folder {}", file, dir);
        //         todo!()
        //     }
        // }

        let mut addon_warnings = vec![];
        for addon in PakAddon::get_pak_addons() {
            if !get(&addon.config_key)
                .as_bool()
                .ok_or_else(|| anyhow!(""))?
            {
                continue;
            }
            let mut missing = vec![];
            for file in addon.files() {
                if !self.bin_path.join("Builtins").join(&file).exists() {
                    missing.push(file);
                }
            }

            if !missing.is_empty() {
                let msg = format!(
                    "PAK Addon '{}': missing Bin/Builtins files: {}",
                    addon.base_name,
                    missing.join(", ")
                );
                error!("{msg}");
                addon_warnings.push(msg);
                continue;
            }

            for file in addon.files() {
                fs::copy(
                    self.bin_path.join("Builtins").join(&file),
                    self.pak_dir.join(file),
                )?;
            }
            info!("PAK Addon '{}': copied successfully", addon.base_name);
        }

        self.last_addon_warnings = addon_warnings;

        let launcher_exe = self.game_path.join(self.gpaths.launcher_process);
        info!("Launching NTE: {}", launcher_exe.display());
        std::process::Command::new(&launcher_exe)
            .spawn()
            .map_err(|e| anyhow!("Failed to launch NTE: {e}"))?;
        Ok(())
    }

    pub fn monitor(&mut self) -> Result<()> {
        const MAX_GRACE: i32 = 5;

        let mut missing = 0;
        let mut seen = false;

        let mut system = System::new();

        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
        );

        let game_process = self.gpaths.game_process.to_lowercase();
        let launcher_process = self.gpaths.launcher_process.to_lowercase();
        let helper_processes = self
            .gpaths
            .helper_processes
            .iter()
            .map(|p| p.to_lowercase())
            .collect::<Vec<_>>();
        info!("Helper processes: {}", helper_processes.join(", "));

        info!("Monitoring for NTE, you must press \"Play\" in the launcher!");

        'outer: loop {
            thread::sleep(Duration::from_secs(1));
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
            );

            let mut launcher_found_this_tick = false;
            for p in system.processes().values() {
                if p.exe().is_none() || p.exe().unwrap().is_empty() {
                    continue;
                }

                let exe = p.exe().unwrap().to_string_lossy().to_lowercase();

                if exe.contains(&game_process) {
                    info!(
                        "NTE process ({}) was detected, game is running.",
                        self.gpaths.game_process
                    );
                    // TODO:
                    error!("UNIMPLEMENTED: on game started");
                    break 'outer;
                }

                if exe.contains(&launcher_process)
                    || helper_processes.iter().any(|p| exe.contains(p))
                {
                    launcher_found_this_tick = true;
                    if !seen {
                        info!("NTE Launcher activity detected.");
                        seen = true;
                        // TODO:
                        error!("UNIMPLEMENTED: on launcher detected");
                    } else if missing > 0 {
                        info!("NTE Launcher activity re-detected. Resetting grace tracker.");
                        missing = 0;
                    }
                }
            }

            if !launcher_found_this_tick {
                if !seen {
                    continue;
                }

                missing += 1;
                if missing == 1 {
                    warn!("NTE Launcher process not detected");
                }

                if missing >= MAX_GRACE {
                    warn!("NTE Launcher failed to resolve within {MAX_GRACE}s of continuous absence. Aborting monitor.");
                    self.sanitize(true)?;
                    return Ok(());
                }
            }
        }

        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
        );

        let ht_procs = system
            .processes()
            .iter()
            .filter(|(_, p)| {
                if p.exe().is_none() || p.exe().unwrap().is_empty() {
                    return false;
                }

                let exe = p.exe().unwrap().to_string_lossy().to_lowercase();
                exe == game_process
            })
            .collect::<Vec<_>>();

        if ht_procs.is_empty() {
            loop {
                system.refresh_processes_specifics(
                    ProcessesToUpdate::All,
                    true,
                    ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
                );

                if !system.processes().iter().any(|(_, p)| {
                    p.exe().is_some_and(|path| {
                        let exe_str = path.to_string_lossy().to_lowercase();
                        !exe_str.is_empty() && exe_str.ends_with(&game_process)
                    })
                }) {
                    break;
                }

                thread::sleep(Duration::from_millis(500));
            }
        } else {
            let proc = ht_procs[0];
            proc.1
                .wait()
                .ok_or_else(|| anyhow!("Could not wait for NTE process"))?;
        }

        info!("NTE was closed, initializing clean-up process...");
        self.sanitize(false)?;

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut needs_kill = false;
        while Instant::now() < deadline {
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
            );

            if !system.processes().iter().any(|(_, p)| {
                p.exe().is_some_and(|path| {
                    let exe_str = path.to_string_lossy().to_lowercase();
                    !exe_str.is_empty() && NTE_PROCESSES.iter().any(|target| exe_str == *target)
                })
            }) {
                needs_kill = false;
                break;
            }
            needs_kill = true;
            thread::sleep(Duration::from_millis(500));
        }

        if needs_kill {
            warn!("Processes did not close within 5 seconds. Force killing...");
            self.kill_nte_processes()?;
        }

        Ok(())
    }
}
