use crate::classes::toast::ToastHandler;
use crate::{CheckboxItem, MainWindow};
use backend::handler::{self, EngineCommand};
use log::*;
use shared::{
    classes::info::paths::CLIENT_PAK_DIR, pathfind::get_game_directory, utils::open_folder,
};
use slint::VecModel;

pub struct ButtonHandler;

impl ButtonHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        // Icon Row
        let w = window.clone();
        window.unwrap().on_bottom_icon_clicked(move |index| {
            if let Some(w) = w.upgrade() {
                #[allow(clippy::match_same_arms)]
                match index {
                    0 => w.set_show_menu(!w.get_show_menu()),
                    1 => w.set_show_mod_manager(!w.get_show_mod_manager()),
                    2 => {} // handled by popup.rs: open discord
                    3 => {} // handled by popup.rs: open gamebanana
                    _ => {}
                }
            }
        });

        // Launch Menu
        let w = window.clone();
        window.unwrap().on_launch_menu_action(move |index| {
            let w = w.clone();
            match index {
                0 => Self::repair_aurora(&w),
                1 => Self::check_for_updates(&w),
                2 => Self::add_desktop_shortcut(&w),
                4 => Self::open_mods_folder(&w),
                5 => Self::kill_game(),
                _ => {}
            }
        });
    }

    fn open_mods_folder(window: &slint::Weak<MainWindow>) {
        let path = match get_game_directory() {
            Ok(path) => path,
            Err(e) => {
                error!("Could not find game directory: {e}");
                ToastHandler::show(window, "Could not find the game directory.", "error");
                return;
            }
        };

        let mods_path = path.join(CLIENT_PAK_DIR);
        if let Err(e) = std::fs::create_dir_all(&mods_path) {
            error!(
                "Failed to create mods folder {}: {e}",
                mods_path.display()
            );
            ToastHandler::show(window, "Failed to create the mods folder.", "error");
            return;
        }

        if let Err(e) = open_folder(&mods_path) {
            error!("Failed to open mods folder: {e}");
            ToastHandler::show(window, "Failed to open the mods folder.", "error");
        }
    }

    fn repair_aurora(window: &slint::Weak<MainWindow>) {
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                let checkboxes = vec![
                    CheckboxItem {
                        label: "Validate Aurora Files".into(),
                        required: true,
                        checked: true,
                    },
                    CheckboxItem {
                        label: "Clean Cache".into(),
                        required: true,
                        checked: false,
                    },
                    CheckboxItem {
                        label: "Remove Injected Files".into(),
                        required: true,
                        checked: true,
                    },
                ];
                w.set_popup_id("repair".into());
                w.set_popup_title("Repair".into());
                w.set_popup_message("This will repair any issues with Aurora".into());
                w.set_popup_confirm_delay(0);
                w.set_popup_required_count(0);
                w.set_popup_checkboxes(slint::ModelRc::new(VecModel::from(checkboxes)));
                w.set_popup_active(true);
            }
        })
        .ok();
    }

    fn check_for_updates(window: &slint::Weak<MainWindow>) {
        if let Some(w) = window.upgrade() {
            w.set_toast_text("Checking for updates...".into());
            w.set_toast_kind("info".into());
            w.set_toast_active(true);
        }
        crate::classes::updater::UpdateHandler::run_update_check(window, true);
    }

    fn add_desktop_shortcut(window: &slint::Weak<MainWindow>) {
        #[cfg(target_os = "windows")]
        {
            let result = std::env::current_exe()
                .map_err(anyhow::Error::from)
                .and_then(|exe| shared::desktop_entry::create_desktop_shortcut(&exe));

            if let Some(w) = window.upgrade() {
                match result {
                    Ok(()) => {
                        w.set_toast_text("Desktop shortcut created.".into());
                        w.set_toast_kind("success".into());
                    }
                    Err(e) => {
                        error!("Failed to create desktop shortcut: {e}");
                        w.set_toast_text("Failed to create shortcut.".into());
                        w.set_toast_kind("error".into());
                    }
                }
                w.set_toast_active(true);
            }
        }

        #[cfg(not(target_os = "windows"))]
        // we can add an alternative some time if you want alawapr -Daturas
        {
            if let Some(w) = window.upgrade() {
                w.set_toast_text("Shortcuts are only supported on Windows.".into());
                w.set_toast_kind("error".into());
                w.set_toast_active(true);
            }
        }
    }

    fn kill_game() {
        match handler::get_tx() {
            Ok(tx) => {
                if let Err(err) = tx.send(EngineCommand::KillProcesses) {
                    error!("Failed to send kill process command: {err}");
                }
            }
            Err(err) => error!("Failed to get engine command sender: {err}"),
        }
    }
}
