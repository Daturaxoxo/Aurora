use crate::MainWindow;

#[cfg(target_os = "windows")]
pub use windows_impl::{activate, deactivate};

#[cfg(not(target_os = "windows"))]
pub fn activate(_window: &slint::Weak<MainWindow>, _hide_window: bool) {
    log::debug!("[Tray] interface minimization is not supported on this platform");
}

#[cfg(not(target_os = "windows"))]
pub const fn deactivate(_window: &slint::Weak<MainWindow>) {}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::MainWindow;
    use anyhow::Result;
    use log::{debug, error, info};
    use slint::{ComponentHandle, Timer, TimerMode};
    use std::cell::RefCell;
    use std::time::Duration;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(100);

    struct TrayState {
        // Held so the OS icon stays alive. Removed from the tray on drop.
        _tray: TrayIcon,
        timer: Timer,
    }

    thread_local! {
        static TRAY: RefCell<Option<TrayState>> = const { RefCell::new(None) };
    }

    pub fn activate(window: &slint::Weak<MainWindow>, hide_window: bool) {
        let window = window.clone();
        let result = slint::invoke_from_event_loop(move || activate_on_ui(&window, hide_window));
        if let Err(e) = result {
            error!("[Tray] could not reach the UI event loop to activate: {e}");
        }
    }

    pub fn deactivate(window: &slint::Weak<MainWindow>) {
        let window = window.clone();
        let result = slint::invoke_from_event_loop(move || teardown_on_ui(&window, true));
        if let Err(e) = result {
            error!("[Tray] could not reach the UI event loop to deactivate: {e}");
        }
    }

    fn activate_on_ui(window: &slint::Weak<MainWindow>, hide_window: bool) {
        if TRAY.with_borrow(Option::is_some) {
            debug!("[Tray] activate requested but the tray icon already exists");
            return;
        }

        let icon = match load_icon() {
            Ok(icon) => icon,
            Err(e) => {
                error!("[Tray] could not load the tray icon image: {e}");
                return;
            }
        };

        let show_item = MenuItem::new(if hide_window { "Show" } else { "Hide" }, true, None);
        let exit_item = MenuItem::new("Exit Aurora", true, None);
        let menu = Menu::new();
        if let Err(e) = menu.append_items(&[&show_item, &exit_item]) {
            error!("[Tray] could not build the tray menu: {e}");
            return;
        }

        let tray = match TrayIconBuilder::new()
            .with_tooltip("Aurora")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
        {
            Ok(tray) => tray,
            Err(e) => {
                error!("[Tray] could not create the tray icon: {e}");
                return;
            }
        };

        if hide_window {
            match window.upgrade() {
                Some(w) => {
                    if let Err(e) = w.hide() {
                        error!("[Tray] could not hide the main window: {e}");
                    }
                }
                None => error!("[Tray] window handle dead, cannot hide the main window"),
            }
        }

        let timer = Timer::default();
        let toggle_id = show_item.id().clone();
        let exit_id = exit_item.id().clone();
        let ww = window.clone();
        let mut label_shows_hide = !hide_window;
        timer.start(TimerMode::Repeated, EVENT_POLL_INTERVAL, move || {
            let visible = ww.upgrade().is_some_and(|w| w.window().is_visible());
            if visible != label_shows_hide {
                show_item.set_text(if visible { "Hide" } else { "Show" });
                label_shows_hide = visible;
            }

            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    info!("[Tray] tray icon clicked, toggling the window");
                    toggle(&ww);
                }
            }
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id() == &toggle_id {
                    info!("[Tray] Show/Hide selected, toggling the window");
                    toggle(&ww);
                } else if event.id() == &exit_id {
                    info!("[Tray] \"Exit Aurora\" selected, quitting");
                    exit(&ww);
                }
            }
        });

        TRAY.with_borrow_mut(|slot| *slot = Some(TrayState { _tray: tray, timer }));
        if hide_window {
            info!("[Tray] interface minimized to the system tray");
        } else {
            info!("[Tray] tray icon created, window left visible");
        }
    }

    fn teardown_on_ui(window: &slint::Weak<MainWindow>, show_window: bool) {
        let Some(state) = TRAY.with_borrow_mut(Option::take) else {
            return;
        };
        state.timer.stop();
        drop(state);
        info!("[Tray] tray icon removed");

        if !show_window {
            return;
        }
        match window.upgrade() {
            Some(w) => {
                if let Err(e) = w.show() {
                    error!("[Tray] could not show the main window: {e}");
                }
            }
            None => error!("[Tray] window handle dead, cannot restore the main window"),
        }
    }

    fn toggle(window: &slint::Weak<MainWindow>) {
        let Some(w) = window.upgrade() else {
            error!("[Tray] window handle dead, cannot toggle the main window");
            return;
        };
        let result = if w.window().is_visible() {
            w.hide()
        } else {
            w.show()
        };
        if let Err(e) = result {
            error!("[Tray] could not toggle the main window: {e}");
        }
    }

    fn exit(window: &slint::Weak<MainWindow>) {
        let window = window.clone();
        let result = slint::invoke_from_event_loop(move || {
            teardown_on_ui(&window, false);
            crate::classes::logwindow::hide();
            if let Some(w) = window.upgrade() {
                let _ = w.hide();
            }
            if let Err(e) = slint::quit_event_loop() {
                error!("[Tray] could not quit the event loop: {e}");
            }
        });
        if let Err(e) = result {
            error!("[Tray] could not queue the exit request: {e}");
        }
    }

    fn load_icon() -> Result<Icon> {
        let image =
            image::load_from_memory(include_bytes!("../../../../production/icons/logo.png"))?
                .into_rgba8();
        let (width, height) = image.dimensions();
        Ok(Icon::from_rgba(image.into_raw(), width, height)?)
    }
}
