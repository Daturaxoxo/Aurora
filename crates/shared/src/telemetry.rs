use std::{
    cell::Cell,
    collections::{HashSet, VecDeque},
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io::Write as _,
    path::{Path, PathBuf},
    sync::mpsc::{SyncSender, sync_channel},
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "windows")]
use std::os::windows::fs::MetadataExt;

#[cfg(target_os = "linux")]
use std::os::{linux::fs::MetadataExt as _, unix::fs::MetadataExt as _};

use anyhow::{Context, Result};
use cpu_info::CpuInfo;
use log::*;
use sysinfo::{Disks, RefreshKind, System};

use crate::{
    classes::info::{
        paths::{CLIENT_PAK_DIR, get_version_paths},
        version::{BypassMethod, detect_distribution, detect_version},
    },
    config::{self, get_all_configs, key},
    pathfind::get_game_directory,
    utils::{get_local_version, read_dir_recursive},
};
use crate::{logger::get_latest_logs, utils};

#[cfg(any(debug_assertions, feature = "beta"))]
const TELEMETRY_ENDPOINT: &str = "https://beta.getaurora.moe/api/v2/telemetry";

#[cfg(not(any(debug_assertions, feature = "beta")))]
const TELEMETRY_ENDPOINT: &str = "https://api.getaurora.moe/v2/telemetry";

