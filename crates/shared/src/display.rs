use anyhow::{anyhow, Result};
use display_info::DisplayInfo;
use log::*;
use slint::{PhysicalPosition, PhysicalSize, WindowPosition};

pub fn get_monitor_size() -> Result<DisplayInfo> {
    let mut last_err = None;

    for attempt in 1..=10 {
        match DisplayInfo::all() {
            Ok(displays) => {
                if let Some(display) = displays.iter().find(|d| d.is_primary).cloned() {
                    if attempt > 1 {
                        info!("get_monitor_size: primary monitor found on attempt {attempt}");
                    }
                    return Ok(display);
                }

                if attempt == 10 {
                    if let Some(display) = displays.first().cloned() {
                        warn!("get_monitor_size: no primary monitor found, falling back to the first display");
                        return Ok(display);
                    }
                    last_err = Some(anyhow!("No displays were reported by the system"));
                } else {
                    info!("get_monitor_size: primary monitor not found after {attempt} attempts.");
                }
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

pub fn center_window(window: &slint::Window) -> Result<()> {
    let monitor_size = match get_monitor_size() {
        Ok(size) => size,
        Err(e) => return Err(anyhow!("Could not get monitor size: {e}")),
    };
    let window_size = window.size();

    let x = (monitor_size.width.cast_signed() - window_size.width.cast_signed()) / 2;
    let y = (monitor_size.height.cast_signed() - window_size.height.cast_signed()) / 2;

    window.set_position(WindowPosition::Physical(PhysicalPosition::new(
        x.max(0),
        y.max(0),
    )));

    Ok(())
}

#[allow(clippy::cast_precision_loss)]
pub fn on_drag(
    scale: f32,
    phys: PhysicalPosition,
    win_size: PhysicalSize,
    delta_x: f32,
    delta_y: f32,
) -> (f32, f32) {
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
        return (phys.x as f32 / scale, phys.y as f32 / scale);
    }

    (new_x, new_y)
}
