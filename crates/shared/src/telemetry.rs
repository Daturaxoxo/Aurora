use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::mpsc::{sync_channel, SyncSender},
    time::Duration,
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
        paths::{get_version_paths, CLIENT_PAK_DIR},
        version::{detect_version, BypassMethod},
    },
    config::{self, get_all_configs, key},
    pathfind::get_game_directory,
    utils::{get_local_version, read_dir_recursive},
};
use crate::{logger::get_latest_logs, utils};

const TELEMETRY_ENDPOINT: &str = "https://beta.getaurora.moe/api/v2/telemetry";

pub fn export_telemetry() -> Result<()> {
    let Some(logs) = get_latest_logs() else {
        error!("Failed to get logs for telemetry");
        return Ok(());
    };

    let utc_timestamp = chrono::Utc::now();
    let local_timestamp = chrono::Local::now();
    let mut file = std::fs::File::create_buffered(format!(
        "./Logs/aurora_telemetry_{}.aulog",
        utc_timestamp.format("%d-%m-%Y %H-%M-%S")
    ))
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
        iana_time_zone::get_timezone()?
    )?;

    writeln!(file, "CPU:                 {}", CpuInfo::new().model)?;

    writeln!(
        file,
        "Memory:              {}",
        utils::format_bytes(sys.total_memory())
    )?;

    writeln!(file)?;

    writeln!(file, "=== Environment ===")?;

    #[cfg(target_os = "windows")]
    writeln!(file, "Windows Defender:    {}", windows_defender_on())?;

    writeln!(file)?;

    writeln!(file, "=== Aurora Configuration ===")?;

    let configs = get_all_configs();
    for (k, v) in configs {
        writeln!(file, "{k}: {v}")?;
    }

    writeln!(file)?;

    writeln!(file, "=== Game Information ===")?;

    let game_path = get_game_directory()?;
    writeln!(file, "Game Path:       {}", game_path.display())?;

    let version = detect_version(game_path.as_path()).unwrap_or_default();
    writeln!(file, "Version:         {version}")?;

    let write_access = std::fs::metadata(&game_path).is_ok();
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
                total_size += entry.metadata()?.file_size();
            }
            unix => {
                total_size += entry.metadata()?.size();
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
        }

        if path.is_file() {
            let metadata = path.metadata()?;
            let size = cfg_select! {
                windows => metadata.file_size(),
                unix => metadata.size(),
            };

            let mtime = cfg_select! {
                windows => metadata.last_write_time(),
                unix => metadata.st_atime(),
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
    let engine_method = BypassMethod::from_num(engine_method_raw)?;
    let vpaths = get_version_paths(game_path.as_path(), version, engine_method);
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
    let entries = std::fs::read_dir(&addon_dir)?;
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            for entry in std::fs::read_dir(entry.path())? {
                let entry = entry?;
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

    writeln!(file)?;

    writeln!(file, "=== Session Log ===")?;
    file.write_all(logs.as_bytes())?;

    file.flush()?;

    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_defender_on() -> bool {
    use windows_sys::Win32::{
        Foundation::S_OK,
        System::SecurityCenter::{
            WscGetSecurityProviderHealth, WSC_SECURITY_PROVIDER_ANTIVIRUS,
            WSC_SECURITY_PROVIDER_HEALTH_GOOD,
        },
    };

    unsafe {
        let mut health = 0;

        let hr = WscGetSecurityProviderHealth(
            WSC_SECURITY_PROVIDER_ANTIVIRUS.unchecked_cast(),
            &raw mut health,
        );

        if hr == S_OK {
            health == WSC_SECURITY_PROVIDER_HEALTH_GOOD
        } else {
            warn!("Failed to query Windows Defender health: hr = {hr}");
            false
        }
    }
}

pub struct ErrorEvent {
    pub timestamp: String,
    pub module: String,
    pub message: String,
}

pub(crate) fn spawn_error_worker() -> SyncSender<ErrorEvent> {
    let (tx, rx) = sync_channel::<ErrorEvent>(256);

    let spawned = std::thread::Builder::new()
        .name("error-telemetry".into())
        .spawn(move || {
            let clean = get_local_version().trim().to_string();
            let client = reqwest::blocking::Client::builder()
                .user_agent(format!("AuroraLauncher/{clean}"))
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default();

            let version = get_local_version().trim().to_string();

            let os_main = System::name().unwrap_or_else(|| "Unknown".to_string());
            let os_version = System::os_version()
                .or_else(System::kernel_version)
                .unwrap_or_else(|| "Unknown".to_string());

            while let Ok(event) = rx.recv() {
                let payload = serde_json::json!({
                    "message": event.message,
                    "module": event.module,
                    "version": version,
                    "timestamp": event.timestamp,
                    "os_main": os_main,
                    "os_version": os_version,
                });

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
