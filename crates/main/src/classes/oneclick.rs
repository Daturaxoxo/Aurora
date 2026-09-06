use std::io;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use ipc::protocol::{self, Message};
use log::{error, info, warn};
use once_cell::sync::Lazy;
use shared::classes::gamebanana::api::NTE_GAME_ID;
use shared::classes::gamebanana::types::NteModFile;
use shared::config::{self, key};
use shared::oneclick::OneClick;
use shared::utils::format_bytes;
use slint::ComponentHandle as _;
use url::Url;

use crate::MainWindow;
use crate::bridge::{Bridge, PopupSpec};
use crate::classes::pages::gbbrowser::{self, GbBrowserHandler};
use crate::classes::pages::modmanager::ModManagerHandler;
use crate::translations::tr;

pub const POPUP_ID: &str = "gb-oneclick";

const CONNECT_ATTEMPTS: u32 = 4;
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(500);
const ACCEPT_TIMEOUT: Duration = Duration::from_millis(250);
const READ_TIMEOUT: Duration = Duration::from_secs(2);

static REQUEST_ACTIVE: AtomicBool = AtomicBool::new(false);
static LISTENER_RUNNING: AtomicBool = AtomicBool::new(false);
static PENDING: Lazy<Mutex<Option<PendingInstall>>> = Lazy::new(|| Mutex::new(None));

struct PendingInstall {
    mod_id: u32,
    author: String,
    name: String,
    thumbnail: Vec<u8>,
    file: NteModFile,
}

pub struct OneClickHandler;

impl OneClickHandler {
    pub fn request_from_args() -> Option<OneClick> {
        let prefix = format!("{}:", shared::oneclick::SCHEME);
        std::env::args().find_map(|arg| {
            if !arg.starts_with(&prefix) {
                return None;
            }
            shared::oneclick::parse(&arg).map_or_else(
                || {
                    warn!("1-Click: rejected malformed URI argument: {arg}");
                    None
                },
                Some,
            )
        })
    }

    pub fn forward(request: &OneClick) -> Result<()> {
        let message = Message::OneClick {
            url: request.url.clone(),
            model: request.model.clone(),
            item_id: request.item_id,
        };

        let mut last_error = None;
        for attempt in 1..=CONNECT_ATTEMPTS {
            match protocol::send_and_confirm(
                &ipc::oneclick_pipe_name(),
                &message,
                ipc::ONECLICK_ACK_TIMEOUT,
            ) {
                Ok(()) => {
                    info!("1-Click: forwarded request to the running instance");
                    return Ok(());
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt < CONNECT_ATTEMPTS {
                        std::thread::sleep(CONNECT_RETRY_DELAY);
                    }
                }
            }
        }

        Err(last_error.map_or_else(
            || anyhow!("could not connect to the running instance"),
            |e| anyhow!("could not connect to the running instance: {e}"),
        ))
    }

    pub fn setup(window: &slint::Weak<MainWindow>) {
        LISTENER_RUNNING.store(true, Ordering::SeqCst);
        let window = window.clone();
        std::thread::spawn(move || Self::listen(&window));
    }

    pub fn shutdown() {
        LISTENER_RUNNING.store(false, Ordering::SeqCst);
    }

