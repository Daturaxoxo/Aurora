#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

include!(concat!(env!("OUT_DIR"), "/uninstall.rs"));

mod elevate;
mod plan;
#[path = "../registry.rs"]
mod registry;
mod run;

use shared::display::{center_window, on_drag};

use slint::{ComponentHandle, LogicalPosition, ModelRc, SharedString, VecModel, WindowPosition};
use std::rc::Rc;

use plan::Plan;
use run::Options;

fn current_plan() -> Plan {
    if elevate::is_relaunch() {
        elevate::plan_from_args()
    } else {
        Plan::resolve()
    }
}

fn app_dir_error(plan: &Plan) -> String {
    plan.app_dir_error.clone().unwrap_or_else(|| {
        "Could not locate the Aurora application folder. Nothing was removed.".to_owned()
    })
}

fn show_app_dir(ui: &UninstallerWindow, plan: &Plan) {
    ui.set_app_path(
        plan.app_dir
            .as_ref()
            .map_or_else(|| "Not found".to_string(), |p| p.display().to_string())
            .into(),
    );
    ui.set_app_found(plan.app_dir.is_some());
    ui.set_app_error(app_dir_error(plan).into());
}

fn start(ui: &UninstallerWindow, options: Options) {
    let plan = current_plan();

    let Some(app_dir) = plan.app_dir.clone() else {
        ui.set_current_step(1);
        run::fail(&ui.as_weak(), app_dir_error(&plan));
        return;
    };

    if !elevate::is_relaunch() && elevate::needed(&plan, options) {
        match elevate::relaunch(&plan, options) {
            Ok(()) => {
                let _ = ui.hide();
                let _ = slint::quit_event_loop();
            }
            Err(e) => {
                ui.set_current_step(1);
                run::fail(&ui.as_weak(), e);
            }
        }
        return;
    }

    ui.set_current_step(1);
    run::run(ui.as_weak(), plan, app_dir, options);
}

fn main() -> Result<(), slint::PlatformError> {
    if let Some(dir) = run::cleanup_target() {
        run::run_cleanup(&dir);
        return Ok(());
    }

    let ui = UninstallerWindow::new()?;

    let window = ui.window();

    window.set_size(slint::LogicalSize::new(850.0, 580.0));
    match center_window(window) {
        Ok(()) => {}
        Err(e) => eprintln!("Failed to center window: {e}"),
    }

    let plan = current_plan();
    show_app_dir(&ui, &plan);
    ui.set_mods_path(
        plan.mods_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
            .into(),
    );
    ui.set_mods_available(plan.mods_dir.is_some());

    ui.set_uninstall_log(ModelRc::from(Rc::new(VecModel::<SharedString>::default())));

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
    ui.on_cancel_clicked(move || {
        if let Some(w) = ui_weak.upgrade() {
            let _ = w.hide();
        }
        let _ = slint::quit_event_loop();
    });

    let ui_weak = ui.as_weak();
    ui.on_browse_clicked(move || {
        let ui_weak = ui_weak.clone();
        std::thread::spawn(move || {
            let Some(picked) = rfd::FileDialog::new()
                .set_title("Select the Aurora Application Folder")
                .pick_folder()
            else {
                return;
            };

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = ui_weak.upgrade() {
                    plan::set_app_dir_override(picked);
                    show_app_dir(&w, &current_plan());
                }
            });
        });
    });

    let ui_weak = ui.as_weak();
    ui.on_next_clicked(move || {
        let Some(w) = ui_weak.upgrade() else {
            return;
        };
        match w.get_current_step() {
            0 => {
                let options = Options {
                    delete_config: w.get_option_delete_config(),
                    delete_mods: w.get_option_delete_mods(),
                    preserve_mods: w.get_option_preserve_mods(),
                };
                start(&w, options);
            }
            1 => {
                if w.get_uninstall_done() {
                    w.set_current_step(2);
                }
            }
            2 => {
                let _ = w.hide();
                let _ = slint::quit_event_loop();
            }
            _ => {}
        }
    });

    if elevate::is_relaunch() {
        let options = elevate::options_from_args();
        ui.set_option_delete_config(options.delete_config);
        ui.set_option_delete_mods(options.delete_mods);
        ui.set_option_preserve_mods(options.preserve_mods);
        let ui_weak = ui.as_weak();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            if let Some(w) = ui_weak.upgrade() {
                start(&w, options);
            }
        });
    }

    ui.show()?;
    slint::run_event_loop_until_quit()?;
    if let Some(dir) = run::take_pending_cleanup() {
        run::cleanup_after_exit(&dir);
    }

    Ok(())
}
