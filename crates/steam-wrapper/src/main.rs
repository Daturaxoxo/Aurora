#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use std::path::{Path, PathBuf};

use shared::classes::info::version::detect_distribution;
use slint::{LogicalPosition, WindowPosition};

use shared::config::{self, key};
use shared::display::{center_window, on_drag};

fn find_nte_launcher(game_path: &str) -> Option<PathBuf> {
    std::fs::read_dir(game_path).ok()?.find_map(|entry| {
        let path = entry.ok()?.path();
        let name = path.file_name()?.to_string_lossy().to_string();
        if name.contains("NTE") && name.contains("Launcher") && name.ends_with(".exe") {
            Some(path)
        } else {
            None
        }
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
    use std::process::Command;

    let path_str = path.to_string_lossy().replace('\'', "''");
    let ps_command = format!("Start-Process -FilePath '{}' -Verb RunAs", path_str);

    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
        .status();

    match status {
        Ok(s) if s.success() => {
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(500));
                std::process::exit(0);
            });
        }
        Ok(s) => {
            eprintln!("PowerShell command failed with status: {s}");
        }
        Err(e) => {
            eprintln!("Failed to execute PowerShell: {e}");
        }
    }
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
    let window = ui.window();
    match center_window(window) {
        Ok(_) => {}
        Err(e) => eprintln!("Failed to center window: {e}"),
    }

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
        if let Err(e) = open::that("https://getaurora.moe/") {
            eprintln!("Failed to open website link: {e}");
        }
    });

    // Launch Buttons

    ui.on_launch_with_mods(|| {
        let raw_app_location = config::get(key::APP_LOCATION);
        let app_location = raw_app_location.as_str().unwrap_or_default();
        let aurora_exe = PathBuf::from(app_location);
        if !aurora_exe.exists() {
            eprintln!("Aurora.exe not found at: {}", aurora_exe.display());
            return;
        }
        launch_elevated(&aurora_exe);
    });

    ui.on_launch_vanilla(|| {
        let raw_game_path = config::get(key::GAME_PATH);
        let game_path = raw_game_path.as_str().unwrap_or_default();
        if game_path.is_empty() {
            eprintln!("Game path not set");
            return;
        }

        let launcher = match find_nte_launcher(game_path) {
            Some(p) => p,
            None => {
                eprintln!("NTE launcher not found in: {}", game_path);
                return;
            }
        };

        let distribution = detect_distribution(Path::new(game_path));

        match std::process::Command::new(&launcher)
            .args(distribution.launch_args())
            .spawn()
        {
            Ok(child) => launch_and_exit(child),
            Err(e) => eprintln!("Failed to launch NTE launcher: {e}"),
        }
    });

    ui.run()
}
