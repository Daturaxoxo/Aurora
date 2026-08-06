#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
include!(concat!(env!("OUT_DIR"), "/main.rs"));

#[cfg(target_os = "windows")]
mod backend;
#[cfg(target_os = "windows")]
mod install;
#[cfg(target_os = "windows")]
mod net;

#[cfg(target_os = "windows")]
use shared::display::{center_window, on_drag};
#[cfg(target_os = "windows")]
use shared::utils::format_bytes;

#[cfg(target_os = "windows")]
use slint::{ComponentHandle, LogicalPosition, ModelRc, SharedString, VecModel, WindowPosition};
#[cfg(target_os = "windows")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "windows")]
use std::rc::Rc;

#[cfg(target_os = "windows")]
fn main() -> Result<(), slint::PlatformError> {
    fn default_install_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        let base = dirs::data_local_dir();
        #[cfg(not(target_os = "windows"))]
        let base = dirs::data_dir();

        base.unwrap_or_else(|| PathBuf::from(".")).join("Aurora")
    }

    fn available_space_for(path: &Path) -> Option<u64> {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        disks
            .list()
            .iter()
            .filter(|d| path.starts_with(d.mount_point()))
            .max_by_key(|d| d.mount_point().as_os_str().len())
            .map(sysinfo::Disk::available_space)
    }

    fn update_available_space(ui: &InstallerWindow) {
        let path = PathBuf::from(ui.get_install_path().as_str());
        let text = match available_space_for(&path) {
            Some(bytes) => format_bytes(bytes),
            None => "Unknown".to_string(),
        };
        ui.set_space_available(text.into());
    }

    let ui = InstallerWindow::new()?;

    let window = ui.window();

    window.set_size(slint::LogicalSize::new(850.0, 580.0));
    match center_window(window) {
        Ok(_) => {}
        Err(e) => eprintln!("Failed to center window: {e}"),
    }

    #[cfg(target_os = "linux")]
    {
        if let Err(e) = slint::BackendSelector::new().select() {
            eprintln!("Could not select the Slint backend: {e}");
        }
        if let Err(e) = slint::set_xdg_app_id(shared::desktop_entry::APP_ID) {
            eprintln!("Could not set the XDG app id: {e}");
        }
    }

    ui.set_is_linux(cfg!(target_os = "linux"));
    if cfg!(target_os = "linux") {
        ui.set_option_desktop_shortcut(false);
    }

    ui.set_install_path(default_install_path().to_string_lossy().into_owned().into());
    ui.set_space_required("Calculating...".into());
    ui.set_total_size("Calculating...".into());
    update_available_space(&ui);

    ui.set_install_log(ModelRc::from(Rc::new(VecModel::<SharedString>::default())));
    install::prepare(ui.as_weak());

    let ui_weak = ui.as_weak();
    ui.on_window_dragged(move |delta_x, delta_y| {
        let Some(w) = ui_weak.upgrade() else {
            return;
        };
        let win = w.window();
        let scale = win.scale_factor();
        let phys = win.position();
        let win_size = win.size();

        let (new_x, new_y) = on_drag(scale, phys, win_size, delta_x, delta_y);

        win.set_position(WindowPosition::Logical(LogicalPosition::new(new_x, new_y)));
    });

    let ui_weak = ui.as_weak();
    ui.on_minimize_clicked(move || {
        if let Some(w) = ui_weak.upgrade() {
            w.window().set_minimized(true);
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_maximize_clicked(move || {
        if let Some(w) = ui_weak.upgrade() {
            w.window().set_maximized(!w.window().is_maximized());
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_close_clicked(move || {
        if let Some(w) = ui_weak.upgrade() {
            let _ = w.hide();
        }
        let _ = slint::quit_event_loop();
    });

    let ui_weak = ui.as_weak();
    ui.on_browse_clicked(move || {
        let ui_weak = ui_weak.clone();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title("Select Installation Folder")
                .pick_folder();

            if let Some(path) = picked {
                let path_str: String = path.to_string_lossy().into_owned();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = ui_weak.upgrade() {
                        w.set_install_path(path_str.into());
                        update_available_space(&w);
                    }
                });
            }
        });
    });

    let ui_weak = ui.as_weak();
    ui.on_reset_path_clicked(move || {
        if let Some(w) = ui_weak.upgrade() {
            w.set_install_path(default_install_path().to_string_lossy().into_owned().into());
            update_available_space(&w);
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_path_edited(move |_path| {
        if let Some(w) = ui_weak.upgrade() {
            update_available_space(&w);
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_back_clicked(move || {
        if let Some(w) = ui_weak.upgrade() {
            let step = w.get_current_step();
            if step == 1 || step == 2 {
                w.set_current_step(step - 1);
            }
        }
    });

    let ui_weak = ui.as_weak();
    ui.on_next_clicked(move || {
        let Some(w) = ui_weak.upgrade() else {
            return;
        };
        match w.get_current_step() {
            0 | 1 => w.set_current_step(w.get_current_step() + 1),
            2 => {
                w.set_current_step(3);
                install::run(
                    w.as_weak(),
                    PathBuf::from(w.get_install_path().as_str()),
                    w.get_option_desktop_shortcut(),
                    w.get_option_start_menu(),
                );
            }
            3 => {
                if w.get_install_done() {
                    w.set_current_step(4);
                }
            }
            4 => {
                if w.get_install_success() && w.get_launch_after() {
                    let exe = PathBuf::from(w.get_install_path().as_str()).join(ipc::AURORA_EXE);
                    if let Err(e) = backend::launch_aurora_elevated(&exe) {
                        eprintln!("Failed to launch Aurora: {e}");
                    }
                }
                let _ = w.hide();
                let _ = slint::quit_event_loop();
            }
            _ => {}
        }
    });

    ui.show()?;
    slint::run_event_loop_until_quit()?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
