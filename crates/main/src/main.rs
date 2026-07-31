#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod bridge;
mod classes;
mod translations;

use anyhow::{anyhow, Result};
use display_info::DisplayInfo;
use log::*;
use sysinfo::{CpuRefreshKind, RefreshKind, System};

use shared::config::{self, key};
use shared::logger::Logger;

use classes::buttons::ButtonHandler;
use classes::pages::addons::AddonsHandler;
use classes::pages::settings::SettingsHandler;
use classes::popup::PopupHandler;
use classes::toast::ToastHandler;
use classes::updater::UpdateHandler;

use bridge::Bridge;
use slint::{LogicalPosition, WindowPosition};

use crate::classes::pages::gbbrowser::GbBrowserHandler;
use crate::classes::pages::lua::LuaScriptsHandler;
use crate::classes::pages::modmanager::ModManagerHandler;
use crate::classes::pages::modules::ModulesHandler;
use crate::classes::pages::screenshots::ScreenshotHandler;

fn main() -> Result<()> {
    Logger::init().unwrap_or_else(|e| {
        panic!("Logger failed to initialize: {e}");
    });

    std::panic::set_hook(Box::new(|info| {
        error!("PANIC: {info}");
    }));

    let _instance_lock =
        match ipc::lock::SingletonLock::acquire(&ipc::install_root().join(ipc::AURORA_LOCK_FILE)) {
            Ok(Some(lock)) => Some(lock),
            Ok(None) => {
                error!("Another instance of Aurora is already running; exiting.");
                return Ok(());
            }
            Err(e) => {
                warn!("Could not acquire the instance lock: {e}");
                None
            }
        };

    config::set(
        key::APP_LOCATION,
        std::env::current_exe()?.display().to_string(),
    );

    let window = MainWindow::new()?;
    window.set_app_version(format!("v{}", shared::utils::get_local_version().trim()).into());
    let slint_window = window.window();
    let monitor_size = match get_monitor_size() {
        Ok(size) => size,
        Err(e) => {
            error!("Could not get monitor size: {e}");
            return Ok(());
        }
    };

    translations::apply_saved_language(&window);

    let (window_width, window_height) = if monitor_size.width < 1366 {
        (960.0, 540.0)
    } else {
        (1280.0, 720.0)
    };
    info!("Setting window size to {window_width}x{window_height}");
    window.set_initial_width(window_width);
    window.set_initial_height(window_height);
    slint_window.set_size(slint::LogicalSize::new(window_width, window_height));

    #[allow(clippy::cast_precision_loss)]
    slint_window.set_position(WindowPosition::Logical(LogicalPosition::new(
        (monitor_size.width / 2 - slint_window.size().width / 2) as f32,
        (monitor_size.height / 2 - slint_window.size().height / 2) as f32,
    )));

    // DRAGGING
    let window_weak = window.as_weak();
    window.on_window_dragged(move |delta_x, delta_y| {
        let Some(w) = window_weak.upgrade() else {
            return;
        };
        let win = w.window();
        let scale = win.scale_factor();
        let phys = win.position();
        let win_size = win.size();
        let mut new_x = phys.x as f32 / scale + delta_x;
        let mut new_y = phys.y as f32 / scale + delta_y;

        match DisplayInfo::all() {
            Ok(displays) if !displays.is_empty() => {
                let win_w = win_size.width as f32 / scale;

                let min_x = displays.iter().map(|d| d.x).min().unwrap_or(0) as f32;
                let min_y = displays.iter().map(|d| d.y).min().unwrap_or(0) as f32;
                let max_x = displays
                    .iter()
                    .map(|d| d.x + d.width.cast_signed())
                    .max()
                    .unwrap_or(i32::MAX) as f32;
                let max_y = displays
                    .iter()
                    .map(|d| d.y + d.height.cast_signed())
                    .max()
                    .unwrap_or(i32::MAX) as f32;
                let margin = 40.0;
                new_x = new_x.clamp(min_x - win_w + margin, max_x - margin);
                new_y = new_y.clamp(min_y, max_y - margin);
            }
            Ok(_) => warn!("DisplayInfo::all() returned no displays during drag"),
            Err(e) => warn!("Could not query displays during drag: {e}"),
        }

        if !new_x.is_finite() || !new_y.is_finite() {
            error!("Computed non-finite window position during drag ({new_x}, {new_y}), ignoring");
            return;
        }

        win.set_position(WindowPosition::Logical(LogicalPosition::new(new_x, new_y)));
    });

    let window_weak = window.as_weak();
    window.on_minimize_clicked(move || {
        if let Some(w) = window_weak.upgrade() {
            w.window().set_minimized(true);
        }
    });

    let window_weak = window.as_weak();
    window.on_maximize_clicked(move || {
        if let Some(w) = window_weak.upgrade() {
            w.window().set_maximized(!w.window().is_maximized());
        }
    });

    let window_weak = window.as_weak();
    window.on_close_clicked(move || {
        classes::logwindow::hide();
        if let Some(w) = window_weak.upgrade() {
            let _ = w.hide();
        }
    });

    window.window().on_close_requested(|| {
        classes::logwindow::hide();
        slint::CloseRequestResponse::HideWindow
    });

    let s = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(
            CpuRefreshKind::everything()
                .without_cpu_usage()
                .without_frequency(),
        ),
    );

    match rayon::ThreadPoolBuilder::new()
        .num_threads(s.cpus().iter().count() / 2)
        .build_global()
    {
        Ok(()) => (),
        Err(e) => error!("Could not create rayon pool: {e}"),
    }

    ToastHandler::setup(window.as_weak());
    ButtonHandler::setup(&window.as_weak());
    SettingsHandler::setup(&window.as_weak());
    PopupHandler::setup(&window.as_weak());
    UpdateHandler::setup(&window.as_weak());
    AddonsHandler::setup(&window.as_weak());
    ScreenshotHandler::setup(&window.as_weak());
    ModManagerHandler::setup(&window.as_weak());
    ModulesHandler::setup(&window.as_weak());
    GbBrowserHandler::setup(&window.as_weak());

    let bin_dir = std::env::current_exe()?
        .parent()
        .map(|p| p.join("Bin"))
        .ok_or_else(|| anyhow!("could not determine the executable's directory"))?;
    LuaScriptsHandler::setup(&window.as_weak(), &bin_dir);

    Bridge::setup(&window.as_weak());
    Ok(window.run()?)
}

fn get_monitor_size() -> Result<DisplayInfo> {
    let mut last_err = None;

    for attempt in 1..=10 {
        match DisplayInfo::all() {
            Ok(displays) => {
                // Last resort fallback: return the first display found if no primary is found
                if attempt == 10 {
                    return Ok(displays.first().cloned().unwrap());
                }

                if let Some(display) = displays.into_iter().find(|d| d.is_primary) {
                    if attempt > 1 {
                        info!("get_monitor_size: primary monitor found on attempt {attempt}");
                    }
                    return Ok(display);
                }
                info!("get_monitor_size: primary monitor not found after {attempt} attempts.");
            }
            Err(e) => {
                last_err = Some(anyhow!("Failed to get monitor information: {e}"));
            }
        }

        if attempt < 10 {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow!("No primary display found")))
}
