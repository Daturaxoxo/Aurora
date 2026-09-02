#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

slint::include_modules!();

use std::path::{Path, PathBuf};

use shared::classes::info::version::{LAUNCHER_MAP, StartMethod, detect_distribution};
use slint::{LogicalPosition, WindowPosition};

use shared::config::{self, key};
use shared::display::{center_window, on_drag};
const LAUNCHER_SEARCH_DEPTH: usize = 2;

#[cfg(windows)]
const STARTING_AURORA: &str =
    "Starting Aurora - accept the Windows administrator prompt to continue.";
#[cfg(not(windows))]
const STARTING_AURORA: &str = "Starting Aurora...";

enum Status {
    Busy(String),
    Error(String),
}

fn report(ui: &slint::Weak<AppWindow>, status: Status) {
    if let Status::Error(message) = &status {
        eprintln!("{message}");
    }

    let delivered = ui.upgrade_in_event_loop(move |ui| match status {
        Status::Busy(message) => {
            ui.set_status_error(false);
            ui.set_status_text(message.into());
        }
        Status::Error(message) => {
            ui.set_busy(false);
            ui.set_status_error(true);
            ui.set_status_text(message.into());
        }
    });

    if let Err(e) = delivered {
        eprintln!("Could not deliver a status update to the window: {e}");
    }
}

fn is_launcher(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".exe") && lower.contains("nte") && lower.contains("launcher")
}

fn search_for_launcher(dir: &Path, depth: usize) -> Option<PathBuf> {
    let mut subdirs = Vec::new();

    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();

        if file_type.is_dir() {
            subdirs.push(path);
        } else if path
            .file_name()
            .is_some_and(|name| is_launcher(&name.to_string_lossy()))
        {
            return Some(path);
        }
    }

    if depth == 0 {
        return None;
    }

    subdirs
        .into_iter()
        .find_map(|sub| search_for_launcher(&sub, depth - 1))
}

fn find_nte_launcher(game_path: &Path) -> Option<PathBuf> {
    // The known names cover every real install and cost three stats.
    for (exe, _) in LAUNCHER_MAP {
        let candidate = game_path.join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    search_for_launcher(game_path, LAUNCHER_SEARCH_DEPTH)
}

fn launch_and_exit(child: std::process::Child, ui: slint::Weak<AppWindow>, what: &'static str) {
    std::thread::spawn(move || {
        let mut child = child;
        std::thread::sleep(std::time::Duration::from_millis(500));
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                report(
                    &ui,
                    Status::Error(format!("{what} started but exited immediately ({status}).")),
                );
            }
            _ => std::process::exit(0),
        }
    });
}

#[cfg(windows)]
fn launch_elevated(path: PathBuf, ui: slint::Weak<AppWindow>) {
    use std::process::Command;
    std::thread::spawn(move || {
        let path_str = path.to_string_lossy().replace('\'', "''");
        let ps_command = format!("Start-Process -FilePath '{path_str}' -Verb RunAs");

        let status = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_command])
            .status();

        match status {
            Ok(s) if s.success() => {
                std::thread::sleep(std::time::Duration::from_millis(500));
                std::process::exit(0);
            }
            Ok(s) => report(
                &ui,
                Status::Error(format!(
                    "Aurora was not started. The administrator prompt was declined or dismissed ({s})."
                )),
            ),
            Err(e) => report(
                &ui,
                Status::Error(format!(
                    "Could not run PowerShell to request administrator rights: {e}"
                )),
            ),
        }
    });
}

#[cfg(not(windows))]
fn launch_elevated(path: PathBuf, ui: slint::Weak<AppWindow>) {
    // On Linux under Wine/Proton there is no UAC, so this just spawns.
    match std::process::Command::new(&path).spawn() {
        Ok(child) => launch_and_exit(child, ui, "Aurora"),
        Err(e) => report(&ui, Status::Error(format!("Could not start Aurora: {e}"))),
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

    let ui_weak_mods = ui.as_weak();
    ui.on_launch_with_mods(move || {
        let Some(window) = ui_weak_mods.upgrade() else {
            return;
        };
        if window.get_busy() {
            return;
        }

        let raw_app_location = config::get(key::APP_LOCATION);
        let app_location = raw_app_location.as_str().unwrap_or_default().trim();
        if app_location.is_empty() {
            report(
                &ui_weak_mods,
                Status::Error(
                    "Aurora is not installed on this machine. Run the Aurora installer and start \
                     Aurora once, then this launcher can find it."
                        .to_string(),
                ),
            );
            return;
        }

        let aurora_exe = PathBuf::from(app_location);
        if !aurora_exe.is_file() {
            report(
                &ui_weak_mods,
                Status::Error(format!(
                    "Aurora is no longer at {}. Reinstall it, or start it once from its new \
                     location so this launcher picks up the change.",
                    aurora_exe.display()
                )),
            );
            return;
        }

        window.set_busy(true);
        report(&ui_weak_mods, Status::Busy(STARTING_AURORA.to_string()));
        launch_elevated(aurora_exe, ui_weak_mods.clone());
    });

    let ui_weak_vanilla = ui.as_weak();
    ui.on_launch_vanilla(move || {
        let Some(window) = ui_weak_vanilla.upgrade() else {
            return;
        };
        if window.get_busy() {
            return;
        }

        let raw_game_path = config::get(key::GAME_PATH);
        let game_path = raw_game_path
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        if game_path.is_empty() {
            report(
                &ui_weak_vanilla,
                Status::Error(
                    "No game folder is set. Open Aurora once and point it at your Neverness To \
                     Everness install."
                        .to_string(),
                ),
            );
            return;
        }

        window.set_busy(true);
        report(
            &ui_weak_vanilla,
            Status::Busy("Looking for the game launcher...".to_string()),
        );

        let ui = ui_weak_vanilla.clone();
        std::thread::spawn(move || {
            let game_path = Path::new(&game_path);

            let Some(launcher) = find_nte_launcher(game_path) else {
                report(
                    &ui,
                    Status::Error(format!(
                        "No NTE launcher was found in {}. Check the game folder set in Aurora.",
                        game_path.display()
                    )),
                );
                return;
            };

            let distribution = detect_distribution(game_path);

            let mut args = distribution.launch_args().to_vec();
            args.extend_from_slice(StartMethod::from_config().launch_args());

            match std::process::Command::new(&launcher).args(&args).spawn() {
                Ok(child) => {
                    report(&ui, Status::Busy("Starting the game...".to_string()));
                    launch_and_exit(child, ui.clone(), "The game launcher");
                }
                Err(e) => report(
                    &ui,
                    Status::Error(format!("Could not start {}: {e}", launcher.display())),
                ),
            }
        });
    });

    ui.run()
}