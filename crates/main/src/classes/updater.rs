#[cfg(windows)]
use std::io;
#[cfg(windows)]
use std::path::Path;
use std::process::Command;
#[cfg(windows)]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use log::*;

use ipc::manifest::hash_file;
#[cfg(windows)]
use ipc::manifest::{LocalManifest, Manifest};
use ipc::protocol::{self, Message};
use reqwest::blocking::Response;
#[cfg(feature = "beta")]
use serde::Deserialize;

use crate::bridge::Bridge;
use crate::{LaunchState, MainWindow};

#[cfg(feature = "beta")]
const SKIP_BETA_PHASING_ARG: &str = "--skip-beta-phasing";
#[cfg(feature = "beta")]
const BETA_PHASE_CHECK_URL: &str = "https://beta.getaurora.moe/api/v2/status";
#[cfg(feature = "beta")]
const CURRENT_BETA_PHASE: i32 = 5;

#[cfg(feature = "beta")]
#[allow(dead_code)]
#[derive(Deserialize)]
struct BetaPhaseResponse {
    active: bool,
    phase: i32,
    message: String,
}

static UPDATE_RUNNING: AtomicBool = AtomicBool::new(false);
static UI_LOCKED: AtomicBool = AtomicBool::new(false);

struct UpdateRunningGuard;

impl Drop for UpdateRunningGuard {
    fn drop(&mut self) {
        UPDATE_RUNNING.store(false, Ordering::SeqCst);
    }
}

pub struct UpdateHandler;

