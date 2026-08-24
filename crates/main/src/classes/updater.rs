use std::io::Read;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::{
    io,
    path::Path,
    sync::{Arc, mpsc},
};

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
static POST_UPDATE_PENDING: AtomicBool = AtomicBool::new(false);

const NO_SAVED_STATE: u8 = u8::MAX;
static PRE_LOCK_STATE: AtomicU8 = AtomicU8::new(NO_SAVED_STATE);
static PRE_LOCK_DISABLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateKind {
    Minor,
    Major,
    Downgrade,
}

struct UpdateRunningGuard;

impl Drop for UpdateRunningGuard {
    fn drop(&mut self) {
        UPDATE_RUNNING.store(false, Ordering::SeqCst);
    }
}

impl LaunchState {
    pub const fn to_code(&self) -> u8 {
        match self {
            Self::Launch => 0,
            Self::Launching => 1,
            Self::Running => 2,
            Self::Updating => 3,
        }
    }

    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::Launch),
            1 => Some(Self::Launching),
            2 => Some(Self::Running),
            3 => Some(Self::Updating),
            _ => None,
        }
    }
}

pub struct UpdateHandler;

impl UpdateHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let has = |flag: &str| args.iter().any(|arg| arg == flag);

        if has(ipc::POST_UPDATE_ARG) {
            info!("launched post-update; init will be confirmed once the window is up");
            POST_UPDATE_PENDING.store(true, Ordering::SeqCst);
            return;
        }

        if has(ipc::SKIP_UPDATE_CHECK_ARG) {
            warn!("startup update check skipped");
            return;
        }

        #[cfg(feature = "beta")]
        {
            if has(SKIP_BETA_PHASING_ARG) {
                info!("skipping beta phasing");
            } else {
                let w = window.clone();
                std::thread::spawn(move || Self::run_beta_phase_gate(&w));
                return;
            }
        }

        Self::run_update_check(window, false);
    }

    pub fn on_window_shown() {
        if POST_UPDATE_PENDING.swap(false, Ordering::SeqCst) {
            info!("window is up; sending init_confirmed");
            std::thread::spawn(Self::send_init_confirmed);
        }
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
                            w.set_popup_title("Beta phase ended".into());
                            w.set_popup_message(
                                "The beta phase for this version has ended and no newer \
                                 build is available yet. You can keep using this version \
                                 for now. Please update once a new build is published."
                                    .into(),
                            );
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

        if show_toast {
            Bridge::show_toast(window, "Checking for updates...", "info");
        }

        let w = window.clone();
        std::thread::spawn(move || {
            let prompt = {
                let _running = UpdateRunningGuard;
                match Self::run_update_flow(&w, show_toast) {
                    Ok(prompt) => prompt,
                    Err(e) => {
                        warn!("update flow failed: {e}");
                        if show_toast {
                            Bridge::show_toast(&w, "Could not check for updates.", "error");
                        }
                        None
                    }
                }
            };
            if let Some(version) = prompt {
                Self::show_update_popup(&w, &version);
            }
        });
    }

    #[cfg(windows)]
    fn run_update_flow(
        window: &slint::Weak<MainWindow>,
        interactive: bool,
    ) -> Result<Option<String>> {
        let root = ipc::install_root();
        let manifest = Self::fetch_manifest()?;

        Self::self_update_updater(&root, &manifest)?;

        let local = match LocalManifest::load(&root) {
            Ok(Some(local)) => local,
            Ok(None) => LocalManifest::build_manifest_from_disk(&root, &manifest),
            Err(e) => {
                warn!("local manifest unreadable ({e}); rebuilding it from disk");
                LocalManifest::build_manifest_from_disk(&root, &manifest)
            }
        };

        if manifest.changed_files(&root, &local).is_empty() {
            info!("no update available");
            if interactive {
                Bridge::show_toast(window, "Aurora is up to date.", "success");
            }
            return Ok(None);
        }

        let local_version = shared::utils::get_local_version();
        match Self::update_kind(&local_version, &manifest.version) {
            UpdateKind::Downgrade => {
                warn!(
                    "the manifest offers {} but {} is installed; ignoring the downgrade",
                    manifest.version,
                    local_version.trim()
                );
                if interactive {
                    Bridge::show_toast(window, "Aurora is up to date.", "success");
                }
            }
            UpdateKind::Minor => {
                if Bridge::game_busy() {
                    info!(
                        "minor update {} is available but the game is running; it will be applied on the next start",
                        manifest.version
                    );
                    return Ok(None);
                }

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
            }
            UpdateKind::Major => {
                info!("update {} available; asking the user", manifest.version);

                return Ok(Some(manifest.version));
            }
        }
        Ok(None)
    }

    #[cfg(target_os = "linux")]
    fn run_update_flow(
        window: &slint::Weak<MainWindow>,
        interactive: bool,
    ) -> Result<Option<String>> {
        let Some(appimage) = ipc::appimage_path() else {
            info!("not running from an AppImage; self-update is unavailable");
            if interactive {
                Bridge::show_toast(
                    window,
                    "Self-update is only available in the AppImage build.",
                    "info",
                );
            }
            return Ok(None);
        };

        let manifest = Self::fetch_linux_manifest()?;
        let current = hash_file(&appimage)
            .with_context(|| format!("failed to hash {}", appimage.display()))?;
        if ipc::manifest::hash_eq(&current, &manifest.appimage.sha256) {
            info!("no update available");
            if interactive {
                Bridge::show_toast(window, "Aurora is up to date.", "success");
            }
            return Ok(None);
        }

        if !Self::appimage_is_replaceable(&appimage) {
            warn!(
                "an update is available but {} cannot be replaced",
                appimage.display()
            );
            Self::show_manual_update_popup(window, &manifest.version);
            return Ok(None);
        }

        let local_version = shared::utils::get_local_version();
        match Self::update_kind(&local_version, &manifest.version) {
            UpdateKind::Downgrade => {
                warn!(
                    "the manifest offers {} but {} is installed; ignoring the downgrade",
                    manifest.version,
                    local_version.trim()
                );
                if interactive {
                    Bridge::show_toast(window, "Aurora is up to date.", "success");
                }
            }
            UpdateKind::Minor => {
                if Bridge::game_busy() {
                    info!(
                        "minor update {} is available but the game is running; it will be applied on the next start",
                        manifest.version
                    );
                    return Ok(None);
                }
                info!(
                    "minor update {} -> {} available; updating silently",
                    local_version.trim(),
                    manifest.version
                );
                Self::begin_locked_update(window);

                if let Err(e) = Self::run_appimage_update(window, true, Some(manifest)) {
                    warn!("silent update failed: {e}");
                    Self::set_update_overlay(window, false);
                    Self::set_locked(window, false);
                    Bridge::show_toast(window, "Update failed. Try again later.", "error");
                }
            }
            UpdateKind::Major => {
                info!("update {} available; asking the user", manifest.version);
                return Ok(Some(manifest.version));
            }
        }
        Ok(None)
    }

    fn parse_version(version: &str) -> Option<Vec<u64>> {
        let trimmed = version.trim().trim_start_matches(['v', 'V']);
        let core = trimmed.split(['-', '+']).next()?;
        let parts = core
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect::<Option<Vec<u64>>>()?;
        (!parts.is_empty()).then_some(parts)
    }

    fn compare_versions(local: &[u64], remote: &[u64]) -> std::cmp::Ordering {
        let width = local.len().max(remote.len());
        let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
        (0..width)
            .map(|i| at(local, i).cmp(&at(remote, i)))
            .find(|ordering| ordering.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    }

    fn update_kind(local: &str, remote: &str) -> UpdateKind {
        let (Some(local_parts), Some(remote_parts)) =
            (Self::parse_version(local), Self::parse_version(remote))
        else {
            return UpdateKind::Major;
        };

        if Self::compare_versions(&local_parts, &remote_parts) == std::cmp::Ordering::Greater {
            return UpdateKind::Downgrade;
        }

        let staple_major = |parts: &[u64]| (parts.first().copied(), parts.get(1).copied());
        if staple_major(&local_parts) == staple_major(&remote_parts) {
            UpdateKind::Minor
        } else {
            UpdateKind::Major
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
            let result = Self::run_appimage_update(&w, false, None);

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
            protocol::listen(&ipc::main_pipe_name()).context("failed to open updater pipe")?;

        let updater_path = root.join(ipc::UPDATER_EXE);
        let mut child = Some(
            Command::new(&updater_path)
                .current_dir(&root)
                .spawn()
                .with_context(|| format!("failed to launch {}", updater_path.display()))?,
        );

        let stream = match protocol::accept_timeout(&listener, ipc::UPDATER_CONNECT_TIMEOUT) {
            Ok(stream) => Arc::new(stream),
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                Self::abort_update_session(window, "the updater never connected", &mut child);
                return Ok(());
            }
            Err(e) => {
                Self::abort_update_session(
                    window,
                    &format!("failed to accept the updater connection: {e}"),
                    &mut child,
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
                    if !silent {
                        Bridge::show_toast(window, "Aurora is already up to date.", "info");
                    }
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
                    Self::kill_updater(&mut child);
                    Self::set_update_overlay(window, false);
                    Self::set_locked(window, false);
                    Bridge::show_toast(window, "Update failed. Try again later.", "error");
                    return Ok(());
                }
                Ok(Ok(Message::OneClick { .. })) => {
                    Self::abort_update_session(
                        window,
                        "the updater sent a 1-click message",
                        &mut child,
                    );
                    return Ok(());
                }
                Ok(Err(e)) => {
                    Self::abort_update_session(
                        window,
                        &format!("the updater connection failed: {e}"),
                        &mut child,
                    );
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    Self::abort_update_session(
                        window,
                        "the updater stopped sending heartbeats",
                        &mut child,
                    );
                    return Ok(());
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    Self::abort_update_session(
                        window,
                        "the updater closed the connection",
                        &mut child,
                    );
                    return Ok(());
                }
            }
        }
    }

    #[cfg(windows)]
    fn abort_update_session(
        window: &slint::Weak<MainWindow>,
        reason: &str,
        child: &mut Option<std::process::Child>,
    ) {
        error!("update session ended abnormally: {reason}; unlocking UI");
        Self::kill_updater(child);
        UPDATE_RUNNING.store(false, Ordering::SeqCst);
        Self::set_update_overlay(window, false);
        Self::set_locked(window, false);
        Bridge::show_toast(
            window,
            "Update interrupted. You can retry from the launch menu.",
            "error",
        );
    }

    #[cfg(windows)]
    fn kill_updater(child: &mut Option<std::process::Child>) {
        let Some(mut child) = child.take() else {
            return;
        };

        match child.try_wait() {
            Ok(Some(status)) => {
                info!("the updater had already exited ({status})");
                return;
            }
            Ok(None) => {}
            Err(e) => warn!("could not query the updater process: {e}"),
        }

        info!("terminating the updater process");
        if let Err(e) = child.kill() {
            warn!("could not terminate the updater: {e}");
        }

        if let Err(e) = child.wait() {
            warn!("could not reap the updater: {e}");
        }
    }

    #[cfg(target_os = "linux")]
    fn run_appimage_update(
        window: &slint::Weak<MainWindow>,
        silent: bool,
        manifest: Option<ipc::manifest::LinuxManifest>,
    ) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let appimage =
            ipc::appimage_path().ok_or_else(|| anyhow!("not running from an AppImage"))?;

        let manifest = match manifest {
            Some(manifest) => manifest,
            None => Self::fetch_linux_manifest()?,
        };

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
        if !ipc::manifest::hash_eq(&actual, &manifest.appimage.sha256) {
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

        let backup = appimage.with_file_name(format!("{}.bak", ipc::APPIMAGE_NAME));
        let _ = std::fs::remove_file(&backup);
        let backed_up = match std::fs::rename(&appimage, &backup) {
            Ok(()) => true,
            Err(e) => {
                warn!(
                    "could not set {} aside before the swap: {e}",
                    appimage.display()
                );
                false
            }
        };

        if let Err(e) = std::fs::rename(&tmp, &appimage) {
            let _ = std::fs::remove_file(&tmp);
            if backed_up {
                let _ = std::fs::rename(&backup, &appimage);
            }
            return Err(anyhow!("failed to replace {}: {e}", appimage.display()));
        }
        info!(
            "replaced {} with version {}",
            appimage.display(),
            manifest.version
        );

        if Bridge::game_busy() {
            info!("update applied while the game is running; deferring the restart");
            let _ = std::fs::remove_file(&backup);
            Self::restart_failed(window);
            return Ok(());
        }

        if silent {
            info!("silent update applied; restarting Aurora");
        }
        Self::relaunch_appimage(window, &appimage, backed_up.then_some(backup.as_path()));
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
        use std::io::Write;

        let client = http_client()?;
        let mut response = client
            .get(url)
            .send()
            .and_then(Response::error_for_status)
            .with_context(|| format!("failed to download {url}"))?;
        let total = response.content_length().unwrap_or(0);
        let deadline = Instant::now() + ipc::HTTP_DOWNLOAD_TIMEOUT;

        let mut file = std::fs::File::create(dst)
            .with_context(|| format!("failed to create {}", dst.display()))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut done: u64 = 0;
        let mut last_progress = std::time::Instant::now();

        loop {
            if Instant::now() >= deadline {
                return Err(anyhow!(
                    "downloading {url} exceeded {}s and was abandoned",
                    ipc::HTTP_DOWNLOAD_TIMEOUT.as_secs()
                ));
            }
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
    fn relaunch_appimage(
        window: &slint::Weak<MainWindow>,
        appimage: &std::path::Path,
        backup: Option<&std::path::Path>,
    ) {
        if let Err(e) = Command::new(appimage).arg(ipc::RELAUNCH_ARG).spawn() {
            error!("failed to relaunch the updated AppImage: {e}");
            if let Some(backup) = backup {
                match std::fs::rename(backup, appimage) {
                    Ok(()) => warn!("restored the previous AppImage after a failed relaunch"),
                    Err(e) => error!("could not restore the previous AppImage: {e}"),
                }
            }
            Self::restart_failed(window);
            return;
        }

        if let Some(backup) = backup {
            let _ = std::fs::remove_file(backup);
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
            match fetch_json::<ipc::manifest::LinuxManifest>(url) {
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
                    last_err = e;
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
        if ipc::manifest::hash_eq(&local_hash, &manifest.updater_hash) {
            return Ok(());
        }

        info!("updater is outdated; downloading new version");
        let Some(entry) = manifest.files.iter().find(|f| f.path == ipc::UPDATER_EXE) else {
            warn!(
                "the manifest has no entry for {}; keeping the installed updater",
                ipc::UPDATER_EXE
            );
            return Ok(());
        };

        let client = http_client()?;
        let mut last_err = anyhow!("no download sources available");
        let mut downloaded = None;
        for url in entry.download_urls() {
            let result = client
                .get(&url)
                .send()
                .and_then(Response::error_for_status)
                .map_err(anyhow::Error::from)
                .and_then(|mut response| {
                    read_response(&mut response, &url, ipc::HTTP_DOWNLOAD_TIMEOUT)
                });
            match result {
                Ok(bytes) => {
                    downloaded = Some(bytes);
                    break;
                }
                Err(e) => {
                    warn!("updater download failed from {url}: {e}");
                    last_err = e;
                }
            }
        }
        let bytes =
            downloaded.ok_or_else(|| last_err.context("all updater download sources failed"))?;

        let actual = ipc::manifest::hash_bytes(&bytes);
        if !ipc::manifest::hash_eq(&actual, &manifest.updater_hash) {
            return Err(anyhow!(
                "updater hash mismatch: expected {}, got {actual}",
                manifest.updater_hash
            ));
        }

        let tmp = root.join(format!("{}.{}.tmp", ipc::UPDATER_EXE, std::process::id()));
        let write_and_verify = || -> Result<()> {
            std::fs::write(&tmp, &bytes).context("failed to write updater .tmp")?;
            let written = hash_file(&tmp).context("failed to hash the downloaded updater")?;
            if !ipc::manifest::hash_eq(&written, &manifest.updater_hash) {
                return Err(anyhow!("the updater was corrupted on the way to disk"));
            }
            Ok(())
        };
        if let Err(e) = write_and_verify() {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }

        if let Err(e) = Self::replace_with_retry(&tmp, &updater_path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(anyhow!("failed to swap in new updater: {e}"));
        }
        info!("updater self-update complete.");
        Ok(())
    }

    #[cfg(windows)]
    fn replace_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
        const ATTEMPTS: u32 = 8;
        const DELAY: Duration = Duration::from_millis(250);

        let mut last = None;
        for attempt in 0..ATTEMPTS {
            match std::fs::rename(from, to) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last = Some(e);
                    if attempt + 1 < ATTEMPTS {
                        std::thread::sleep(DELAY);
                    }
                }
            }
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("rename failed")))
    }

    #[cfg(windows)]
    fn fetch_manifest() -> Result<Manifest> {
        let mut last_err = anyhow!("no manifest sources configured");
        for url in ipc::manifest_urls() {
            match fetch_json::<Manifest>(url) {
                Ok(manifest) => {
                    let checked = manifest.validate_urls().and_then(|()| manifest.validate());
                    if let Err(e) = checked {
                        warn!("manifest from {url} rejected: {e}");
                        last_err = anyhow!(e);
                        continue;
                    }
                    return Ok(manifest);
                }
                Err(e) => {
                    warn!("manifest fetch failed from {url}: {e}");
                    last_err = e;
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
        Self::set_locked(window, true);
        slint::invoke_from_event_loop(crate::classes::logwindow::hide).ok();
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
            crate::translations::tr("progress.updating-aurora-finishing")
        } else {
            crate::translations::tr("progress.updating-aurora-downloading")
                .replace("{0}", &file_index.saturating_add(1).to_string())
                .replace("{1}", &file_count.to_string())
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
        if Bridge::game_busy() {
            info!("update applied while the game is running; deferring the restart");
            Self::restart_failed(window);
            return;
        }

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
            let Some(w) = w.upgrade() else { return };
            if locked {
                PRE_LOCK_STATE.store(w.get_launch_state().to_code(), Ordering::SeqCst);
                PRE_LOCK_DISABLED.store(w.get_launch_disabled(), Ordering::SeqCst);
                w.set_launch_disabled(true);
                w.set_launch_state(LaunchState::Updating);
                return;
            }

            let saved = PRE_LOCK_STATE.swap(NO_SAVED_STATE, Ordering::SeqCst);
            match LaunchState::from_code(saved) {
                Some(LaunchState::Updating) | None => {
                    w.set_launch_state(LaunchState::Launch);
                    w.set_launch_disabled(false);
                }
                Some(state) => {
                    w.set_launch_state(state);
                    w.set_launch_disabled(PRE_LOCK_DISABLED.load(Ordering::SeqCst));
                }
            }
        })
        .ok();
    }

    fn send_init_confirmed() {
        for _ in 0..10 {
            if let Ok(mut stream) = protocol::connect(&ipc::init_pipe_name())
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
        let root =
            ipc::install_root_result().context("could not determine the install directory")?;
        let manifest = Self::fetch_manifest()?;
        let local = match LocalManifest::load(&root) {
            Ok(Some(local)) => local,
            Ok(None) => LocalManifest::build_manifest_from_disk(&root, &manifest),
            Err(e) => {
                warn!("local manifest unreadable ({e}); rebuilding it from disk");
                LocalManifest::build_manifest_from_disk(&root, &manifest)
            }
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
        Ok(!ipc::manifest::hash_eq(&current, &manifest.appimage.sha256))
    }

    #[cfg(feature = "beta")]
    fn check_beta_phasing() -> Result<bool> {
        let res: BetaPhaseResponse = fetch_json(BETA_PHASE_CHECK_URL)
            .context("could not read the beta phasing endpoint")?;

        Ok(res.active && res.phase == CURRENT_BETA_PHASE)
    }
}

fn http_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(ipc::user_agent(&shared::utils::get_local_version()))
        .connect_timeout(ipc::HTTP_CONNECT_TIMEOUT)
        .timeout(ipc::HTTP_STALL_TIMEOUT)
        .build()
        .context("failed to build the HTTP client")
}

fn read_response(response: &mut Response, url: &str, budget: Duration) -> Result<Vec<u8>> {
    let started = Instant::now();
    let mut body = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        if started.elapsed() > budget {
            return Err(anyhow!(
                "{url} did not finish sending within {}s",
                budget.as_secs()
            ));
        }
        let n = response
            .read(&mut chunk)
            .with_context(|| format!("failed to read the response from {url}"))?;
        if n == 0 {
            return Ok(body);
        }

        body.extend_from_slice(&chunk[..n]);
    }
}

fn fetch_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let client = http_client()?;
    let mut response = client
        .get(url)
        .send()
        .and_then(Response::error_for_status)
        .with_context(|| format!("request to {url} failed"))?;
    let body = read_response(&mut response, url, ipc::HTTP_MANIFEST_TIMEOUT)?;
    serde_json::from_slice(&body).with_context(|| format!("invalid JSON from {url}"))
}
