use crate::{CheckboxItem, MainWindow};
use log::error;
#[cfg(target_os = "windows")]
use mslnk::ShellLink;
use shared::{classes::info::paths::CLIENT_PAK_DIR, pathfind::get_game_directory};
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
                4 => Self::open_mods_folder(),
                _ => {}
            }
        });
    }

    fn open_mods_folder() {
        match get_game_directory() {
            Ok(path) => {
                let mods_path = path.join(CLIENT_PAK_DIR);
                if let Err(e) = open::that(&mods_path) {
                    error!("Failed to open mods folder: {e}");
                }
            }
            Err(e) => error!("Could not find game directory: {e}"),
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
            let result = (|| -> anyhow::Result<()> {
                let exe = std::env::current_exe()?;

                let desktop = dirs::desktop_dir()
                    .ok_or_else(|| anyhow::anyhow!("Could not find desktop directory"))?;

                let shortcut = desktop.join("Aurora.lnk");

                let mut link = ShellLink::new(&exe)?;
                link.set_name(Some("Aurora".to_string()));
                link.set_working_dir(
                    exe.parent()
                        .and_then(|p| p.to_str())
                        .map(std::string::ToString::to_string),
                );
                link.create_lnk(&shortcut)?;

                Ok(())
            })();

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
}
