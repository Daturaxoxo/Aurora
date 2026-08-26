use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use log::*;
use serde::{Deserialize, Serialize};

use crate::classes::info::version::{Version, detect_version};
use crate::utils::get_local_version;
use crate::{config, utils};

const PATH: &str = "/app/ccu";
const TELEMETRY_KEY: &str = "telemetry";
const HEARTBEAT: Duration = Duration::from_mins(10);
const JITTER: f64 = 0.05;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const REGION_UNKNOWN: &str = "unknown";

static SESSION: OnceLock<String> = OnceLock::new();
static REGION: OnceLock<&'static str> = OnceLock::new();
static STOP_CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();

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

pub fn region() -> &'static str {
    if let Some(cached) = REGION.get() {
        return cached;
    }

    let stored = config::get(config::key::GAME_PATH);
    let path = stored.as_str().unwrap_or_default();

    let version = if path.is_empty() {
        Version::Unknown
    } else {
        detect_version(Path::new(path)).unwrap_or_default()
    };

    let name = region_name(version);
    if name != REGION_UNKNOWN {
        let _ = REGION.set(name);
    }

    name
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
