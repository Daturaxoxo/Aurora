use crate::bridge::Bridge;
use crate::classes::pages::addons::AddonsHandler;
use crate::classes::pages::lua::LuaScriptsHandler;
use crate::classes::pages::screenshots::ScreenshotHandler;
use crate::classes::pages::settings::SettingsHandler;
use crate::classes::repair::RepairHandler;
use crate::classes::updater::UpdateHandler;
use crate::{CheckboxItem, MainWindow};

use slint::Model as _;

pub struct PopupHandler;

impl PopupHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let w = window.clone();
        window.unwrap().on_popup_confirm_callback(move |id| {
            if let Some(ww) = w.upgrade() {
                match id.as_str() {
                    "discord-popup" => {
                        let _ = open::that("https://discord.gg/565jfeYsbp");
                    }
                    "gamebanana-popup" => {
                        let _ = open::that("https://gamebanana.com/games/23012");
                    }
                    "screenshot-delete" => {
                        ScreenshotHandler::confirm_delete(&w);
                    }
                    "addon-delete" => {
                        AddonsHandler::confirm_delete(&w);
                    }
                    "update-popup" => {
                        UpdateHandler::start_update(&w);
                    }
                    "lua-delete" => {
                        LuaScriptsHandler::confirm_delete(&w);
                    }
                    "beta-phase-inactive" => {
                        std::process::exit(0);
                    }
                    crate::classes::pages::settings::IGNORE_CHECKSUM_POPUP_ID => {
                        SettingsHandler::confirm_ignore_checksum();
                    }
                    #[cfg(target_os = "linux")]
                    crate::classes::desktop::POPUP_ID => {
                        crate::classes::desktop::apply(true);
                        crate::classes::desktop::mark_prompted();
                        ww.set_desktop_entry(true);
                    }
                    "repair" => {
                        let checkboxes = ww
                            .get_popup_checkboxes()
                            .iter()
                            .collect::<Vec<CheckboxItem>>();
                        let validate_files = checkboxes[0].checked;
                        let clean_cache = checkboxes[1].checked;
                        let remove_files = checkboxes[2].checked;

                        match RepairHandler::repair(validate_files, clean_cache, remove_files) {
                            Ok(()) => {}
                            Err(e) => {
                                Bridge::show_toast(&w, &e.to_string(), "error");
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        let w = window.clone();
        window.unwrap().on_popup_cancel_callback(move |id| {
            if let Some(_w) = w.upgrade() {
                if id.as_str() == "beta-phase-inactive" {
                    std::process::exit(0);
                }

                if id.as_str() == "screenshot-delete" {
                    ScreenshotHandler::cancel_delete();
                }

                if id.as_str() == crate::classes::pages::settings::IGNORE_CHECKSUM_POPUP_ID {
                    SettingsHandler::cancel_ignore_checksum(&w);
                }

                #[cfg(target_os = "linux")]
                if id.as_str() == crate::classes::desktop::POPUP_ID {
                    crate::classes::desktop::mark_prompted();
                }
            }
        });
    }
}
