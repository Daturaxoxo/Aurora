use std::path::Path;
use std::process::{self, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use log::*;

use ipc::manifest::{hash_file, LocalManifest, Manifest};
use ipc::protocol::{self, Message};
use reqwest::blocking::Response;
use serde::Deserialize;

use crate::bridge::Bridge;
use crate::MainWindow;

#[cfg(feature = "beta")]
const SKIP_BETA_PHASING_ARG: &str = "--skip-beta-phasing";
#[cfg(feature = "beta")]
const BETA_PHASE_CHECK_URL: &str = "https://beta.getaurora.moe/api/v2/status";
#[cfg(feature = "beta")]
const CURRENT_BETA_PHASE: i32 = 3;

#[cfg(feature = "beta")]
#[allow(dead_code)]
#[derive(Deserialize)]
struct BetaPhaseResponse {
    active: bool,
    phase: i32,
    message: String,
}

static UPDATE_RUNNING: AtomicBool = AtomicBool::new(false);

pub struct UpdateHandler;

impl UpdateHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let args: Vec<String> = std::env::args().collect();
        let mut skip_beta_phasing = false;

        for arg in args {
            match arg.as_str() {
                ipc::POST_UPDATE_ARG => {
                    info!("launched post-update. Sending init_confirmed");
                    std::thread::spawn(Self::send_init_confirmed);
                    return;
                }
                ipc::SKIP_UPDATE_CHECK_ARG => {
                    // Relaunched by the updater after a failed, rolled-back update.
                    warn!("startup update check skipped");
                    return;
                }
                #[cfg(feature = "beta")]
                SKIP_BETA_PHASING_ARG => {
                    info!("skipping beta phasing");
                    skip_beta_phasing = true;
                }

                _ => {}
            }
        }

        #[cfg(feature = "beta")]
        {
            if !skip_beta_phasing {
                let w = window.clone();
                std::thread::spawn(move || Self::run_beta_phase_gate(&w));
                return;
            }
        }

        Self::run_update_check(window, false);
    }

    #[cfg(feature = "beta")]
    fn run_beta_phase_gate(window: &slint::Weak<MainWindow>) {
        match Self::check_beta_phasing() {
            Ok(true) => info!("beta phasing is active"),
            Ok(false) => {
                warn!("beta phasing is not active");
                #[cfg(not(debug_assertions))]
                match Self::update_available() {
                    Ok(true) => {
                        info!("beta phase inactive but an update is available; updating");
                    }
                    Ok(false) | Err(_) => {
                        let w = window.clone();
                        slint::invoke_from_event_loop(move || {
                            if let Some(w) = w.upgrade() {
                                w.set_popup_id("beta-phase-inactive".into());
                                w.set_popup_title("Beta phase inactive".into());
                                w.set_popup_message("The beta phase corresponding to this version is inactive. Please update or download the latest version.".into());
                                w.set_popup_confirm_delay(0);
                                w.set_popup_required_count(0);
                                w.set_popup_checkboxes(slint::ModelRc::default());
                                w.set_popup_active(true);
                            }
                        })
                        .ok();
                        return;
                    }
                }
            }
            Err(e) => {
                error!("failed to check beta phasing: {e}");
                process::exit(0);
            }
        }

        Self::run_update_check(window, false);
    }

    pub fn run_update_check(window: &slint::Weak<MainWindow>, show_toast: bool) {
        if cfg!(debug_assertions) {
            info!("update check skipped in debug build");
            if show_toast {
                Bridge::show_toast(
                    window,
                    "Update checks are disabled in debug builds.",
                    "info",
                );
            }
            return;
        }

        if UPDATE_RUNNING.swap(true, Ordering::SeqCst) {
            if show_toast {
                Bridge::show_toast(window, "An update check is already running.", "info");
            }
            return;
        }

        let w = window.clone();
        std::thread::spawn(move || {
            if let Err(e) = Self::run_update_flow(&w, show_toast) {
                warn!("update flow failed: {e}");
                if show_toast {
                    Bridge::show_toast(&w, "Could not check for updates.", "error");
                }
            }
            UPDATE_RUNNING.store(false, Ordering::SeqCst);
        });
    }

    fn run_update_flow(window: &slint::Weak<MainWindow>, interactive: bool) -> Result<()> {
        let root = ipc::install_root();
        let manifest = Self::fetch_manifest()?;

        Self::self_update_updater(&root, &manifest)?;

        let local = match LocalManifest::load(&root) {
            Ok(Some(local)) => local,
            _ => LocalManifest::build_manifest_from_disk(&root, &manifest),
        };

        if manifest.changed_files(&root, &local).is_empty() {
            info!("no update available");
            if interactive {
                Bridge::show_toast(window, "Aurora is up to date.", "success");
            }
            return Ok(());
        }

        let local_version = shared::utils::get_local_version();
        if Self::is_minor_update(&local_version, &manifest.version) {
            info!(
                "minor update {} -> {} available; updating silently",
                local_version.trim(),
                manifest.version
            );
            Self::begin_locked_update(window);
            if let Err(e) = Self::run_updater(window, true) {
                warn!("silent update failed: {e}");
                Self::set_update_overlay(window, false);
                Self::set_locked(window, false);
                Bridge::show_toast(window, "Update failed. Try again later.", "error");
            }
            return Ok(());
        }

        info!("update {} available; asking the user", manifest.version);
        Self::show_update_popup(window, &manifest.version);
        Ok(())
    }

    fn parse_staple_major(version: &str) -> Option<(u64, u64)> {
        let mut parts = version.trim().split('.');
        let staple = parts.next()?.parse().ok()?;
        let major = parts.next()?.parse().ok()?;
        Some((staple, major))
    }

    fn is_minor_update(local: &str, remote: &str) -> bool {
        match (
            Self::parse_staple_major(local),
            Self::parse_staple_major(remote),
        ) {
            (Some(l), Some(r)) => l == r,
            _ => false,
        }
    }

    pub fn start_update(window: &slint::Weak<MainWindow>) {
        if UPDATE_RUNNING.swap(true, Ordering::SeqCst) {
            return;
        }

        let w = window.clone();
        std::thread::spawn(move || {
            if let Err(e) = Self::run_updater(&w, false) {
                warn!("update failed: {e}");
                Self::set_update_overlay(&w, false);
                Self::set_locked(&w, false);
                Bridge::show_toast(&w, "Update failed. Try again later.", "error");
            }
            UPDATE_RUNNING.store(false, Ordering::SeqCst);
        });
    }

    fn run_updater(window: &slint::Weak<MainWindow>, silent: bool) -> Result<()> {
        let root = ipc::install_root();

        let listener =
            protocol::listen(ipc::MAIN_PIPE_NAME).context("failed to open updater pipe")?;

        let updater_path = root.join(ipc::UPDATER_EXE);
        Command::new(&updater_path)
            .current_dir(&root)
            .spawn()
            .with_context(|| format!("failed to launch {}", updater_path.display()))?;

        let (tx, rx) = mpsc::channel::<Message>();
        std::thread::spawn(move || {
            let Ok(mut stream) = protocol::accept(&listener) else {
                return;
            };
            while let Ok(msg) = protocol::read_message(&mut stream) {
                if tx.send(msg).is_err() {
                    return;
                }
            }
        });

        let mut locked = false;
        let mut last_heartbeat = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(Message::Lock) => {
                    info!("updater: update in progress, locking UI");
                    locked = true;
                    last_heartbeat = Instant::now();
                    Self::set_locked(window, true);
                    Self::set_update_overlay(window, true);
                }
                Ok(Message::Heartbeat) => last_heartbeat = Instant::now(),
                Ok(Message::Progress {
                    file_index,
                    file_count,
                    bytes_done,
                    bytes_total,
                }) => {
                    last_heartbeat = Instant::now();
                    Self::set_update_progress(
                        window,
                        file_index,
                        file_count,
                        bytes_done,
                        bytes_total,
                    );
                }
                Ok(Message::Unlock) => {
                    info!("updater: update finished");
                    if silent {
                        info!("silent update applied; restarting Aurora");
                        Self::restart_app(window);
                        return Ok(());
                    }
                    Self::set_update_overlay(window, false);
                    Self::set_locked(window, false);
                    Bridge::show_toast(window, "Aurora has been updated.", "success");
                    return Ok(());
                }
                Ok(Message::NoUpdate) => {
                    // The check already found changes, so this only happens if a
                    // new manifest landed while the popup was open.
                    info!("updater: no update available");
                    Self::set_update_overlay(window, false);
                    Self::set_locked(window, false);
                    return Ok(());
                }
                Ok(Message::CloseNow) => {
                    info!("updater: Aurora.exe is being replaced, exiting");
                    slint::invoke_from_event_loop(|| {
                        let _ = slint::quit_event_loop();
                    })
                    .ok();
                    return Ok(());
                }
                Ok(Message::Error { message }) => {
                    error!("updater reported an error: {message}");
                    Self::set_update_overlay(window, false);
                    Self::set_locked(window, false);
                    Bridge::show_toast(window, "Update failed. Try again later.", "error");
                    return Ok(());
                }
                Ok(Message::InitConfirmed) => {} // not expected on this pipe
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if locked && last_heartbeat.elapsed() > ipc::HEARTBEAT_TIMEOUT {
                        error!("updater heartbeat lost; auto-unlocking UI");
                        Self::set_update_overlay(window, false);
                        Self::set_locked(window, false);
                        Bridge::show_toast(
                            window,
                            "Update interrupted. You can retry from the launch menu.",
                            "error",
                        );
                        return Ok(());
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if locked {
                        error!("updater connection lost while locked; auto-unlocking UI");
                        Self::set_update_overlay(window, false);
                        Self::set_locked(window, false);
                        Bridge::show_toast(
                            window,
                            "Update interrupted. You can retry from the launch menu.",
                            "error",
                        );
                    }
                    return Ok(());
                }
            }
        }
    }

    fn self_update_updater(root: &Path, manifest: &Manifest) -> Result<()> {
        let updater_path = root.join(ipc::UPDATER_EXE);
        let local_hash = if updater_path.exists() {
            hash_file(&updater_path).context("failed to hash local updater")?
        } else {
            String::new()
        };
        if local_hash == manifest.updater_hash {
            return Ok(());
        }

        info!("updater is outdated; downloading new version");
        let entry = manifest
            .files
            .iter()
            .find(|f| f.path == ipc::UPDATER_EXE)
            .ok_or_else(|| anyhow!("manifest has no entry for {}", ipc::UPDATER_EXE))?;

        let bytes = reqwest::blocking::get(&entry.url)
            .and_then(reqwest::blocking::Response::error_for_status)
            .with_context(|| format!("failed to download {}", entry.url))?
            .bytes()
            .context("failed to read updater download")?;

        let actual = ipc::manifest::hash_bytes(&bytes);
        if actual != manifest.updater_hash {
            return Err(anyhow!(
                "updater hash mismatch: expected {}, got {actual}",
                manifest.updater_hash
            ));
        }

        let tmp = root.join(format!("{}.tmp", ipc::UPDATER_EXE));
        std::fs::write(&tmp, &bytes).context("failed to write updater .tmp")?;
        std::fs::rename(&tmp, &updater_path).context("failed to swap in new updater")?;
        info!("updater self-update complete.");
        Ok(())
    }

    fn fetch_manifest() -> Result<Manifest> {
        let mut last_err = anyhow!("no manifest sources configured");
        for url in [ipc::MANIFEST_URL_PRIMARY, ipc::MANIFEST_URL_FALLBACK] {
            let result = reqwest::blocking::get(url)
                .and_then(Response::error_for_status)
                .and_then(Response::json::<Manifest>);
            match result {
                Ok(manifest) => return Ok(manifest),
                Err(e) => {
                    warn!("manifest fetch failed from {url}: {e}");
                    last_err = e.into();
                }
            }
        }
        Err(last_err.context("all manifest sources failed"))
    }

    fn show_update_popup(window: &slint::Weak<MainWindow>, version: &str) {
        let message =
            format!("Aurora has detected a new update ({version}), do you want to update?");
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                w.set_popup_id("update-popup".into());
                w.set_popup_title("Update available".into());
                w.set_popup_message(message.into());
                w.set_popup_confirm_delay(0);
                w.set_popup_required_count(0);
                w.set_popup_checkboxes(slint::ModelRc::default());
                w.set_popup_active(true);
            }
        })
        .ok();
    }

    fn begin_locked_update(window: &slint::Weak<MainWindow>) {
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            crate::classes::logwindow::hide();
            if let Some(w) = w.upgrade() {
                w.set_launch_disabled(true);
                w.set_launch_button_text("Updating...".into());
            }
        })
        .ok();
        Self::set_update_overlay(window, true);
    }

    fn set_update_overlay(window: &slint::Weak<MainWindow>, active: bool) {
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                if active {
                    w.set_progress_overlay_title("Updating Aurora".into());
                    w.set_progress_overlay_progress(0.0);
                    w.set_progress_overlay_text("Preparing update...".into());
                    w.set_progress_overlay_cancellable(false);
                }
                w.set_progress_overlay_active(active);
            }
        })
        .ok();
    }

    #[allow(clippy::cast_possible_truncation)]
    fn set_update_progress(
        window: &slint::Weak<MainWindow>,
        file_index: u32,
        file_count: u32,
        bytes_done: u64,
        bytes_total: u64,
    ) {
        let count = file_count.max(1);
        #[allow(clippy::cast_precision_loss)]
        let file_frac = if bytes_total > 0 {
            (bytes_done as f64 / bytes_total as f64).min(1.0)
        } else {
            0.0
        };
        let overall = ((f64::from(file_index) + file_frac) / f64::from(count)).clamp(0.0, 1.0);
        let text = if file_index >= file_count {
            "Finishing update...".to_string()
        } else {
            format!(
                "Downloading file {} of {}",
                file_index.saturating_add(1),
                file_count
            )
        };

        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                w.set_progress_overlay_progress(overall as f32);
                w.set_progress_overlay_text(text.into());
            }
        })
        .ok();
    }

    fn restart_app(window: &slint::Weak<MainWindow>) {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                error!("could not resolve current exe for restart: {e}");
                Self::restart_failed(window);
                return;
            }
        };

        if let Err(e) = Command::new(exe)
            .arg(ipc::RELAUNCH_ARG)
            .current_dir(ipc::install_root())
            .spawn()
        {
            error!("failed to respawn Aurora after update: {e}");
            Self::restart_failed(window);
            return;
        }

        slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        })
        .ok();
    }

    fn restart_failed(window: &slint::Weak<MainWindow>) {
        Self::set_update_overlay(window, false);
        Self::set_locked(window, false);
        Bridge::show_toast(
            window,
            "Aurora has been updated. Please restart it manually.",
            "info",
        );
    }

    fn set_locked(window: &slint::Weak<MainWindow>, locked: bool) {
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                w.set_launch_disabled(locked);
                w.set_launch_button_text(if locked { "Updating..." } else { "Launch" }.into());
            }
        })
        .ok();
    }

    fn send_init_confirmed() {
        for _ in 0..10 {
            if let Ok(mut stream) = protocol::connect(ipc::INIT_PIPE_NAME) {
                if protocol::write_message(&mut stream, &Message::InitConfirmed).is_ok() {
                    info!("init_confirmed sent");
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        warn!("could not deliver init_confirmed to the updater");
    }

    #[cfg(feature = "beta")]
    #[cfg(not(debug_assertions))]
    fn update_available() -> Result<bool> {
        let root = ipc::install_root();
        let manifest = Self::fetch_manifest()?;
        let local = match LocalManifest::load(&root) {
            Ok(Some(local)) => local,
            _ => LocalManifest::build_manifest_from_disk(&root, &manifest),
        };
        Ok(!manifest.changed_files(&root, &local).is_empty())
    }

    #[cfg(feature = "beta")]
    fn check_beta_phasing() -> Result<bool> {
        let res: BetaPhaseResponse = reqwest::blocking::get(BETA_PHASE_CHECK_URL)
            .with_context(|| "Couldn't connect to beta phasing endpoint")?
            .json()
            .with_context(|| "Couldn't parse JSON from beta phasing endpoint")?;

        Ok(res.active && res.phase == CURRENT_BETA_PHASE)
    }
}
