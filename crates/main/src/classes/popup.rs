use crate::bridge::Bridge;
use crate::classes::pages::screenshots::ScreenshotHandler;
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
                    "update-popup" => {
                        UpdateHandler::start_update(&w);
                    }
                    "beta-phase-inactive" => {
                        std::process::exit(0);
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
                            Ok(_) => {}
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
                match id.as_str() {
                    "beta-phase-inactive" => {
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        });
    }
}
