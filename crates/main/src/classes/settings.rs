use crate::MainWindow;

use backend::{
    classes::rpc::DiscordRpc,
    handler::{self, EngineCommand},
};
use log::*;
use shared::{
    config::{self, key},
    utils,
};
pub struct SettingsHandler;

impl SettingsHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let w = window.clone();

        let mut rpc = match DiscordRpc::new(utils::get_current_timestamp()) {
            Ok(rpc) => Some(rpc),
            Err(e) => {
                error!("Failed to create discord rpc: {e}");
                None
            }
        };

        dbg!(rpc.is_some());

        if config::get(key::DISCORD_RPC).as_bool().unwrap_or(true) {
            if let Some(rpc) = rpc.as_mut() {
                rpc.set_idle().unwrap_or_else(|e| {
                    error!("Failed to set idle discord rpc: {e}");
                });
            }
        }

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
                                });
                            }
                        } else if let Some(rpc) = rpc.as_mut() {
                            rpc.clear_activity().unwrap_or_else(|e| {
                                error!("Failed to clear activity discord rpc: {e}");
                            });
                        }
                    }

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
                    _ => {}
                }
            }
        });
    }
}
