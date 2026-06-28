#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod bridge;
mod classes;

use anyhow::Result;
use display_info::DisplayInfo;
use log::*;

use shared::logger::Logger;

use classes::buttons::ButtonHandler;
use classes::popup::PopupHandler;
use classes::toast::ToastHandler;

use bridge::Bridge;

use crate::classes::settings::SettingsHandler;

fn main() -> Result<()> {
    Logger::init().unwrap_or_else(|e| {
        panic!("Logger failed to initialize: {e}");
    });

    #[cfg(target_os = "linux")]
    ensure_root();

    let window = MainWindow::new()?;
    let slint_window = window.window();
    let monitor_size = get_monitor_size().unwrap();

    if monitor_size.width < 1366 {
        info!("Setting window size to 960x540");
        slint_window.set_size(slint::PhysicalSize::new(960, 540));
    } else {
        info!("Setting window size to 1280x720");
        slint_window.set_size(slint::PhysicalSize::new(1280, 720));
    }

    // DRAGGING
    let window_weak = window.as_weak();
    window.on_window_dragged(move |delta_x, delta_y| {
        if let Some(w) = window_weak.upgrade() {
            let logical_pos = w.window().position();
            #[allow(clippy::cast_precision_loss)]
            w.window()
                .set_position(slint::WindowPosition::Logical(slint::LogicalPosition::new(
                    logical_pos.x as f32 + delta_x,
                    logical_pos.y as f32 + delta_y,
                )));
        }
    });

    let window_weak = window.as_weak();
    window.on_minimize_clicked(move || {
        if let Some(w) = window_weak.upgrade() {
            w.window().set_minimized(true);
        }
    });

    let window_weak = window.as_weak();
    window.on_close_clicked(move || {
        if let Some(w) = window_weak.upgrade() {
            let _ = w.hide();
        }
    });
    ToastHandler::setup(window.as_weak());
    ButtonHandler::setup(&window.as_weak());
    SettingsHandler::setup(&window.as_weak());
    PopupHandler::setup(&window.as_weak());

    Bridge::setup(&window.as_weak());
    Ok(window.run()?)
}

fn get_monitor_size() -> Option<DisplayInfo> {
    DisplayInfo::all()
        .unwrap()
        .into_iter()
        .find(|display| display.is_primary)
}

#[cfg(target_os = "linux")]
fn ensure_root() {
    if unsafe { libc::getuid() } == 0 {
        return;
    }

    let exe = std::env::current_exe().expect("Could not get exe path");
    std::process::exit(
        std::process::Command::new("pkexec")
            .arg(exe)
            .status()
            .expect("Failed to launch pkexec")
            .code()
            .unwrap_or(1),
    );
}
