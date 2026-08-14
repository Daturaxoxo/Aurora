use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use log::*;
use once_cell::sync::Lazy;
use shared::utils::{self, get_cache_dir};

use crate::MainWindow;

const TIMEOUT: Duration = Duration::from_secs(20);
static IN_FLIGHT: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));
static FAILED: Lazy<Mutex<HashSet<String>>> = Lazy::new(|| Mutex::new(HashSet::new()));

thread_local! {
    static DECODED: RefCell<HashMap<String, slint::Image>> = RefCell::new(HashMap::new());
}

#[must_use]
pub fn cached(url: &str) -> Option<slint::Image> {
    DECODED.with(|decoded| decoded.borrow().get(url).cloned())
}

pub fn load(
    window: slint::Weak<MainWindow>,
    requests: Vec<(String, String)>,
    apply: fn(&MainWindow, &str, &slint::Image),
) {
    let requests: Vec<(String, String)> = {
        let mut in_flight = IN_FLIGHT.lock().unwrap_or_else(PoisonError::into_inner);
        let failed = FAILED.lock().unwrap_or_else(PoisonError::into_inner);
        requests
            .into_iter()
            .filter(|(_, url)| {
                cached(url).is_none() && !failed.contains(url) && in_flight.insert(url.clone())
            })
            .collect()
    };

    if requests.is_empty() {
        return;
    }

    std::thread::spawn(move || {
        for (id, url) in requests {
            let decoded = fetch(&url).and_then(|bytes| decode(&url, &bytes));

            IN_FLIGHT
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&url);

            let Some((pixels, width, height)) = decoded else {
                FAILED
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(url);
                continue;
            };

            let window = window.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(w) = window.upgrade() else { return };

                let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                    &pixels, width, height,
                );
                let image = slint::Image::from_rgba8(buffer);

                DECODED.with(|decoded| decoded.borrow_mut().insert(url, image.clone()));
                apply(&w, &id, &image);
            });
        }
    });
}

fn cache_path(url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);

    get_cache_dir()
        .join("ModIcons")
        .join(format!("{:016x}", hasher.finish()))
}

fn fetch(url: &str) -> Option<Vec<u8>> {
    let path = cache_path(url);
    match std::fs::read(&path) {
        Ok(bytes) if !bytes.is_empty() => return Some(bytes),
        _ => trace!("[ModIcons] {url} is not cached yet"),
    }

    debug!("[ModIcons] downloading {url}");

    let response = reqwest::blocking::Client::builder()
        .user_agent(format!("AuroraLauncher/{}", utils::get_local_version()))
        .timeout(TIMEOUT)
        .build()
        .map_err(|e| warn!("[ModIcons] could not build the http client: {e}"))
        .ok()?
        .get(url)
        .send()
        .map_err(|e| warn!("[ModIcons] could not download '{url}': {e}"))
        .ok()?;

    if !response.status().is_success() {
        warn!("[ModIcons] '{url}' returned {}", response.status());
        return None;
    }

    let bytes = response
        .bytes()
        .map_err(|e| warn!("[ModIcons] could not read '{url}': {e}"))
        .ok()?
        .to_vec();

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, &bytes) {
        warn!("[ModIcons] could not cache '{url}': {e}");
    }

    Some(bytes)
}

fn decode(url: &str, bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let decoded = image::load_from_memory(bytes)
        .map_err(|e| warn!("[ModIcons] could not decode '{url}': {e}"))
        .ok()?
        .into_rgba8();

    let (width, height) = decoded.dimensions();
    Some((decoded.into_raw(), width, height))
}
