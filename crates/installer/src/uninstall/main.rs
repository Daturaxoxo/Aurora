#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

include!(concat!(env!("OUT_DIR"), "/uninstall.rs"));

mod elevate;
mod plan;
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

fn start(ui: &UninstallerWindow, options: Options) {
    let plan = current_plan();

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
    run::run(ui.as_weak(), plan, options);
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = UninstallerWindow::new()?;

    let window = ui.window();

    window.set_size(slint::LogicalSize::new(850.0, 580.0));
    match center_window(window) {
        Ok(()) => {}
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

    let plan = current_plan();
    ui.set_app_path(
        plan.app_dir
            .as_ref()
            .map_or_else(|| "Not found".to_string(), |p| p.display().to_string())
            .into(),
    );
    ui.set_mods_path(
        plan.mods_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
            .into(),
    );
    ui.set_mods_available(plan.mods_dir.is_some());
    ui.set_dev_build(plan.dev_build);

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
