use crate::MainWindow;
use log::error;
#[cfg(target_os = "windows")]
use mslnk::ShellLink;
use shared::pathfind::get_game_directory;

pub struct ButtonHandler;

impl ButtonHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        // Icon Row
        let w = window.clone();
        window.unwrap().on_bottom_icon_clicked(move |index| {
            if let Some(w) = w.upgrade() {
                match index {
                    0 => w.set_show_menu(!w.get_show_menu()),
                    1 => {} // undone: mod manager
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
                let mods_path = path.join("Client/WindowsNoEditor/HT/Content/Paks/AuroraMods");
                if let Err(e) = open::that(&mods_path) {
                    error!("Failed to open mods folder: {e}");
                }
            }
            Err(e) => error!("Could not find game directory: {e}"),
        }
    }

    fn repair_aurora(_window: &slint::Weak<MainWindow>) {
        // NOTE: When finishing up, change _window to window (prefixed with _ right now to make sure we don't get any warnings since its not being used) -Daturas
        /*
            Repair Aurora: Used to repair any issues with the application and clean up anything unnecessary
            Pre-Repair: Creates a Popup Dialog
            - 1. Checkbox: Validate Aurora Files | Should Repair validate Bin and Builtins?
            - 2. Checkbox: Clean Cache | Should Aurora clean any Aurora related cache? (GameBanana cache, telemetry data, etc)
            - 3. Checkbox: Remove Injected Files | Should Aurora clean any related files in the game directory?
            Flow:
            - 1. Close any and all NTE processes before doing anything (prompt the user first before doing anything)
            - 2. Validate \Bin (check everything to make sure no DLLs/ASIs are missing, redownload if they are required)
            - 3. Validate \Builtins (check everything to make sure no DLLs/ASIs are missing, don't redownload but warn the user)
            - 4. Validate game path to see if its correct
            - 5. Clean up any aurora related files in the game path (dlls, asi files, plugins [?])
                - NOTE: should also have checks for any old files.
            - 6. Display any warnings, done actions, etc in a final window.
        */
        todo!()
    }

    fn check_for_updates(window: &slint::Weak<MainWindow>) {
        // TODO: hook into the update checker in shared, or something.
        if let Some(w) = window.upgrade() {
            w.set_toast_text("Checking for updates...".into());
            w.set_toast_kind("info".into());
            w.set_toast_active(true);
        }
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