    fn listen(window: &slint::Weak<MainWindow>) {
        let listener = match protocol::listen_cross_elevation(&ipc::oneclick_pipe_name()) {
            Ok(listener) => listener,
            Err(e) => {
                error!("1-Click: could not open the listener: {e}");
                return;
            }
        };
        info!("1-Click: listening for browser requests");

        loop {
            match protocol::accept_timeout(&listener, ACCEPT_TIMEOUT) {
                Ok(stream) => {
                    if let Err(e) = protocol::set_read_timeout(&stream, Some(READ_TIMEOUT)) {
                        warn!("1-Click: could not set the request timeout: {e}");
                    }
                    let mut stream = stream;
                    match protocol::read_message(&mut stream) {
                        Ok(Message::OneClick {
                            url,
                            model,
                            item_id,
                        }) => {
                            let request = OneClick {
                                url,
                                model,
                                item_id,
                            };
                            let window = window.clone();
                            if slint::invoke_from_event_loop(move || {
                                Self::handle(&window, &request);
                            })
                            .is_err()
                            {
                                break;
                            }
                        }
                        Ok(other) => warn!("1-Click: ignored unexpected IPC message {other:?}"),
                        Err(e) => warn!("1-Click: rejected unreadable IPC request: {e}"),
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::TimedOut => {
                    if !LISTENER_RUNNING.load(Ordering::SeqCst) {
                        break;
                    }
                }
                Err(e) => {
                    error!("1-Click: listener failed: {e}");
                    break;
                }
            }
        }
        info!("1-Click: listener stopped");
    }

    pub fn handle(window: &slint::Weak<MainWindow>, request: &OneClick) {
        Self::raise_window(window);

        let Some(w) = window.upgrade() else { return };
        if GbBrowserHandler::install_in_progress(window) {
            Bridge::show_toast(window, "Another install is in progress", "warning");
            return;
        }
        if w.get_popup_active() {
            Bridge::show_toast(window, "Another action requires confirmation", "warning");
            return;
        }
        if REQUEST_ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            Bridge::show_toast(window, "Another 1-click request is in progress", "warning");
            return;
        }

        let file_id = match Self::validate_local(request) {
            Ok(file_id) => file_id,
            Err(e) => {
                Self::reject(window, &e);
                return;
            }
        };

        if let Err(e) = Self::validate_game_path() {
            Self::reject(window, &e);
            return;
        }

        let item_id = request.item_id;
        let window = window.clone();
        gbbrowser::runtime().spawn(async move {
            let installed = tokio::task::spawn_blocking(move || {
                ModManagerHandler::is_source_installed(item_id, file_id)
            })
            .await
            .unwrap_or(false);
            if installed {
                Self::reject(&window, &anyhow!("This mod file is already installed"));
                return;
            }

            let profile = match gbbrowser::mod_profile(item_id).await {
                Ok(profile) => profile,
                Err(e) => {
                    Self::reject(
                        &window,
                        &anyhow!("Could not verify this GameBanana mod: {e:#}"),
                    );
                    return;
                }
            };

            let result = (|| -> Result<PendingInstall> {
                if profile.id != item_id {
                    return Err(anyhow!("GameBanana returned a different mod"));
                }
                if profile.game_id != NTE_GAME_ID {
                    return Err(anyhow!("This mod is not for Neverness to Everness"));
                }
                let file = profile
                    .files
                    .into_iter()
                    .find(|file| file.id == file_id)
                    .ok_or_else(|| anyhow!("The requested file is not part of this mod"))?;

                Ok(PendingInstall {
                    mod_id: profile.id,
                    author: profile.author,
                    name: profile.name,
                    thumbnail: profile.thumbnail,
                    file,
                })
            })();

            match result {
                Ok(pending) => Self::show_confirmation(&window, pending, profile.is_nsfw),
                Err(e) => Self::reject(&window, &e),
            }
        });
    }

    pub fn confirm(window: &slint::Weak<MainWindow>) {
        let pending = PENDING.lock().unwrap().take();
        REQUEST_ACTIVE.store(false, Ordering::SeqCst);
        let Some(pending) = pending else { return };

        if !GbBrowserHandler::download_oneclick(
            window,
            pending.mod_id,
            pending.author,
            pending.name,
            &pending.thumbnail,
            pending.file,
        ) {
            Bridge::show_toast(window, "Another install is in progress", "warning");
        }
    }

    pub fn cancel() {
        PENDING.lock().unwrap().take();
        REQUEST_ACTIVE.store(false, Ordering::SeqCst);
        info!("1-Click: user cancelled the request");
    }

    fn validate_local(request: &OneClick) -> Result<u32> {
        if request.model != "Mod" {
            return Err(anyhow!("Only GameBanana mods can be installed"));
        }

        let url = Url::parse(&request.url).context("The 1-click URL is invalid")?;
        if url.scheme() != "https" {
            return Err(anyhow!("The 1-click URL is not secure"));
        }
        if url.host_str() != Some("gamebanana.com") {
            return Err(anyhow!("The 1-click URL is not from GameBanana"));
        }

        let segments: Vec<_> = url
            .path_segments()
            .ok_or_else(|| anyhow!("The 1-click URL has no file ID"))?
            .collect();
        if segments.len() != 2 || !matches!(segments[0], "mmdl" | "dl") {
            return Err(anyhow!("The 1-click URL has an unsupported path"));
        }
        segments[1]
            .parse()
            .context("The 1-click URL has an invalid file ID")
    }

    fn validate_game_path() -> Result<()> {
        let game_path = config::get(key::GAME_PATH);
        let configured = game_path
            .as_str()
            .filter(|path| !path.trim().is_empty())
            .map(std::path::Path::new)
            .ok_or_else(|| anyhow!("Set the game directory before installing mods"))?;
        shared::classes::info::version::detect_version(configured)
            .map(|_| ())
            .context("The configured game directory is invalid")
    }

    fn show_confirmation(window: &slint::Weak<MainWindow>, pending: PendingInstall, is_nsfw: bool) {
        let window = window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(w) = window.upgrade() else {
                REQUEST_ACTIVE.store(false, Ordering::SeqCst);
                return;
            };
            if GbBrowserHandler::install_in_progress(&window) {
                REQUEST_ACTIVE.store(false, Ordering::SeqCst);
                Bridge::show_toast(&window, "Another install is in progress", "warning");
                return;
            }
            if w.get_popup_active() {
                REQUEST_ACTIVE.store(false, Ordering::SeqCst);
                Bridge::show_toast(&window, "Another action requires confirmation", "warning");
                return;
            }

            let notice = if is_nsfw && !config::get(key::GB_NSFW).as_bool().unwrap_or(false) {
                tr("popup.oneclick.nsfw-notice")
            } else {
                String::new()
            };

            let mut details = vec![
                (tr("popup.detail.file"), pending.file.name.clone()),
                (tr("popup.detail.size"), format_bytes(pending.file.size)),
            ];
            if pending.file.download_count > 0 {
                details.push((
                    tr("popup.detail.downloads"),
                    pending.file.download_count.to_string(),
                ));
            }

            let spec = PopupSpec {
                id: POPUP_ID.to_owned(),
                kind: "install".to_owned(),
                title: tr("popup.oneclick.title"),
                subject: pending.name.clone(),
                subject_note: format!("{}{}", tr("popup.oneclick.author-prefix"), pending.author),
                details,
                notice,
                confirm_label: tr("global.button.install"),
                ..PopupSpec::default()
            };

            *PENDING.lock().unwrap() = Some(pending);
            spec.apply(&w);
        });
    }

    fn reject(window: &slint::Weak<MainWindow>, error: &anyhow::Error) {
        REQUEST_ACTIVE.store(false, Ordering::SeqCst);
        warn!("1-Click: rejected request: {error:#}");
        Bridge::show_toast(window, &error.to_string(), "error");
    }

    fn raise_window(window: &slint::Weak<MainWindow>) {
        let Some(w) = window.upgrade() else { return };
        let _ = w.show();
        w.window().set_minimized(false);

        #[cfg(target_os = "windows")]
        {
            use i_slint_backend_winit::WinitWindowAccessor;
            w.window()
                .with_winit_window(i_slint_backend_winit::winit::window::Window::focus_window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_supported_file_paths() {
        for path in ["mmdl/123", "dl/456"] {
            let request = OneClick {
                url: format!("https://gamebanana.com/{path}"),
                model: "Mod".into(),
                item_id: 1,
            };
            assert!(OneClickHandler::validate_local(&request).is_ok());
        }
    }

    #[test]
    fn rejects_untrusted_urls_before_networking() {
        for url in [
            "http://gamebanana.com/mmdl/1",
            "https://gamebanana.com.evil.test/mmdl/1",
            "https://gamebanana.com/mods/1",
            "file:///tmp/mod.zip",
        ] {
            let request = OneClick {
                url: url.into(),
                model: "Mod".into(),
                item_id: 1,
            };
            assert!(OneClickHandler::validate_local(&request).is_err(), "{url}");
        }
    }
}
