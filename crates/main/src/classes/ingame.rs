use crate::bridge::engine;
use ipc::ingame::{self, Accent};
use log::*;
use std::collections::HashSet;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;
const HOLD: Duration = Duration::from_secs(8);
static ANNOUNCED: Mutex<Option<HashSet<String>>> = Mutex::new(None);
pub fn notify_once(key: &str, title: String, subtitle: String, accent: Accent) {
    if !engine::session_active() {
        return;
    }
    {
        let mut announced = ANNOUNCED.lock().unwrap_or_else(PoisonError::into_inner);
        if !announced
            .get_or_insert_with(HashSet::new)
            .insert(key.to_owned())
        {
            return;
        }
    }
    std::thread::spawn(move || match ingame::send_toast(&title, &subtitle, HOLD, accent) {
        Ok(()) => info!("in-game toast shown: {title}"),
        Err(e) => debug!("in-game toast not delivered: {e}"),
    });
}
pub fn session_closed() {
    *ANNOUNCED.lock().unwrap_or_else(PoisonError::into_inner) = None;
}
