#![windows_subsystem = "windows"]

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let window = MainWindow::new()?;
    let slint_window = window.window();
    let monitor_size = slint_window.size();
    let mut target_w = 1280.0;
    let mut target_h = 720.0;

    if monitor_size.width < 1366 {
        target_w = 960.0;
        target_h = 540.0
    }
    slint_window.set_size(slint::PhysicalSize::new(target_w as u32, target_h as u32));

    // DRAGGING
    let window_weak = window.as_weak();
    window.on_window_dragged(move |delta_x, delta_y| {
        if let Some(w) = window_weak.upgrade() {
            let logical_pos = w.window().position();
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
            w.window().set_minimized(true)
        }
    });

    let window_weak = window.as_weak();
    window.on_close_clicked(move || {
        if let Some(w) = window_weak.upgrade() {
            let _ = w.hide();
        }
    });

    window.run()
}
