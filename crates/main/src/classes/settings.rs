use std::{env, fs};

use crate::MainWindow;

use backend::{
    classes::rpc::DiscordRpc,
    handler::{self, EngineCommand},
};
use log::*;
use rfd::FileDialog;
use shared::{
    config::{self, key},
    utils,
};
use slint::{ModelRc, SharedString, VecModel};
pub struct SettingsHandler;

impl SettingsHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let mut rpc = match DiscordRpc::new(utils::get_current_timestamp()) {
            Ok(rpc) => Some(rpc),
            Err(e) => {
                error!("Failed to create discord rpc: {e}");
                // TODO: Probably should display something here @daturas

                None
            }
        };

        if config::get(key::DISCORD_RPC).as_bool().unwrap_or(true) {
            if let Some(rpc) = rpc.as_mut() {
                rpc.set_idle().unwrap_or_else(|e| {
                    error!("Failed to set idle discord rpc: {e}");
                    // TODO: Probably should display something here @daturas
                });
            }
        }

        if !config::get(key::CUSTOM_ADDONS)
            .as_array()
            .unwrap_or(&vec![])
            .is_empty()
        {
            window.unwrap().set_custom_addons(ModelRc::new(
                config::get(key::CUSTOM_ADDONS)
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .map(SharedString::from)
                    .collect::<VecModel<SharedString>>(),
            ));
        }

        let w = window.clone();
        window.unwrap().on_setting_toggled(move |index| {
            if let Some(_w) = w.upgrade() {
                match index {
                    // TODO: Interface minimization
                    0 => {
                        debug!("Interface minimization");
                    }

                    // TODO: Discord RPC
                    1 => {
                        let value = config::get(key::DISCORD_RPC).as_bool().unwrap();
                        config::set(key::DISCORD_RPC, !value);
                        if config::get(key::DISCORD_RPC).as_bool().unwrap() {
                            if let Some(rpc) = rpc.as_mut() {
                                rpc.set_idle().unwrap_or_else(|e| {
                                    error!("Failed to set idle discord rpc: {e}");
                                    // TODO: Probably should display something here @daturas
                                });
                            }
                        } else if let Some(rpc) = rpc.as_mut() {
                            rpc.clear_activity().unwrap_or_else(|e| {
                                error!("Failed to clear activity discord rpc: {e}");
                                // TODO: Probably should display something here @daturas
                            });
                        }
                    }

                    // Censorship toggle
                    2 => {
                        let value = config::get(key::CENSORSHIP_REMOVE).as_bool().unwrap();
                        config::set(key::CENSORSHIP_REMOVE, !value);
                        match handler::get_tx() {
                            Ok(tx) => {
                                tx.send(EngineCommand::Update).ok();
                            }
                            Err(e) => {
                                error!("Failed to send update command to engine: {e}");
                                // TODO: Probably should display something here @daturas
                            }
                        }
                    }

                    // No drive line toggle
                    3 => {
                        let value = config::get(key::NO_DRIVE_LINE).as_bool().unwrap();
                        config::set(key::NO_DRIVE_LINE, !value);
                        match handler::get_tx() {
                            Ok(tx) => {
                                tx.send(EngineCommand::Update).ok();
                            }
                            Err(e) => {
                                error!("Failed to send update command to engine: {e}");
                                // TODO: Probably should display something here @daturas
                            }
                        }
                    }

                    // Hide UID toggle
                    4 => {
                        let value = config::get(key::HIDE_UID).as_bool().unwrap();
                        config::set(key::HIDE_UID, !value);
                        match handler::get_tx() {
                            Ok(tx) => {
                                tx.send(EngineCommand::Update).ok();
                            }
                            Err(e) => {
                                error!("Failed to send update command to engine: {e}");
                                // TODO: Probably should display something here @daturas
                            }
                        }
                    }

                    // Hide notification dots toggle
                    5 => {
                        let value = config::get(key::HIDE_NOTIF_DOTS).as_bool().unwrap();
                        config::set(key::HIDE_NOTIF_DOTS, !value);
                        match handler::get_tx() {
                            Ok(tx) => {
                                tx.send(EngineCommand::Update).ok();
                            }
                            Err(e) => {
                                error!("Failed to send update command to engine: {e}");
                                // TODO: Probably should display something here @daturas
                            }
                        }
                    }

                    // TODO: Developer mode
                    6 => {
                        debug!("Developer mode");
                    }

                    // TODO: Extensive logging
                    7 => {
                        debug!("Extensive logging");
                    }

                    // Custom addons toggle
                    8 => {
                        let value = config::get(key::CUSTOM_ADDONS_TOGGLED)
                            .as_bool()
                            .unwrap_or(false);

                        config::set(key::CUSTOM_ADDONS_TOGGLED, !value);
                    }
                    _ => {}
                }
            }
        });

        let w = window.clone();
        window.unwrap().on_setting_clicked(move |index| {
            if let Some(_w) = w.upgrade() {
                match index {
                    // 0 = browse game directory
                    0 => {
                        // TODO: do something with this
                        let dir = FileDialog::new()
                            .set_directory(env::current_dir().unwrap())
                            .pick_folder();
                        debug!("Game directory: {dir:?}");
                    }
                    // 1 = custom addons clicked
                    1 => {
                        let files = FileDialog::new()
                            .add_filter("Addons", &["dll", "asi"])
                            .add_filter("All Files", &["*"])
                            .set_directory(env::current_dir().unwrap())
                            .pick_files();
                        if let Some(files) = files {
                            let mut paths = vec![];
                            for file in &files {
                                let dst = config::get_userdata_path().join("ThirdParty");
                                if !dst.exists() {
                                    fs::create_dir_all(&dst).ok();
                                }

                                let file_dst = dst.join(file.file_name().unwrap());
                                if let Err(e) = fs::copy(file, &file_dst) {
                                    error!("Failed to copy addon file: {e}");
                                    // TODO: Probably should display something here @daturas
                                } else {
                                    debug!("Copied addon file to: {}", file_dst.display());
                                    paths.push(file_dst);
                                }
                            }
                            debug!("Setting custom addons: {paths:?}");
                            config::set(
                                key::CUSTOM_ADDONS,
                                paths
                                    .iter()
                                    .map(|f| f.display().to_string())
                                    .collect::<Vec<String>>(),
                            );
                        }
                    }
                    // 2 = export telemetry clicked
                    2 => {
                        debug!("Exporting telemetry");
                    }
                    _ => {}
                }
            }
        });
    }
}
