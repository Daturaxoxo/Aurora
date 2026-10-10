use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::Result;
use log::*;
use serde::{Deserialize, Serialize};

use crate::classes::games::InstallationFacts;
use crate::classes::info::version::Version;
use crate::utils::get_local_version;
use crate::{config, telemetry, utils};

const PATH: &str = "/app/ccu";
const TELEMETRY_KEY: &str = "telemetry";
const HEARTBEAT: Duration = Duration::from_mins(10);
const JITTER: f64 = 0.05;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const REGION_UNKNOWN: &str = "unknown";

static SESSION: OnceLock<String> = OnceLock::new();
static STOP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
static HEARTBEAT_THREAD: Mutex<Option<Heartbeat>> = Mutex::new(None);
static STOPPED: AtomicBool = AtomicBool::new(false);

struct Heartbeat {
    stop: mpsc::Sender<()>,
    done: mpsc::Receiver<()>,
    thread: JoinHandle<()>,
}

#[derive(Serialize)]
struct Beat<'a> {
    session: &'a str,
    event: &'a str,
    version: &'a str,
    os: &'a str,
    region: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stats {
    pub count: u32,
    #[serde(default)]
    pub by_os: HashMap<String, u32>,
    #[serde(default)]
    pub by_version: HashMap<String, u32>,
    #[serde(default)]
    pub by_region: HashMap<String, u32>,
}

pub fn session_id() -> &'static str {
    SESSION.get_or_init(new_session_id)
}

pub fn enabled() -> bool {
    config::get(TELEMETRY_KEY).as_bool().unwrap_or(true)
}

const fn region_name(version: Version) -> &'static str {
    match version {
        Version::Global => "global",
        Version::CN => "china",
        Version::TW => "taiwan",
        Version::Unknown => REGION_UNKNOWN,
    }
}

pub fn region_of(installation: Option<&InstallationFacts>) -> &'static str {
    installation
        .and_then(|facts| Version::from_key(facts.variant()))
        .map_or(REGION_UNKNOWN, region_name)
}

pub fn region() -> &'static str {
    telemetry::context().map_or(REGION_UNKNOWN, |c| c.region)
}

fn entropy(salt: u64) -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let stack = &raw const salt as usize as u64;

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(salt);
    hasher.write_u64(stack);
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    hasher.write_u32(std::process::id());
    hasher.finish()
}

fn new_session_id() -> String {
    let hi = entropy(0);
    let lo = entropy(1);

    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        hi >> 32,
        (hi >> 16) & 0xffff,
        hi & 0x0fff,
        0x8000 | ((lo >> 48) & 0x3fff),
        lo & 0xffff_ffff_ffff
    )
}

fn interval() -> Duration {
    let byte = u8::try_from(entropy(2) & 0xff).unwrap_or(128);
    let factor = (f64::from(byte) / 255.0 - 0.5).mul_add(2.0 * JITTER, 1.0);

    HEARTBEAT.mul_f64(factor)
}

fn send(event: &str, version: &str) -> Result<()> {
    super::post_json(
        PATH,
        &Beat {
            session: session_id(),
            event,
            version,
            os: std::env::consts::OS,
            region: region(),
        },
    )
}

pub fn spawn() {
    if !enabled() {
        return;
    }

    let mut heartbeat = HEARTBEAT_THREAD
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if STOPPED.load(Ordering::SeqCst) || heartbeat.is_some() {
        return;
    }

    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let spawned = thread::Builder::new()
        .name("ccu-heartbeat".into())
        .spawn(move || {
            let version = get_local_version().trim().to_string();
            let period = interval();

            let mut event = "start";
            loop {
                if let Err(e) = send(event, &version) {
                    debug!("ccu {event} failed: {e:#}");
                }

                event = "beat";
                match stop_rx.recv_timeout(period) {
                    Err(RecvTimeoutError::Timeout) => {}
                    Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            done_tx.send(()).ok();
        });

    match spawned {
        Ok(thread) => {
            *heartbeat = Some(Heartbeat {
                stop: stop_tx,
                done: done_rx,
                thread,
            });
        }
        Err(e) => warn!("Could not spawn the ccu heartbeat thread: {e}"),
    }
    drop(heartbeat);
}

pub fn stop() {
    STOPPED.store(true, Ordering::SeqCst);
    let Some(heartbeat) = HEARTBEAT_THREAD
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    else {
        return;
    };

    heartbeat.stop.send(()).ok();
    // a beat that is still being sent can hold the thread for the whole request timeout, dont wait on that
    match heartbeat.done.recv_timeout(SHUTDOWN_TIMEOUT) {
        Ok(()) => {
            if heartbeat.thread.join().is_err() {
                warn!("The ccu heartbeat thread panicked");
            }
        }
        Err(_) => debug!("The ccu heartbeat did not stop within {SHUTDOWN_TIMEOUT:?}, leaving it"),
    }

    let Some(session) = SESSION.get() else {
        return;
    };

    let client = STOP_CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(format!("AuroraLauncher/{}", utils::get_local_version()))
            .connect_timeout(SHUTDOWN_TIMEOUT)
            .timeout(SHUTDOWN_TIMEOUT)
            .build()
            .unwrap_or_else(|e| {
                warn!("could not build the ccu shutdown client, falling back to default: {e}");
                reqwest::blocking::Client::default()
            })
    });

    let version = get_local_version().trim().to_string();
    let payload = Beat {
        session,
        event: "stop",
        version: &version,
        os: std::env::consts::OS,
        region: region(),
    };

    if let Err(e) = client.post(super::url(PATH)).json(&payload).send() {
        debug!("ccu stop failed: {e}");
    }
}

pub fn stats() -> Result<Stats> {
    super::get_json(PATH)
}