impl UpdateHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let args: Vec<String> = std::env::args().collect();
        #[cfg(feature = "beta")]
        let mut skip_beta_phasing = false;

        for arg in args {
            match arg.as_str() {
                ipc::POST_UPDATE_ARG => {
                    info!("launched post-update. Sending init_confirmed");
                    std::thread::spawn(Self::send_init_confirmed);
                    return;
                }
                ipc::SKIP_UPDATE_CHECK_ARG => {
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
                if matches!(Self::update_available(), Ok(true)) {
                    info!("beta phase inactive but an update is available; updating");
                } else {
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
            Err(e) => {
                warn!("could not reach the beta phasing endpoint: {e}; continuing");
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
            let _running = UpdateRunningGuard;
            if let Err(e) = Self::run_update_flow(&w, show_toast) {
                warn!("update flow failed: {e}");
                if show_toast {
                    Bridge::show_toast(&w, "Could not check for updates.", "error");
                }
            }
        });
    }

    #[cfg(windows)]
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

    #[cfg(target_os = "linux")]
    fn run_update_flow(window: &slint::Weak<MainWindow>, interactive: bool) -> Result<()> {
        let Some(appimage) = ipc::appimage_path() else {
            info!("not running from an AppImage; self-update is unavailable");
            if interactive {
                Bridge::show_toast(
                    window,
                    "Self-update is only available in the AppImage build.",
                    "info",
                );
            }
            return Ok(());
        };

        let manifest = Self::fetch_linux_manifest()?;
        let current = hash_file(&appimage)
            .with_context(|| format!("failed to hash {}", appimage.display()))?;
        if current == manifest.appimage.sha256 {
            info!("no update available");
            if interactive {
                Bridge::show_toast(window, "Aurora is up to date.", "success");
            }
            return Ok(());
        }

        if !Self::appimage_is_replaceable(&appimage) {
            warn!(
                "an update is available but {} cannot be replaced",
                appimage.display()
            );
            Self::show_manual_update_popup(window, &manifest.version);
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
            if let Err(e) = Self::run_appimage_update(window, true) {
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
            warn!("start_update ignored: an update is already running");
            Bridge::show_toast(window, "An update is already running.", "info");
            return;
        }

        let w = window.clone();
        std::thread::spawn(move || {
            let _running = UpdateRunningGuard;
            #[cfg(windows)]
            let result = Self::run_updater(&w, false);
            #[cfg(target_os = "linux")]
            let result = Self::run_appimage_update(&w, false);

            if let Err(e) = result {
                warn!("update failed: {e}");
                Self::set_update_overlay(&w, false);
                Self::set_locked(&w, false);
                Bridge::show_toast(&w, "Update failed. Try again later.", "error");
            }
        });
    }

    #[cfg(windows)]
    fn run_updater(window: &slint::Weak<MainWindow>, silent: bool) -> Result<()> {
        let root = ipc::install_root();

        let listener =
            protocol::listen(ipc::MAIN_PIPE_NAME).context("failed to open updater pipe")?;

        let updater_path = root.join(ipc::UPDATER_EXE);
        Command::new(&updater_path)
            .current_dir(&root)
            .spawn()
            .with_context(|| format!("failed to launch {}", updater_path.display()))?;

        let stream = match protocol::accept_timeout(&listener, ipc::UPDATER_CONNECT_TIMEOUT) {
            Ok(stream) => Arc::new(stream),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                Self::abort_update_session(window, "the updater never connected");
                return Ok(());
            }
            Err(e) => {
                Self::abort_update_session(
                    window,
                    &format!("failed to accept the updater connection: {e}"),
                );
                return Ok(());
            }
        };

        let rx = protocol::spawn_reader(stream);

        loop {
            match rx.recv_timeout(ipc::HEARTBEAT_TIMEOUT) {
                Ok(Ok(Message::Hello)) => info!("updater: connected"),
                Ok(Ok(Message::Lock)) => {
                    info!("updater: update in progress, locking UI");
                    Self::set_locked(window, true);
                    Self::set_update_overlay(window, true);
                }
                Ok(Ok(Message::Heartbeat | Message::InitConfirmed)) => {}
                Ok(Ok(Message::Progress {
                    file_index,
                    file_count,
                    bytes_done,
                    bytes_total,
                })) => {
                    Self::set_update_progress(
                        window,
                        file_index,
                        file_count,
                        bytes_done,
                        bytes_total,
                    );
                }
                Ok(Ok(Message::Unlock)) => {
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
                Ok(Ok(Message::NoUpdate)) => {
                    info!("updater: no update available");
                    Self::set_update_overlay(window, false);
                    Self::set_locked(window, false);
                    return Ok(());
                }
                Ok(Ok(Message::CloseNow)) => {
                    info!("updater: Aurora.exe is being replaced, exiting");
                    slint::invoke_from_event_loop(|| {
                        let _ = slint::quit_event_loop();
                    })
                    .ok();
                    return Ok(());
                }
                Ok(Ok(Message::Error { message })) => {
                    error!("updater reported an error: {message}");
                    Self::set_update_overlay(window, false);
                    Self::set_locked(window, false);
                    Bridge::show_toast(window, "Update failed. Try again later.", "error");
                    return Ok(());
                }
                Ok(Err(e)) => {
                    Self::abort_update_session(
                        window,
                        &format!("the updater connection failed: {e}"),
                    );
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    Self::abort_update_session(window, "the updater stopped sending heartbeats");
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    Self::abort_update_session(window, "the updater closed the connection");
                    return Ok(());
                }
            }
        }
    }

    #[cfg(windows)]
    fn abort_update_session(window: &slint::Weak<MainWindow>, reason: &str) {
        error!("update session ended abnormally: {reason}; unlocking UI");
        UPDATE_RUNNING.store(false, Ordering::SeqCst);
        Self::set_update_overlay(window, false);
        Self::set_locked(window, false);
        Bridge::show_toast(
            window,
            "Update interrupted. You can retry from the launch menu.",
            "error",
        );
    }

    #[cfg(target_os = "linux")]
    fn run_appimage_update(window: &slint::Weak<MainWindow>, silent: bool) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let appimage =
            ipc::appimage_path().ok_or_else(|| anyhow!("not running from an AppImage"))?;
        let manifest = Self::fetch_linux_manifest()?;

        Self::set_locked(window, true);
        Self::set_update_overlay(window, true);

        let tmp = appimage.with_file_name(format!("{}.new", ipc::APPIMAGE_NAME));
        let _ = std::fs::remove_file(&tmp);

        let sources = manifest.appimage.download_urls();
        if let Err(e) = Self::download_from_any(window, &sources, &tmp) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }

        let actual = hash_file(&tmp).context("failed to hash the downloaded AppImage")?;
        if actual != manifest.appimage.sha256 {
            let _ = std::fs::remove_file(&tmp);
            return Err(anyhow!(
                "AppImage hash mismatch: expected {}, got {actual}",
                manifest.appimage.sha256
            ));
        }

        let mode = std::fs::metadata(&appimage).map_or(0o755, |m| m.permissions().mode());
        if let Err(e) = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode)) {
            let _ = std::fs::remove_file(&tmp);
            return Err(anyhow!("failed to make the new AppImage executable: {e}"));
        }

        if let Err(e) = std::fs::rename(&tmp, &appimage) {
            let _ = std::fs::remove_file(&tmp);
            return Err(anyhow!("failed to replace {}: {e}", appimage.display()));
        }
        info!(
            "replaced {} with version {}",
            appimage.display(),
            manifest.version
        );

        if silent {
            info!("silent update applied; restarting Aurora");
        }
        Self::relaunch_appimage(window, &appimage);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn download_from_any(
        window: &slint::Weak<MainWindow>,
        urls: &[String],
        dst: &std::path::Path,
    ) -> Result<()> {
        let mut last_err = anyhow!("no download sources available");
        for (i, url) in urls.iter().enumerate() {
            info!("downloading {url}");
            match Self::download_with_progress(window, url, dst) {
                Ok(()) => {
                    if i > 0 {
                        info!("fell back to {url}");
                    }
                    return Ok(());
                }
                Err(e) => {
                    warn!("download failed from {url}: {e}");
                    let _ = std::fs::remove_file(dst);
                    last_err = e;
                }
            }
        }
        Err(last_err.context("all download sources failed"))
    }

    #[cfg(target_os = "linux")]
    fn download_with_progress(
        window: &slint::Weak<MainWindow>,
        url: &str,
        dst: &std::path::Path,
    ) -> Result<()> {
        use std::io::{Read, Write};

        let mut response = reqwest::blocking::get(url)
            .and_then(Response::error_for_status)
            .with_context(|| format!("failed to download {url}"))?;
        let total = response.content_length().unwrap_or(0);

        let mut file = std::fs::File::create(dst)
            .with_context(|| format!("failed to create {}", dst.display()))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut done: u64 = 0;
        let mut last_progress = std::time::Instant::now();

        loop {
            let n = response.read(&mut buf).context("download interrupted")?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .with_context(|| format!("failed to write {}", dst.display()))?;
            done += n as u64;

            if last_progress.elapsed() >= Duration::from_millis(100) {
                last_progress = std::time::Instant::now();
                Self::set_update_progress(window, 0, 1, done, total);
            }
        }
        file.sync_all().context("failed to flush the download")?;
        Self::set_update_progress(window, 1, 1, 0, 0);
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn appimage_is_replaceable(appimage: &std::path::Path) -> bool {
        let Some(parent) = appimage.parent() else {
            return false;
        };
        let probe = parent.join(format!(".{}.probe", ipc::APPIMAGE_NAME));
        match std::fs::File::create(&probe) {
            Ok(_) => {
                let _ = std::fs::remove_file(&probe);
                true
            }
            Err(e) => {
                warn!("{} is not writable: {e}", parent.display());
                false
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn relaunch_appimage(window: &slint::Weak<MainWindow>, appimage: &std::path::Path) {
        if let Err(e) = Command::new(appimage).arg(ipc::RELAUNCH_ARG).spawn() {
            error!("failed to relaunch the updated AppImage: {e}");
            Self::restart_failed(window);
            return;
        }
        slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        })
        .ok();
    }

    #[cfg(target_os = "linux")]
    fn fetch_linux_manifest() -> Result<ipc::manifest::LinuxManifest> {
        let mut last_err = anyhow!("no manifest sources configured");
        for url in ipc::manifest_urls() {
            let result = reqwest::blocking::get(url)
                .and_then(Response::error_for_status)
                .and_then(Response::json::<ipc::manifest::LinuxManifest>);
            match result {
                Ok(manifest) => {
                    if let Err(e) = manifest.appimage.validate_url() {
                        warn!("manifest from {url} rejected: {e}");
                        last_err = anyhow!(e);
                        continue;
                    }
                    return Ok(manifest);
                }
                Err(e) => {
                    warn!("manifest fetch failed from {url}: {e}");
                    last_err = e.into();
                }
            }
        }
        Err(last_err.context("all manifest sources failed"))
    }

    #[cfg(target_os = "linux")]
    fn show_manual_update_popup(window: &slint::Weak<MainWindow>, version: &str) {
        let message = format!(
            "Aurora {version} is available, but this AppImage is in a location it cannot write \
             to. Download the new version manually to update."
        );
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                w.set_popup_id("update-manual".into());
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

    #[cfg(windows)]
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

        let mut last_err = anyhow!("no download sources available");
        let mut downloaded = None;
        for url in entry.download_urls() {
            let result = reqwest::blocking::get(&url)
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(reqwest::blocking::Response::bytes);
            match result {
                Ok(bytes) => {
                    downloaded = Some(bytes);
                    break;
                }
                Err(e) => {
                    warn!("updater download failed from {url}: {e}");
                    last_err = e.into();
                }
            }
        }
        let bytes =
            downloaded.ok_or_else(|| last_err.context("all updater download sources failed"))?;

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

    #[cfg(windows)]
    fn fetch_manifest() -> Result<Manifest> {
        let mut last_err = anyhow!("no manifest sources configured");
        for url in ipc::manifest_urls() {
            let result = reqwest::blocking::get(url)
                .and_then(Response::error_for_status)
                .and_then(Response::json::<Manifest>);
            match result {
                Ok(manifest) => {
                    if let Err(e) = manifest.validate_urls() {
                        warn!("manifest from {url} rejected: {e}");
                        last_err = anyhow!(e);
                        continue;
                    }
                    return Ok(manifest);
                }
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

    pub fn ui_locked() -> bool {
        UI_LOCKED.load(Ordering::SeqCst)
    }

    fn begin_locked_update(window: &slint::Weak<MainWindow>) {
        UI_LOCKED.store(true, Ordering::SeqCst);
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            crate::classes::logwindow::hide();
            if let Some(w) = w.upgrade() {
                w.set_launch_disabled(true);
                w.set_launch_state(LaunchState::Updating);
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
                    w.set_progress_overlay_title(
                        crate::translations::tr("progress.updating-aurora-title").into(),
                    );
                    w.set_progress_overlay_progress(0.0);
                    w.set_progress_overlay_text(
                        crate::translations::tr("progress.updating-aurora-preparing").into(),
                    );
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

    #[cfg(windows)]
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
        UI_LOCKED.store(locked, Ordering::SeqCst);
        let w = window.clone();
        slint::invoke_from_event_loop(move || {
            if let Some(w) = w.upgrade() {
                w.set_launch_disabled(locked);
                w.set_launch_state(if locked {
                    LaunchState::Updating
                } else {
                    LaunchState::Launch
                });
            }
        })
        .ok();
    }

    fn send_init_confirmed() {
        for _ in 0..10 {
            if let Ok(mut stream) = protocol::connect(ipc::INIT_PIPE_NAME)
                && protocol::write_message(&mut stream, &Message::InitConfirmed).is_ok()
            {
                info!("init_confirmed sent");
                return;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        warn!("could not deliver init_confirmed to the updater");
    }

    #[cfg(feature = "beta")]
    #[cfg(not(debug_assertions))]
    #[cfg(windows)]
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
    #[cfg(not(debug_assertions))]
    #[cfg(target_os = "linux")]
    fn update_available() -> Result<bool> {
        let appimage =
            ipc::appimage_path().ok_or_else(|| anyhow!("not running from an AppImage"))?;
        let manifest = Self::fetch_linux_manifest()?;
        let current = hash_file(&appimage)
            .with_context(|| format!("failed to hash {}", appimage.display()))?;
        Ok(current != manifest.appimage.sha256)
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