pub fn export_telemetry() -> Result<()> {
    let Some(logs) = get_latest_logs() else {
        error!("Failed to get logs for telemetry");
        return Ok(());
    };

    let utc_timestamp = chrono::Utc::now();
    let local_timestamp = chrono::Local::now();
    let log_dir = crate::logger::log_dir();
    std::fs::create_dir_all(&log_dir).with_context(|| "Couldn't create the log directory")?;
    let mut file = std::fs::File::create_buffered(log_dir.join(format!(
        "aurora_telemetry_{}.aulog",
        utc_timestamp.format("%d-%m-%Y %H-%M-%S")
    )))
    .with_context(|| "Couldn't create telemetry file")?;

    // Write a UTF-8 BOM so text editors don't misinterpret the ASCII bytes as UTF-16LE
    file.write_all(b"\xEF\xBB\xBF")?;

    writeln!(file, "Aurora V2 Telemetry Export")?;
    writeln!(
        file,
        "Generated: {} (local) | {} (UTC)",
        local_timestamp.format("%d/%m/%Y %H-%M-%S"),
        utc_timestamp.format("%d/%m/%Y %H-%M-%S")
    )?;

    writeln!(file)?;

    let mut sys = System::new_with_specifics(RefreshKind::everything().without_processes());
    sys.refresh_specifics(RefreshKind::everything().without_processes());
    writeln!(file, "=== System Information ===")?;

    writeln!(file, "Aurora Version:      {}", utils::get_local_version())?;

    writeln!(
        file,
        "OS:                  {}",
        System::name().unwrap_or_else(|| "Unknown".to_string())
    )?;

    writeln!(
        file,
        "Kernel:              {}",
        System::kernel_version().unwrap_or_else(|| "Unknown".to_string())
    )?;

    writeln!(file, "Architecture:        {}", System::cpu_arch())?;

    writeln!(
        file,
        "Hostname:            {}",
        System::host_name().unwrap_or_else(|| "Unknown".to_string())
    )?;

    writeln!(
        file,
        "Timezone:            {}",
        iana_time_zone::get_timezone().unwrap_or_else(|s| format!("Unknown ({s})"))
    )?;

    writeln!(file, "CPU:                 {}", CpuInfo::new().model)?;

    writeln!(
        file,
        "Memory:              {}",
        utils::format_bytes(sys.total_memory())
    )?;

    writeln!(file)?;

    writeln!(file, "=== Aurora Configuration ===")?;

    let configs = get_all_configs();
    for (k, v) in configs {
        writeln!(file, "{k}: {v}")?;
    }

    writeln!(file)?;

    writeln!(file, "=== Game Information ===")?;

    let game_path = get_game_directory().with_context(|| "Failed to get game directory")?;
    writeln!(file, "Game Path:       {}", game_path.display())?;

    let version = detect_version(game_path.as_path()).unwrap_or_default();
    writeln!(file, "Version:         {version}")?;

    let distro = detect_distribution(game_path.as_path());
    writeln!(file, "Distribution:    {distro:?}")?;

    let write_access = probe_write_access(game_path.as_path());
    writeln!(file, "Write Access:    {write_access}")?;

    let disks = Disks::new_with_refreshed_list();
    for d in &disks {
        writeln!(
            file,
            "Disk {}:     {} free of {}",
            d.mount_point().display(),
            utils::format_bytes(d.available_space()),
            utils::format_bytes(d.total_space())
        )?;
    }

    let mods_path = game_path.join(CLIENT_PAK_DIR);
    let scan = read_dir_recursive(&mods_path);
    let mut file_count = 0;
    let mut total_size = 0u64;
    for entry in scan {
        if !entry.file_type().is_file() {
            continue;
        }
        file_count += 1;
        cfg_select! {
            windows => {
                total_size += entry.metadata().map_or(0, |metadata| metadata.file_size());
            }
            unix => {
                total_size += entry.metadata().map_or(0, |metadata| metadata.size());
            }
        };
    }
    writeln!(
        file,
        "Mods Loaded: {} file(s), {} total",
        file_count,
        utils::format_bytes(total_size)
    )?;

    writeln!(file)?;

    writeln!(file, "=== File Status ===")?;
    let mut print_entry = |name: &str, path: &Path| -> Result<()> {
        if !path.exists() {
            writeln!(file, "{} MISSING [{}]", name, path.display())?;
            return Ok(());
        }

        if path.is_file() {
            let metadata = path.metadata()?;
            let size = cfg_select! {
                windows => metadata.file_size(),
                unix => metadata.size(),
            };

            let mtime = cfg_select! {
                windows => metadata.last_write_time(),
                unix => metadata.st_mtime(),
            };
            writeln!(
                file,
                "{} EXISTS ({}, modified {}) [{}]",
                name,
                utils::format_bytes(size),
                mtime,
                path.display()
            )?;
        } else if path.is_dir() {
            let count = path.read_dir()?.count();
            writeln!(
                file,
                "{} DIR ({} file(s)) [{}]",
                name,
                count,
                path.display()
            )?;
        } else {
            writeln!(file, "{}: MISSING [{}]", name, path.display())?;
        }
        Ok(())
    };

    let engine_method_raw = match config::get(key::ENGINE_METHOD).as_i64() {
        Some(v) => v,
        None => config::get(key::ENGINE_METHOD)
            .as_str()
            .unwrap_or("0")
            .parse::<i64>()?,
    };
    let engine_method = BypassMethod::resolve(engine_method_raw, version)?;
    let vpaths = get_version_paths(game_path.as_path(), version, distro, engine_method);
    for (name, path) in vpaths.all_dll_targets() {
        print_entry(&name, path.as_path())?;
    }

    print_entry("Mod folder", mods_path.as_path())?;

    let config_path = config::config_file_path();
    print_entry("Config file", config_path.as_path())?;

    let addon_dir = utils::get_bin_path()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Addons");

    // TODO: should use the read_dir_recursive fn from utils but i don't
    // get paid enough....... it works
    match std::fs::read_dir(&addon_dir) {
        Ok(entries) => {
            for entry in entries {
                let Ok(entry) = entry else { continue };
                if entry.file_type()?.is_dir() {
                    for entry in std::fs::read_dir(entry.path())? {
                        let Ok(entry) = entry else { continue };
                        if entry.file_type()?.is_file() {
                            let name = entry.file_name().into_string().unwrap_or_default();
                            if std::path::Path::new(&name)
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
                            {
                                let path = entry.path();
                                print_entry(&format!("ENABLED: {name}"), path.as_path())?;
                            } else if std::path::Path::new(&name)
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("disabled"))
                            {
                                let path = entry.path();
                                print_entry(&format!("DISABLED: {name}"), path.as_path())?;
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            warn!("Could not read the addon directory: {e}");
            print_entry("Addon folder", addon_dir.as_path())?;
        }
    }

    writeln!(file)?;

    writeln!(file, "=== Session Log ===")?;
    file.write_all(logs.as_bytes())?;

    file.flush()?;

    Ok(())
}

fn probe_write_access(dir: &Path) -> bool {
    let probe = dir.join(format!(".aurora_write_probe_{}", std::process::id()));

    match std::fs::File::create(&probe) {
        Ok(_) => {
            if let Err(e) = std::fs::remove_file(&probe) {
                warn!("Could not remove the write probe {}: {e}", probe.display());
            }
            true
        }
        Err(_) => false,
    }
}

fn scrub_username(text: &str) -> String {
    let user = dirs::home_dir()
        .and_then(|home| {
            home.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();

    if user.is_empty() {
        return text.to_string();
    }

    text.replace(&user, "<user>")
}

pub struct ErrorEvent {
    pub timestamp: String,
    pub module: String,
    pub message: String,
}

const MAX_EVENTS_PER_SESSION: usize = 50; // maximum amount of telemetry logs an user can give in a session (done so our api doesn't die when an error loop happens) -datura
const SEND_BURST: usize = 3; // telemetry logs allowed at once, basically: "maximum of X errors within {SEND_WINDOW} seconds"
const SEND_WINDOW: Duration = Duration::from_secs(5); // the time frame SEND_BURST uses

thread_local! {
    static REPORTING: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn is_reporting_thread() -> bool {
    REPORTING.with(Cell::get)
}

struct Budget {
    seen: HashSet<u64>,
    recent: VecDeque<Instant>,
}

impl Budget {
    fn new() -> Self {
        Self {
            seen: HashSet::new(),
            recent: VecDeque::with_capacity(SEND_BURST),
        }
    }

    fn admit(&mut self, event: &ErrorEvent) -> bool {
        if self.seen.len() >= MAX_EVENTS_PER_SESSION {
            return false;
        }
        let mut hasher = DefaultHasher::new();
        event.module.hash(&mut hasher);
        event.message.hash(&mut hasher);
        self.seen.insert(hasher.finish())
    }

    fn pace(&mut self) {
        loop {
            while self
                .recent
                .front()
                .is_some_and(|at| at.elapsed() >= SEND_WINDOW)
            {
                self.recent.pop_front();
            }

            if self.recent.len() < SEND_BURST {
                break;
            }
            let Some(oldest) = self.recent.front().copied() else {
                break;
            };
            thread::sleep(SEND_WINDOW.saturating_sub(oldest.elapsed()));
        }

        self.recent.push_back(Instant::now());
    }
}

pub(crate) fn spawn_error_worker() -> SyncSender<ErrorEvent> {
    let (tx, rx) = sync_channel::<ErrorEvent>(256);

    let spawned = std::thread::Builder::new()
        .name("error-telemetry".into())
        .spawn(move || {
            REPORTING.with(|reporting| reporting.set(true));

            let mut budget = Budget::new();
            let version = get_local_version();
            let client = reqwest::blocking::Client::builder()
                .user_agent(format!("AuroraLauncher/{version}"))
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default();

            let os_main = System::name().unwrap_or_else(|| "Unknown".to_string());
            let os_version = System::os_version()
                .or_else(System::kernel_version)
                .unwrap_or_else(|| "Unknown".to_string());

            while let Ok(event) = rx.recv() {
                if config::get(config::key::TELEMETRY_OPT_OUT)
                    .as_bool()
                    .unwrap_or(false)
                {
                    continue;
                }

                if !budget.admit(&event) {
                    continue;
                }

                let payload = serde_json::json!({
                    "message": scrub_username(&event.message),
                    "module": event.module,
                    "version": version,
                    "timestamp": event.timestamp,
                    "os_main": os_main,
                    "os_version": os_version,
                    "region": crate::api::ccu::region(),
                });

                budget.pace();

                match client.post(TELEMETRY_ENDPOINT).json(&payload).send() {
                    Ok(res) if !res.status().is_success() => {
                        eprintln!("error telemetry: server returned HTTP {}", res.status());
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("error telemetry: failed to send error: {e}"),
                }
            }
        });

    if spawned.is_err() {
        eprintln!("error telemetry: failed to spawn worker thread");
    }

    tx
}
