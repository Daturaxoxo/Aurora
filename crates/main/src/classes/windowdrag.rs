use i_slint_backend_winit::WinitWindowAccessor;
use log::*;
use slint::ComponentHandle;

/// Hands the drag over to the compositor.
pub fn start<T: ComponentHandle + 'static>(component: &T) {
    let started = component
        .window()
        .with_winit_window(i_slint_backend_winit::winit::window::Window::drag_window);

    match started {
        Some(Ok(())) => release_grab(component),
        Some(Err(e)) => warn!("Could not start the compositor window drag: {e}"),
        None => warn!("Could not access the winit window to start a drag"),
    }
}

fn release_grab<T: ComponentHandle + 'static>(component: &T) {
    let weak = component.as_weak();

    let queued = slint::invoke_from_event_loop(move || {
        let Some(component) = weak.upgrade() else {
            return;
        };
        if let Err(e) = component
            .window()
            .try_dispatch_event(slint::platform::WindowEvent::PointerExited)
        {
            warn!("Could not reset the pointer state after a drag: {e}");
        }
    });

    if let Err(e) = queued {
        warn!("Could not queue the pointer state reset after a drag: {e}");
    }
}
