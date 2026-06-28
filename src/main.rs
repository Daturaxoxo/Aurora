#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use std::path::PathBuf;
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    app_location: String,
    game_path: String,
}

fn load_config() -> Result<Config, String> {
    let config_path = dirs::config_dir()
        .ok_or("Could not resolve config directory")?
        .join("Aurora")
        .join("UserData")
        .join("config.json");

    let contents = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read config.json: {e}"))?;

    serde_json::from_str(&contents)
        .map_err(|e| format!("Failed to parse config.json: {e}"))
}

fn find_nte_launcher(game_path: &str) -> Option<PathBuf> {
    std::fs::read_dir(game_path).ok()?.find_map(|entry| {
        let path = entry.ok()?.path();
        let name = path.file_name()?.to_string_lossy().to_string();
        if name.contains("NTE") && name.contains("Launcher") && name.ends_with(".exe") {
            Some(path)
        } else {None}
    })
}

fn launch_and_exit(child: std::process::Child) {
    std::thread::spawn(move || {
        let mut child = child;
        std::thread::sleep(std::time::Duration::from_millis(500));
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                eprintln!("Process exited early: {status}");
            }
            _ => std::process::exit(0),
        }
    });
}

#[cfg(windows)]
fn launch_elevated(path: &std::path::Path) {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file = HSTRING::from(path.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("runas"),
            &file,
            &HSTRING::from(""),
            &HSTRING::from(""),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        eprintln!("ShellExecuteW failed with code: {}", result.0 as usize);
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(500));
        std::process::exit(0);
    });
}

#[cfg(not(windows))]
fn launch_elevated(path: &std::path::Path) {
    // On Linux under Wine/Proton, just launch normally
    match std::process::Command::new(path).spawn() {
        Ok(child) => launch_and_exit(child),
        Err(e) => eprintln!("Failed to launch: {e}"),
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;

    ui.show()?;
    center_window(&ui);

    let ui_weak = ui.as_weak();
    ui.on_window_dragged(move |delta_x, delta_y| {
        if let Some(w) = ui_weak.upgrade() {
            let logical_pos = w.window().position();
            #[allow(clippy::cast_precision_loss)]
            w.window().set_position(slint::WindowPosition::Logical(
                slint::LogicalPosition::new(
                    logical_pos.x as f32 + delta_x,
                    logical_pos.y as f32 + delta_y,
                ),
            ));
        }
    });

    let ui_weak_min = ui.as_weak();
    ui.on_minimize_window(move || {
        if let Some(w) = ui_weak_min.upgrade() {
            w.window().set_minimized(true);
        }
    });

    ui.on_close_window(|| std::process::exit(0));

    // External Links

    ui.on_open_discord(|| {
        if let Err(e) = open::that("https://discord.gg/565jfeYsbp") {
            eprintln!("Failed to open Discord link: {e}");
        }
    });

    ui.on_open_website(|| {
        if let Err(e) = open::that("https://aurorante.moe") {
            eprintln!("Failed to open website link: {e}");
        }
    });

    // Launch Buttons

    ui.on_launch_with_mods(|| {
        let config = match load_config() {
            Ok(c) => c,
            Err(e) => { eprintln!("Config error: {e}"); return; }
        };
        let aurora_exe = std::path::PathBuf::from(&config.app_location);
        if !aurora_exe.exists() {
            eprintln!("Aurora.exe not found at: {}", aurora_exe.display());
            return;
        }
        launch_elevated(&aurora_exe);
    });

    ui.on_launch_vanilla(|| {
        let config = match load_config() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Config error: {e}");
                return;
            }
        };

        let launcher = match find_nte_launcher(&config.game_path) {
            Some(p) => p,
            None => {
                eprintln!("NTE launcher not found in: {}", config.game_path);
                return;
            }
        };

        match std::process::Command::new(&launcher).spawn() {
            Ok(child) => launch_and_exit(child),
            Err(e) => eprintln!("Failed to launch NTE launcher: {e}"),
        }
    });

    ui.run()
}

fn center_window(ui: &AppWindow) {
    let window = ui.window();
    let win_size = window.size();

    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) } as i32;
        let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) } as i32;
        let x = (screen_w - win_size.width as i32) / 2;
        let y = (screen_h - win_size.height as i32) / 2;
        window.set_position(slint::WindowPosition::Physical(
            slint::PhysicalPosition::new(x, y),
        ));
    }

    #[cfg(not(windows))]
    let _ = win_size;
}