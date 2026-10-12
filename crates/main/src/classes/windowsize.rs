use crate::MainWindow;
use log::*;
use serde_json::json;
use shared::config::{self, key};
use shared::display::{center_window, get_monitor_size};
use slint::{ComponentHandle as _, LogicalSize};
use std::cell::RefCell;
use std::time::Duration;

pub const MIN_SIZE: (f32, f32) = (960.0, 540.0);
const DEFAULT_SIZE: (f32, f32) = (1280.0, 720.0);
const SMALL_MONITOR_WIDTH: f32 = 1366.0;
const MAX_MONITOR_SHARE: f32 = 0.95;
const TRACK_INTERVAL: Duration = Duration::from_secs(1);

thread_local! {
    static TRACK_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

fn monitor_logical_size() -> Option<(f32, f32)> {
    let monitor = match get_monitor_size() {
        Ok(monitor) => monitor,
        Err(e) => {
            error!("Could not get monitor size: {e}");
            return None;
        }
    };

    let scale = if monitor.scale_factor.is_finite() && monitor.scale_factor > 0.0 {
        monitor.scale_factor
    } else {
        warn!(
            "Monitor reported an unusable scale factor ({}); assuming 1.0",
            monitor.scale_factor
        );
        1.0
    };

    #[allow(clippy::cast_precision_loss)]
    Some((monitor.width as f32 / scale, monitor.height as f32 / scale))
}

fn default_size(monitor: Option<(f32, f32)>) -> (f32, f32) {
    match monitor {
        Some((width, _)) if width < SMALL_MONITOR_WIDTH => MIN_SIZE,
        _ => DEFAULT_SIZE,
    }
}

fn fit_to_monitor((width, height): (f32, f32), monitor: Option<(f32, f32)>) -> (f32, f32) {
    let Some((monitor_w, monitor_h)) = monitor else {
        return (width.max(MIN_SIZE.0), height.max(MIN_SIZE.1));
    };
    let max_w = (monitor_w * MAX_MONITOR_SHARE).max(MIN_SIZE.0);
    let max_h = (monitor_h * MAX_MONITOR_SHARE).max(MIN_SIZE.1);
    (
        width.clamp(MIN_SIZE.0, max_w).round(),
        height.clamp(MIN_SIZE.1, max_h).round(),
    )
}

#[derive(Clone, Copy, PartialEq)]
struct SavedWindow {
    size: (f32, f32),
    maximized: bool,
}

fn saved_window() -> Option<SavedWindow> {
    let value = config::get(key::WINDOW_SIZE);
    #[allow(clippy::cast_possible_truncation)]
    let width = value.get("width")?.as_f64()? as f32;
    #[allow(clippy::cast_possible_truncation)]
    let height = value.get("height")?.as_f64()? as f32;
    let maximized = value
        .get("maximized")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    (width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0).then_some(
        SavedWindow {
            size: (width, height),
            maximized,
        },
    )
}

fn save_window(saved: SavedWindow) {
    let (width, height) = saved.size;
    config::set(
        key::WINDOW_SIZE,
        json!({ "width": width, "height": height, "maximized": saved.maximized }),
    );
}

fn remember_enabled() -> bool {
    config::get(key::REMEMBER_WINDOW_SIZE)
        .as_bool()
        .unwrap_or(true)
}

fn apply_size(window: &MainWindow, (width, height): (f32, f32)) {
    window.set_initial_width(width);
    window.set_initial_height(height);
    window.window().set_size(LogicalSize::new(width, height));
}

pub fn apply_startup_size(window: &MainWindow) {
    let monitor = monitor_logical_size();

    let size = match saved_window().map(|w| w.size) {
        Some(saved) if remember_enabled() => {
            let fitted = fit_to_monitor(saved, monitor);
            if fitted != saved {
                info!(
                    "Saved window size {}x{} does not fit the monitor, using {}x{}",
                    saved.0, saved.1, fitted.0, fitted.1
                );
            }
            fitted
        }
        _ => default_size(monitor),
    };

    info!("Setting window size to {}x{}", size.0, size.1);
    apply_size(window, size);
}

pub fn apply_startup_maximized(window: &MainWindow) {
    if remember_enabled() && saved_window().is_some_and(|w| w.maximized) {
        info!("Window was closed maximized, maximizing it");
        window.window().set_maximized(true);
    }
}

pub fn reset(window: &MainWindow) {
    let size = default_size(monitor_logical_size());
    info!("Resetting window size to {}x{}", size.0, size.1);
    let slint_window = window.window();
    if slint_window.is_maximized() {
        slint_window.set_maximized(false);
    }
    apply_size(window, size);
    save_window(SavedWindow {
        size,
        maximized: false,
    });
    if let Err(e) = center_window(slint_window) {
        error!("Could not center window: {e}");
    }
}

pub fn start_tracking(window: &slint::Weak<MainWindow>) {
    let window = window.clone();
    let mut last_seen: Option<(f32, f32)> = None;
    let mut last_saved = saved_window();
    let default = default_size(monitor_logical_size());
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, TRACK_INTERVAL, move || {
        let Some(w) = window.upgrade() else { return };
        let slint_window = w.window();
        if !slint_window.is_visible() || slint_window.is_minimized() {
            last_seen = None;
            return;
        }

        if slint_window.is_maximized() {
            last_seen = None;
            if !last_saved.is_some_and(|s| s.maximized) {
                let saved = SavedWindow {
                    size: last_saved.map_or(default, |s| s.size),
                    maximized: true,
                };
                debug!("Saving window as maximized");
                save_window(saved);
                last_saved = Some(saved);
            }
            return;
        }

        let logical = slint_window.size().to_logical(slint_window.scale_factor());
        let current = (logical.width.round(), logical.height.round());
        if current.0 < MIN_SIZE.0 || current.1 < MIN_SIZE.1 {
            return;
        }

        let saved = SavedWindow {
            size: current,
            maximized: false,
        };
        if last_seen == Some(current) && last_saved != Some(saved) {
            debug!("Saving window size {}x{}", current.0, current.1);
            save_window(saved);
            last_saved = Some(saved);
        }
        last_seen = Some(current);
    });

    TRACK_TIMER.with(|cell| *cell.borrow_mut() = Some(timer));
}
