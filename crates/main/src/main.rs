#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

mod bridge;
mod classes;
mod translations;

use anyhow::{anyhow, Result};
use log::*;
use sysinfo::{CpuRefreshKind, RefreshKind, System};

use backend::classes::addons;
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

    config::migrate();
    addons::migrate();

    #[cfg(target_os = "windows")]
    set_app_user_model_id();

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

    let exe = std::env::current_exe()?;
    let installed = exe
        .parent()
        .is_some_and(|dir| dir.join(ipc::LOCAL_MANIFEST_FILE).is_file());

    #[cfg(target_os = "linux")]
    let app_location = ipc::appimage_path().or_else(|| installed.then(|| exe.clone()));
    #[cfg(not(target_os = "linux"))]
    let app_location = installed.then(|| exe.clone());

    match &app_location {
        Some(path) => config::set(key::APP_LOCATION, path.display().to_string()),
        None => info!(
            "{} is not an installed copy of Aurora; keeping the stored app location",
            exe.display()
        ),
    }

    if std::env::args().any(|arg| arg == ipc::QUICK_START_ARG) {
        info!("Quick start requested; running headless launch");
        if let Err(e) = Bridge::quick_start() {
            error!("Quick start failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    if let Some(app_path) = &app_location {
        if !config::get(key::QUICK_START_CREATED)
            .as_bool()
            .unwrap_or(false)
        {
            info!("Quick start not created; running first-time setup");
            match shared::desktop_entry::create_quick_start_shortcut(app_path) {
                Ok(()) => config::set(key::QUICK_START_CREATED, true),
                Err(e) => warn!("Could not create the quick start shortcut: {e}"),
            }
        }
    }

    let window = MainWindow::new()?;

    #[cfg(target_os = "linux")]
    if let Err(e) = slint::set_xdg_app_id(shared::desktop_entry::APP_ID) {
        warn!("Could not set the XDG app id: {e}");
    }

    window.set_app_version(format!("v{}", shared::utils::get_local_version()).into());
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

    let scale_factor = if monitor_size.scale_factor.is_finite() && monitor_size.scale_factor > 0.0 {
        monitor_size.scale_factor
    } else {
        warn!(
            "Monitor reported an unusable scale factor ({}); assuming 1.0",
            monitor_size.scale_factor
        );
        1.0
    };
    let logical_monitor_width = monitor_size.width as f32 / scale_factor;

    let (window_width, window_height) = if logical_monitor_width < 1366.0 {
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
    // Wayland will not let a client move itself, so the compositor runs the drag there.
    #[cfg(target_os = "linux")]
    {
        let window_weak = window.as_weak();
        window.on_window_drag_started(move || {
            if let Some(w) = window_weak.upgrade() {
                classes::windowdrag::start(&w);
            }
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
        use slint::{LogicalPosition, WindowPosition};

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
    }

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

    #[cfg(target_os = "windows")]
    set_window_icon(&window);

    shared::api::ccu::spawn();
    slint::run_event_loop_until_quit()?;
    shared::api::ccu::stop();
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_app_user_model_id() {
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    const APP_USER_MODEL_ID: &str = "Aurora.Launcher";

    let id: Vec<u16> = APP_USER_MODEL_ID
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let hr = unsafe { SetCurrentProcessExplicitAppUserModelID(id.as_ptr()) };
    if hr < 0 {
        warn!("Could not set the app user model id (hresult {hr:#x})");
    }
}

#[cfg(target_os = "windows")]
fn set_window_icon(window: &MainWindow) {
    use i_slint_backend_winit::winit::window::Icon;
    use i_slint_backend_winit::WinitWindowAccessor;

    const LOGO: &[u8] = include_bytes!("../../../production/icons/logo.png");

    let icon = match image::load_from_memory(LOGO) {
        Ok(image) => {
            let image = image.into_rgba8();
            let (width, height) = image.dimensions();
            Icon::from_rgba(image.into_raw(), width, height)
        }
        Err(e) => {
            warn!("Could not decode the window icon: {e}");
            return;
        }
    };

    match icon {
        Ok(icon) => {
            if window
                .window()
                .with_winit_window(|w| w.set_window_icon(Some(icon.clone())))
                .is_none()
            {
                warn!("Could not access the winit window to set the icon");
            }
        }
        Err(e) => warn!("Could not build the window icon: {e}"),
    }
}

fn acquire_instance_lock() -> std::io::Result<Option<ipc::lock::SingletonLock>> {
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

    let path = ipc::state_root().join(ipc::AURORA_LOCK_FILE);

    let relaunched = std::env::args().any(|arg| {
        matches!(
            arg.as_str(),
            ipc::RELAUNCH_ARG | ipc::POST_UPDATE_ARG | ipc::SKIP_UPDATE_CHECK_ARG
        )
    });

    let attempts = if relaunched {
        u32::try_from(ipc::RELAUNCH_LOCK_TIMEOUT.as_millis() / RETRY_DELAY.as_millis())
            .unwrap_or(u32::MAX)
            .max(1)
    } else {
        1
    };

    for attempt in 0..attempts {
        match ipc::lock::SingletonLock::acquire(&path)? {
            Some(lock) => {
                if attempt > 0 {
                    info!("Acquired the instance lock after {attempt} retry(s)");
                }
                return Ok(Some(lock));
            }
            None if attempt + 1 < attempts => {
                if attempt == 0 {
                    info!("Another instance still holds the lock; waiting for it to exit");
                }
                std::thread::sleep(RETRY_DELAY);
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
