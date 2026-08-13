use std::collections::HashMap;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use log::*;
use serde::{Deserialize, Serialize};

use crate::config;
use crate::utils::get_local_version;

const PATH: &str = "/app/ccu";
const TELEMETRY_KEY: &str = "telemetry";
const HEARTBEAT: Duration = Duration::from_mins(2);
const JITTER: f64 = 0.10;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

static SESSION: OnceLock<String> = OnceLock::new();

#[derive(Serialize)]
struct Beat<'a> {
    session: &'a str,
    event: &'a str,
    version: &'a str,
    os: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Stats {
    pub count: u32,
    #[serde(default)]
    pub by_os: HashMap<String, u32>,
    #[serde(default)]
    pub by_version: HashMap<String, u32>,
}

pub fn session_id() -> &'static str {
    SESSION.get_or_init(new_session_id)
}

pub fn enabled() -> bool {
    config::get(TELEMETRY_KEY).as_bool().unwrap_or(true)
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
        },
    )
}

pub fn spawn() {
    if !enabled() {
        return;
    }

    let spawned = thread::Builder::new()
        .name("ccu-heartbeat".into())
        .spawn(|| {
            let version = get_local_version().trim().to_string();
            let period = interval();

            let mut event = "start";
            loop {
                if let Err(e) = send(event, &version) {
                    debug!("ccu {event} failed: {e:#}");
                }

                event = "beat";
                thread::sleep(period);
            }
        });

    if spawned.is_err() {
        warn!("Could not spawn the ccu heartbeat thread");
    }
}

pub fn stop() {
    let Some(session) = SESSION.get() else {
        return;
    };

    let Ok(client) = reqwest::blocking::Client::builder()
        .connect_timeout(SHUTDOWN_TIMEOUT)
        .timeout(SHUTDOWN_TIMEOUT)
        .build()
    else {
        return;
    };

    let version = get_local_version().trim().to_string();
    let payload = Beat {
        session,
        event: "stop",
        version: &version,
        os: std::env::consts::OS,
    };

    if let Err(e) = client.post(super::url(PATH)).json(&payload).send() {
        debug!("ccu stop failed: {e}");
    }
}

pub fn stats() -> Result<Stats> {
    super::get_json(PATH)
}
