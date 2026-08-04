#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod bridge;
mod classes;
mod translations;

use anyhow::{anyhow, Result};
use log::*;
use sysinfo::{CpuRefreshKind, RefreshKind, System};

use shared::config::{self, key};
use shared::display::{center_window, get_monitor_size};
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

    #[cfg(target_os = "linux")]
    if is_running_root() {
        error!("Aurora should not be run as root; exiting.");
        return Ok(());
    }

    if let Err(e) = std::fs::create_dir_all(ipc::state_root()) {
        error!("Could not create the state directory: {e}");
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    if let Err(e) = shared::appimage::sync_bin() {
        error!("Could not sync the bundled Bin payload: {e}");
    }

    let _instance_lock = match acquire_instance_lock() {
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

    #[cfg(target_os = "windows")]
    if !config::get(key::QUICK_START_CREATED)
        .as_bool()
        .unwrap_or(false)
    {
        info!("Quick start not created; running first-time setup");
        create_quick_start_shortcut()
            .unwrap_or_else(|e| warn!("Could not create desktop shortcut: {e}"));
    }

    if std::env::args().any(|arg| arg == ipc::QUICK_START_ARG) {
        info!("Quick start requested; running headless launch");
        if let Err(e) = Bridge::quick_start() {
            error!("Quick start failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let window = MainWindow::new()?;

    #[cfg(target_os = "linux")]
    if let Err(e) = slint::set_xdg_app_id(shared::desktop_entry::APP_ID) {
        warn!("Could not set the XDG app id: {e}");
    }

    window.set_app_version(format!("v{}", shared::utils::get_local_version().trim()).into());
    let slint_window = window.window();

    window.set_ui_font_family("Segoe UI".into());
    register_cjk_fallback();
    translations::apply_saved_language(&window);

    let monitor_size = match get_monitor_size() {
        Ok(size) => size,
        Err(e) => {
            error!("Could not get monitor size: {e}");
            return Ok(());
        }
    };

    let (window_width, window_height) = if monitor_size.width < 1366 {
        (960.0, 540.0)
    } else {
        (1280.0, 720.0)
    };
    info!("Setting window size to {window_width}x{window_height}");
    window.set_initial_width(window_width);
    window.set_initial_height(window_height);
    slint_window.set_size(slint::LogicalSize::new(window_width, window_height));

    match center_window(slint_window) {
        Ok(()) => {}
        Err(e) => error!("Could not center window: {e}"),
    }

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

        let (new_x, new_y) = shared::display::on_drag(scale, phys, win_size, delta_x, delta_y);

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
        let _ = slint::quit_event_loop();
    });

    window.window().on_close_requested(|| {
        classes::logwindow::hide();
        let _ = slint::quit_event_loop();
        slint::CloseRequestResponse::HideWindow
    });

    let s = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(
            CpuRefreshKind::everything()
                .without_cpu_usage()
                .without_frequency(),
        ),
    );

    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(s.cpus().iter().count() / 2)
        .build_global();

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

    let bin_dir = shared::utils::get_bin_path()
        .ok_or_else(|| anyhow!("could not determine the Bin directory"))?;
    LuaScriptsHandler::setup(&window.as_weak(), &bin_dir);

    Bridge::setup(&window.as_weak());

    #[cfg(target_os = "linux")]
    classes::desktop::prompt_on_first_run(&window.as_weak());

    window.show()?;
    shared::api::ccu::spawn();
    slint::run_event_loop_until_quit()?;
    shared::api::ccu::stop();
    Ok(())
}

#[cfg(target_os = "windows")]
fn create_quick_start_shortcut() -> Result<()> {
    use mslnk::ShellLink;
    use std::path::PathBuf;

    const QUICK_START_ICON: &[u8] = include_bytes!("../../../production/icons/startup.ico");

    let assets_path = config::get_userdata_path()
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| anyhow!("could not resolve the userdata parent directory"))?;
    std::fs::create_dir_all(&assets_path)?;
    let icon_location = assets_path.join("startup.ico");
    std::fs::write(&icon_location, QUICK_START_ICON)?;

    let exe = std::env::current_exe()?;
    let desktop_dir = dirs::desktop_dir().unwrap_or_else(|| PathBuf::from("."));

    let mut link = ShellLink::new(&exe)?;
    link.set_name(Some("Aurora Quick Start".to_string()));
    link.set_arguments(Some(ipc::QUICK_START_ARG.to_string()));
    link.set_working_dir(
        exe.parent()
            .and_then(|p| p.to_str())
            .map(std::string::ToString::to_string),
    );
    link.set_icon_location(icon_location.to_str().map(std::string::ToString::to_string));
    link.create_lnk(desktop_dir.join("Aurora Quick Start.lnk"))?;

    config::set(key::QUICK_START_CREATED, true);
    info!("Quick start desktop shortcut created");
    Ok(())
}

fn acquire_instance_lock() -> std::io::Result<Option<ipc::lock::SingletonLock>> {
    let path = ipc::state_root().join(ipc::AURORA_LOCK_FILE);
    let relaunched = std::env::args().any(|arg| arg == ipc::RELAUNCH_ARG);

    let attempts = if relaunched { 40 } else { 1 };
    for attempt in 0..attempts {
        match ipc::lock::SingletonLock::acquire(&path)? {
            Some(lock) => return Ok(Some(lock)),
            None if attempt + 1 < attempts => {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            None => {}
        }
    }
    Ok(None)
}

fn register_cjk_fallback() {
    use slint::fontique_010::fontique;
    let font_data = {
        #[cfg(target_os = "windows")]
        {
            std::fs::read("C:/Windows/Fonts/msyh.ttc").ok()
        }

        #[cfg(target_os = "macos")]
        {
            return;
        }

        #[cfg(target_os = "linux")]
        {
            [
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            ]
            .iter()
            .find_map(|p| std::fs::read(p).ok())
        }
    };

    let Some(data) = font_data else {
        warn!("No system CJK font found; CJK glyphs may not render correctly");
        return;
    };

    let blob = fontique::Blob::new(std::sync::Arc::new(data));
    let mut collection = slint::fontique_010::shared_collection();
    let fonts = collection.register_fonts(blob, None);
    for script in ["Hani", "Hans", "Hant"] {
        collection.append_fallbacks(
            fontique::FallbackKey::new(fontique::Script::from_str_unchecked(script), None),
            fonts.iter().map(|x| x.0),
        );
    }

    info!("Registered system CJK font as fallback for Han script");
}

#[cfg(target_os = "linux")]
fn is_running_root() -> bool {
    unsafe { libc::getuid() == 0 }
}
