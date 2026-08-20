use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;

use backend::classes::legacymods;
use log::*;
use slint::{ComponentHandle as _, Model as _};

use shared::config::{self, key};

use crate::bridge::Bridge;
use crate::classes::pages::modmanager::ModManagerHandler;
use crate::{MainWindow, Tr, TrKey};

pub const POPUP_ID: &str = "legacy-mods";

const POLL_INTERVAL: Duration = Duration::from_millis(500);

thread_local! {
    static PROMPT_TIMER: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
    static PENDING: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

pub fn mark_prompted() {
    config::set(key::LEGACY_MODS_PROMPTED, true);
    stop_prompt();
}

pub fn prompt_on_first_run(window: &slint::Weak<MainWindow>) {
    if was_prompted() {
        debug!("[LegacyMods] migration prompt already answered, skipping");
        return;
    }

    let folders = legacymods::find_legacy_folders();
    if folders.is_empty() {
        debug!("[LegacyMods] no legacy mod folder found, skipping");
        return;
    }

    info!("[LegacyMods] first run, queueing the migration prompt");
    PENDING.with(|slot| *slot.borrow_mut() = folders);

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

pub fn confirm(window: &slint::Weak<MainWindow>) {
    mark_prompted();

    let folders = PENDING.with(RefCell::take);

    match legacymods::migrate(&folders) {
        Ok(report) if report.failures.is_empty() => {
            refresh_mod_manager(window, report.migrated);
            Bridge::show_toast(
                window,
                &format!("Migrated {} mod(s).", report.migrated),
                "success",
            );
        }
        Ok(report) => {
            refresh_mod_manager(window, report.migrated);
            Bridge::show_toast(
                window,
                &format!(
                    "Migrated {} mod(s), {} could not be moved.",
                    report.migrated,
                    report.failures.len()
                ),
                "error",
            );
        }
        Err(e) => Bridge::show_toast(window, &format!("Migration failed - {e}"), "error"),
    }
}

fn refresh_mod_manager(window: &slint::Weak<MainWindow>, migrated: usize) {
    if migrated == 0 {
        return;
    }

    info!("[LegacyMods] rescanning the mods folder after the migration");
    ModManagerHandler::reload(window);
}

fn show_prompt(w: &MainWindow) {
    info!("[LegacyMods] showing the migration prompt");

    let keys = w.global::<TrKey>();
    let title = translation(w, keys.get_popup_legacy_mods_title());
    let message = translation(w, keys.get_popup_legacy_mods_message());

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
    config::get(key::LEGACY_MODS_PROMPTED)
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
