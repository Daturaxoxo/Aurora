use std::cell::RefCell;
use std::time::Duration;

use log::*;
use slint::{ComponentHandle as _, Model as _};

use shared::config::{self, key};

use crate::{MainWindow, Tr, TrKey};

pub const POPUP_ID: &str = "desktop-entry";

const POLL_INTERVAL: Duration = Duration::from_millis(500);

thread_local! {
    static PROMPT_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

pub fn apply(enabled: bool) {
    config::set(key::DESKTOP_ENTRY, enabled);

    if enabled {
        info!("[Desktop] installing the desktop entry");
        shared::desktop_entry::install();
    } else {
        info!("[Desktop] removing the desktop entry");
        shared::desktop_entry::uninstall();
    }
}

pub fn mark_prompted() {
    config::set(key::DESKTOP_ENTRY_PROMPTED, true);
    stop_prompt();
}

pub fn prompt_on_first_run(window: &slint::Weak<MainWindow>) {
    if was_prompted() {
        debug!("[Desktop] desktop entry prompt already answered, skipping");
        return;
    }

    info!("[Desktop] first run, queueing the desktop entry prompt");

    let window = window.clone();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, POLL_INTERVAL, move || {
        let Some(w) = window.upgrade() else {
            stop_prompt();
            return;
        };

        if was_prompted() {
            stop_prompt();
            return;
        }

        if w.get_popup_active() {
            return;
        }

        show_prompt(&w);
    });

    PROMPT_TIMER.with(|slot| *slot.borrow_mut() = Some(timer));
}

fn show_prompt(w: &MainWindow) {
    info!("[Desktop] showing the desktop entry prompt");

    let keys = w.global::<TrKey>();
    let title = translation(w, keys.get_popup_desktop_entry_title());
    let message = translation(w, keys.get_popup_desktop_entry_message());

    w.set_popup_id(POPUP_ID.into());
    w.set_popup_title(title);
    w.set_popup_message(message);
    w.set_popup_confirm_delay(0);
    w.set_popup_required_count(0);
    w.set_popup_checkboxes(slint::ModelRc::default());
    w.set_popup_active(true);
}

fn translation(w: &MainWindow, index: i32) -> slint::SharedString {
    w.global::<Tr>()
        .get_values()
        .row_data(index.try_into().unwrap_or(0))
        .unwrap_or_default()
}

fn was_prompted() -> bool {
    config::get(key::DESKTOP_ENTRY_PROMPTED)
        .as_bool()
        .unwrap_or(false)
}

fn stop_prompt() {
    PROMPT_TIMER.with(|slot| {
        if let Some(timer) = slot.borrow().as_ref() {
            timer.stop();
        }
    });
}
