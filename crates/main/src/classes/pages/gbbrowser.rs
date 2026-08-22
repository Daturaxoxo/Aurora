use crate::classes::characters;
use crate::classes::pages::modmanager::{ModManagerHandler, ModSource};
use crate::classes::pages::sanitize_download_filename;
use crate::{GbCharacter, GbFileItem, GbModItem, MainWindow};

use anyhow::{Result, anyhow};
use log::*;
use once_cell::sync::Lazy;
use shared::classes::gamebanana::api::GameBananaApi;
use shared::classes::gamebanana::types::{NteMod, NteModFile};
use shared::config::{self, key};
use shared::utils::{format_bytes, get_gamebanana_download_dir, get_local_version};
use slint::{Model, ModelRc, VecModel};

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

const PAGE_SIZE: usize = 15;
const DOWNLOAD_BUFFER: usize = 1 << 20;
const MIN_SEARCH_LEN: usize = 3;
const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TOTAL_TIMEOUT: Duration = Duration::from_hours(1);
const DOWNLOAD_ATTEMPTS: u32 = 4;
const DOWNLOAD_RETRY_BACKOFF: Duration = Duration::from_secs(2);
const INSTALL_RUNNING: u8 = 0;
const INSTALL_CANCELLED: u8 = 1;
const INSTALL_COMMITTED: u8 = 2;

/// Must stay in the same order as the character list in gbbrowser.slint.
pub const CHARACTERS: &[(&str, u32)] = &[
    ("Adler", 43034),
    ("Aurelia", 46387),
    ("Baicang", 43035),
    ("Chaos", 46559),
    ("Chiz", 45472),
    ("Daffodill", 45474),
    ("Edgar", 45475),
    ("Fadia", 43036),
    ("Haniel", 45473),
    ("Hathor", 43037),
    ("Hotori", 43038),
    ("Iroi", 46560),
    ("Jiuyuan", 45476),
    ("Lacrimosa", 43039),
    ("Linko", 48013),
    ("Mint", 43040),
    ("Nanally", 43041),
    ("Sakiri", 43042),
    ("Shinku", 46561),
    ("Skia", 43043),
    ("Zankou", 48011),
    ("Zero (F)", 43032),
    ("Zero (M)", 43033),
];

static RUNTIME: Lazy<tokio::runtime::Runtime> =
    Lazy::new(|| tokio::runtime::Runtime::new().expect("could not create tokio runtime"));
static API: Lazy<GameBananaApi> = Lazy::new(GameBananaApi::new);

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(format!("AuroraLauncher/{}", get_local_version()))
        .read_timeout(DOWNLOAD_IDLE_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .build()
        .unwrap_or_else(|e| {
            error!("GameBanana Browser Backend: could not build the HTTP client, falling back to default: {e}");
            reqwest::Client::default()
        })
});

#[derive(Debug, Clone)]
struct Thumbnail {
    pixels: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Feed,
    Search(String),
    Category(u32),
    Partial(String),
}

#[derive(Debug, Clone, Default)]
struct GbMod {
    id: u32,
    author: String,
    name: String,
    thumb: Option<Thumbnail>,
}

struct PreviewState {
    mod_id: u32,
    urls: Vec<String>,
    index: usize,
    cached_images: HashMap<usize, Thumbnail>,
}

struct GbState {
    mode: Mode,
    page: u32,
    generation: u64,
    loading: bool,
    end_reached: bool,
    mods: Vec<(NteMod, Option<Thumbnail>)>,
    seen: HashSet<u32>,
    files: Vec<NteModFile>,
    files_mod: GbMod,
    preview: Option<PreviewState>,
}

impl Default for GbState {
    fn default() -> Self {
        Self {
            mode: Mode::Feed,
            page: 1,
            generation: 0,
            loading: false,
            end_reached: false,
            mods: Vec::new(),
            seen: HashSet::new(),
            files: Vec::new(),
            files_mod: GbMod::default(),
            preview: None,
        }
    }
}

static STATE: Lazy<Mutex<GbState>> = Lazy::new(|| Mutex::new(GbState::default()));
static INSTALL_STATE: AtomicU8 = AtomicU8::new(INSTALL_RUNNING);
static INSTALL_CANCEL_SIGNAL: Lazy<tokio::sync::Notify> = Lazy::new(tokio::sync::Notify::new);

pub fn runtime() -> &'static tokio::runtime::Runtime {
    &RUNTIME
}

pub async fn mod_files(mod_id: u32) -> Option<Vec<NteModFile>> {
    API.get_mod_files(mod_id).await
}

fn show_nsfw() -> bool {
    config::get(key::GB_NSFW).as_bool().unwrap_or(false)
}

fn decode_thumb(bytes: &[u8]) -> Option<Thumbnail> {
    if bytes.is_empty() {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some(Thumbnail {
        pixels: img.into_raw(),
        width: w,
        height: h,
    })
}

fn encode_png(t: &Thumbnail) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(t.width, t.height, t.pixels.clone())?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| warn!("GameBanana Browser Backend: could not encode the icon png: {e}"))
        .ok()?;
    Some(out.into_inner())
}

fn to_item(m: &NteMod, thumb: Option<&Thumbnail>, hide_downloads: bool) -> GbModItem {
    let thumbnail = thumb.map_or_else(slint::Image::default, |t| {
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            &t.pixels, t.width, t.height,
        );
        slint::Image::from_rgba8(buffer)
    });

    GbModItem {
        id: i32::try_from(m.id).unwrap_or(0),
        name: m.name.as_str().into(),
        author: m.author.as_str().into(),
        thumbnail,
        has_thumbnail: thumb.is_some(),
        likes: i32::try_from(m.like_count).unwrap_or(0),
        views: i32::try_from(m.view_count).unwrap_or(0),
        downloads: if hide_downloads {
            "".into()
        } else {
            m.download_count.to_string().into()
        },
    }
}

fn show_toast(w: &MainWindow, kind: &str, text: String) {
    w.set_toast_text(text.into());
    w.set_toast_kind(kind.into());
    w.set_toast_active(true);
}

pub struct GbBrowserHandler;

impl GbBrowserHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let w = window.unwrap();
        w.set_gb_show_nsfw(show_nsfw());
        w.set_gb_mods(Rc::new(VecModel::<GbModItem>::default()).into());
        w.set_gb_characters(Self::character_model());
        Self::bind(window);
        info!("GameBanana Browser Backend: setup() complete");
    }

    fn character_model() -> ModelRc<GbCharacter> {
        let characters: Vec<GbCharacter> = CHARACTERS
            .iter()
            .map(|(name, _)| {
                let icon = characters::icon_for(name).unwrap_or_else(|| {
                    warn!("GameBanana Browser Backend: no character icon matches '{name}'");
                    slint::Image::default()
                });
                GbCharacter {
                    name: (*name).into(),
                    icon,
                }
            })
            .collect();

        Rc::new(VecModel::from(characters)).into()
    }

    fn push_row(w: &MainWindow, item: GbModItem) {
        if let Some(model) = w
            .get_gb_mods()
            .as_any()
            .downcast_ref::<VecModel<GbModItem>>()
        {
            model.push(item);
        }
    }

    fn rebuild_model(w: &MainWindow) {
        let state = STATE.lock().unwrap();
        let nsfw = show_nsfw();
        let hide_downloads = matches!(state.mode, Mode::Category(_));
        let items: Vec<GbModItem> = state
            .mods
            .iter()
            .filter(|(m, _)| nsfw || !m.is_nsfw)
            .map(|(m, t)| to_item(m, t.as_ref(), hide_downloads))
            .collect();
        drop(state);
        w.set_gb_mods(Rc::new(VecModel::from(items)).into());
    }

    fn set_mode(window: &slint::Weak<MainWindow>, mode: Mode) {
        let partial = matches!(mode, Mode::Partial(_));
        {
            let mut state = STATE.lock().unwrap();
            if state.mode == mode {
                return;
            }
            state.mode = mode;
        }

        if partial {
            Self::clear_results(window);
        } else {
            Self::load(window, true);
        }
    }

    fn clear_results(window: &slint::Weak<MainWindow>) {
        {
            let mut state = STATE.lock().unwrap();
            state.generation += 1;
            state.page = 1;
            state.loading = false;
            state.end_reached = true;
            state.mods.clear();
            state.seen.clear();
        }

        if let Some(w) = window.upgrade() {
            w.set_gb_mods(Rc::new(VecModel::<GbModItem>::default()).into());
            w.set_gb_loading(false);
        }
    }

    fn load(window: &slint::Weak<MainWindow>, reset: bool) {
        let (generation, mode, page) = {
            let mut state = STATE.lock().unwrap();
            if state.loading {
                if !reset {
                    return;
                }
            } else if !reset && state.end_reached {
                return;
            }

            if reset {
                state.generation += 1;
                state.page = 1;
                state.end_reached = false;
                state.mods.clear();
                state.seen.clear();
            } else {
                state.page += 1;
            }
            state.loading = true;
            (state.generation, state.mode.clone(), state.page)
        };

        if let Some(w) = window.upgrade() {
            if reset {
                w.set_gb_mods(Rc::new(VecModel::<GbModItem>::default()).into());
            }
            w.set_gb_loading(true);
        }

        let ww = window.clone();
        RUNTIME.spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<NteMod>();

            let api_mode = mode.clone();
            let fetch = tokio::spawn(async move {
                match api_mode {
                    Mode::Feed => API.get_nte_mods(page, false, Some(tx)).await,
                    Mode::Search(query) => API.search_nte_mods(&query, page, false, Some(tx)).await,
                    Mode::Category(id) => API.get_category_mods(id, page, false, Some(tx)).await,
                    Mode::Partial(_) => Ok(Vec::new()),
                }
            });

            let hide_downloads = matches!(mode, Mode::Category(_));
            while let Some(m) = rx.recv().await {
                let bytes = m.thumbnail.clone();
                let thumb = tokio::task::spawn_blocking(move || decode_thumb(&bytes))
                    .await
                    .unwrap_or_else(|e| {
                        warn!("GameBanana Browser Backend: thumbnail decode panicked: {e}");
                        None
                    });
                let ww2 = ww.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = ww2.upgrade() else { return };
                    let mut state = STATE.lock().unwrap();
                    if state.generation != generation || !state.seen.insert(m.id) {
                        return;
                    }
                    let visible = show_nsfw() || !m.is_nsfw;
                    let item = to_item(&m, thumb.as_ref(), hide_downloads);
                    state.mods.push((m, thumb));
                    drop(state);
                    if visible {
                        Self::push_row(&w, item);
                    }
                });
            }

            let outcome = match fetch.await {
                Ok(Ok(loaded)) => Some(loaded),
                Ok(Err(e)) => {
                    warn!("GameBanana Browser Backend: could not load page {page}: {e}");
                    None
                }
                Err(e) => {
                    error!("GameBanana Browser Backend: loading page {page} panicked: {e}");
                    None
                }
            };
            let end_reached = outcome.as_ref().is_some_and(|mods| {
                if matches!(mode, Mode::Feed) {
                    mods.is_empty()
                } else {
                    mods.len() < PAGE_SIZE
                }
            });
            let failed = outcome.is_none();

            let ww2 = ww.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(w) = ww2.upgrade() else { return };
                let mut state = STATE.lock().unwrap();
                if state.generation != generation {
                    return;
                }
                state.loading = false;
                if end_reached {
                    state.end_reached = true;
                }
                if failed && !reset && state.page == page {
                    state.page = state.page.saturating_sub(1).max(1);
                }
                drop(state);
                w.set_gb_loading(false);
            });
        });
    }

    fn set_preview_image(w: &MainWindow, thumb: Option<&Thumbnail>) {
        let image = thumb.map_or_else(slint::Image::default, |t| {
            let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &t.pixels, t.width, t.height,
            );
            slint::Image::from_rgba8(buffer)
        });
        w.set_gb_preview_image(image);
    }

    fn fetch_preview(window: &slint::Weak<MainWindow>, mod_id: u32, index: usize) {
        let state = STATE.lock().unwrap();
        let Some(p) = state.preview.as_ref() else {
            return;
        };
        if p.mod_id != mod_id {
            return;
        }
        let Some(url) = p.urls.get(index) else {
            return;
        };
        let url = url.clone();
        drop(state);

        if let Some(w) = window.upgrade() {
            w.set_gb_preview_loading(true);
        }

        let ww = window.clone();
        RUNTIME.spawn(async move {
            let thumb = match HTTP.get(&url).send().await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => decode_thumb(&bytes),
                    Err(e) => {
                        warn!("GameBanana Browser Backend: could not read preview '{url}': {e}");
                        None
                    }
                },
                Err(e) => {
                    warn!("GameBanana Browser Backend: could not fetch preview '{url}': {e}");
                    None
                }
            };

            let _ = slint::invoke_from_event_loop(move || {
                let Some(w) = ww.upgrade() else { return };
                let mut state = STATE.lock().unwrap();
                let Some(p) = state.preview.as_mut() else {
                    return;
                };
                if p.mod_id != mod_id {
                    return;
                }
                let is_current = p.index == index;
                let apply = thumb.as_ref().filter(|_| is_current).cloned();
                if let Some(t) = thumb {
                    p.cached_images.insert(index, t);
                }
                drop(state);

                if let Some(t) = apply {
                    Self::set_preview_image(&w, Some(&t));
                }
                if is_current {
                    w.set_gb_preview_loading(false);
                }
            });
        });
    }

    fn set_progress(window: &slint::Weak<MainWindow>, progress: f32, text: String) {
        let ww = window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = ww.upgrade() {
                w.set_progress_overlay_progress(progress);
                w.set_progress_overlay_text(text.into());
            }
        });
    }

    async fn discard_partial(path: &std::path::Path) {
        if let Err(e) = tokio::fs::remove_file(path).await {
            warn!(
                "GameBanana Browser Backend: could not remove partial download '{}': {e}",
                path.display()
            );
        }
    }

    fn download_and_install(window: &slint::Weak<MainWindow>, mod_: GbMod, file: NteModFile) {
        Self::start_download(window, mod_, file, None);
    }

    pub(crate) fn download_update(
        window: &slint::Weak<MainWindow>,
        source: ModSource,
        file: NteModFile,
        folder: std::path::PathBuf,
    ) {
        let mod_ = GbMod {
            id: source.mod_id,
            author: source.author,
            name: source.name,
            thumb: None,
        };
        Self::start_download(window, mod_, file, Some(folder));
    }

    fn start_download(
        window: &slint::Weak<MainWindow>,
        mod_: GbMod,
        file: NteModFile,
        update_folder: Option<std::path::PathBuf>,
    ) {
        INSTALL_STATE.store(INSTALL_RUNNING, Ordering::SeqCst);
        let updating = update_folder.is_some();

        let icon_png = if updating {
            None
        } else {
            mod_.thumb.as_ref().and_then(encode_png)
        };

        let ww = window.clone();
        let _ = slint::invoke_from_event_loop({
            let ww = ww.clone();
            let name = file.name.clone();
            move || {
                if let Some(w) = ww.upgrade() {
                    w.set_progress_overlay_title(
                        if updating {
                            "Updating Mod"
                        } else {
                            "Installing Mod"
                        }
                        .into(),
                    );
                    w.set_progress_overlay_progress(0.0);
                    w.set_progress_overlay_text(format!("Downloading {name}...").into());
                    w.set_progress_overlay_cancellable(true);
                    w.set_progress_overlay_active(true);
                }
            }
        });

        RUNTIME.spawn(async move {
            // Ok(None) means the download was cancelled by the user
            let result: Result<Option<std::path::PathBuf>> = async {
                use tokio::io::AsyncWriteExt;

                let Some(safe_name) = sanitize_download_filename(&file.name) else {
                    return Err(anyhow!("unsafe file name '{}'", file.name));
                };

                let dir = get_gamebanana_download_dir();
                tokio::fs::create_dir_all(&dir).await?;
                let path = dir.join(&safe_name);

                let started = Instant::now();
                let mut done: u64 = 0;
                let mut last_percent: u64 = 0;
                let mut hasher = md5::Context::new();
                let mut total: u64 = file.size;
                let mut attempt: u32 = 1;

                let cancelled = loop {
                    let mut request = HTTP.get(&file.url).timeout(DOWNLOAD_TOTAL_TIMEOUT);
                    if done > 0 {
                        request =
                            request.header(reqwest::header::RANGE, format!("bytes={done}-"));
                    }

                    let transfer: Result<bool> = async {
                        let mut resp = request.send().await?.error_for_status()?;
                        let resuming = done > 0
                            && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
                        if done > 0 && !resuming {
                            warn!(
                                "GameBanana Browser Backend: '{}' could not be resumed, starting over",
                                file.name
                            );
                            done = 0;
                            last_percent = 0;
                            hasher = md5::Context::new();
                        }

                        total = resp.content_length().map_or(file.size, |len| {
                            if resuming {
                                done + len
                            } else {
                                len
                            }
                        });

                        let mut out = tokio::io::BufWriter::with_capacity(
                            DOWNLOAD_BUFFER,
                            tokio::fs::OpenOptions::new()
                                .create(true)
                                .write(true)
                                .append(resuming)
                                .truncate(!resuming)
                                .open(&path)
                                .await?,
                        );

                        let stopped = loop {
                            if INSTALL_STATE.load(Ordering::SeqCst) == INSTALL_CANCELLED {
                                break true;
                            }
                            let chunk = tokio::select! {
                                biased;
                                () = INSTALL_CANCEL_SIGNAL.notified() => break true,
                                chunk = tokio::time::timeout(DOWNLOAD_IDLE_TIMEOUT, resp.chunk()) => {
                                    chunk.map_err(|_| {
                                        anyhow!(
                                            "the download stalled for more than {}s",
                                            DOWNLOAD_IDLE_TIMEOUT.as_secs()
                                        )
                                    })??
                                }
                            };
                            let Some(chunk) = chunk else { break false };
                            hasher.consume(&chunk);
                            out.write_all(&chunk).await?;
                            done += chunk.len() as u64;
                            if let Some(percent) = (done * 100).checked_div(total)
                                && percent > last_percent {
                                    last_percent = percent;
                                    #[allow(
                                        clippy::cast_precision_loss,
                                        clippy::cast_possible_truncation
                                    )]
                                    let frac = (done as f64 / total as f64).min(1.0) as f32;
                                    Self::set_progress(
                                        &ww,
                                        frac,
                                        format!("Downloading {}...", file.name),
                                    );
                                }
                        };
                        out.flush().await?;
                        Ok(stopped)
                    }
                    .await;

                    match transfer {
                        Ok(stopped) => break stopped,
                        Err(e) => {
                            let cancelled =
                                INSTALL_STATE.load(Ordering::SeqCst) == INSTALL_CANCELLED;
                            if cancelled || attempt >= DOWNLOAD_ATTEMPTS {
                                Self::discard_partial(&path).await;
                                return if cancelled {
                                    Ok(None)
                                } else {
                                    Err(e.context(format!(
                                        "'{}' failed after {DOWNLOAD_ATTEMPTS} attempts",
                                        file.name
                                    )))
                                };
                            }

                            warn!(
                                "GameBanana Browser Backend: '{}' interrupted at {}, retrying ({attempt}/{DOWNLOAD_ATTEMPTS}): {e:#}",
                                file.name,
                                format_bytes(done)
                            );
                            Self::set_progress(
                                &ww,
                                0.0,
                                format!("Reconnecting to resume {}...", file.name),
                            );
                            tokio::time::sleep(DOWNLOAD_RETRY_BACKOFF * attempt).await;
                            attempt += 1;
                        }
                    }
                };

                if cancelled {
                    Self::discard_partial(&path).await;
                    return Ok(None);
                }

                let digest = hasher.finalize();
                let digest = format!("{digest:x}");
                if !file.md5.is_empty() && !file.md5.eq_ignore_ascii_case(&digest) {
                    Self::discard_partial(&path).await;
                    return Err(anyhow!(
                        "checksum mismatch for '{}' (expected {}, got {digest})",
                        file.name,
                        file.md5,
                    ));
                }

                let secs = started.elapsed().as_secs_f64();
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let rate = if secs > 0.0 {
                    (done as f64 / secs) as u64
                } else {
                    0
                };
                info!(
                    "GameBanana Browser Backend: fetched '{}' ({}) in {secs:.1}s - {}/s",
                    file.name,
                    format_bytes(done),
                    format_bytes(rate)
                );

                Ok(Some(path))
            }
            .await;

            match result {
                Ok(Some(path)) => {
                    if INSTALL_STATE
                        .compare_exchange(
                            INSTALL_RUNNING,
                            INSTALL_COMMITTED,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_err()
                    {
                        info!("GameBanana Browser Backend: download of '{}' cancelled", file.name);
                        Self::discard_partial(&path).await;
                        return;
                    }
                    info!("GameBanana Browser Backend: downloaded '{}'", path.display());
                    let ww2 = ww.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ww2.upgrade() {
                            // Extraction can't be aborted, so cancelling stops here
                            w.set_progress_overlay_cancellable(false);
                            w.set_progress_overlay_progress(1.0);
                            w.set_progress_overlay_text(
                                if updating { "Updating..." } else { "Installing..." }.into(),
                            );
                        }
                    });

                    let source = ModSource {
                        mod_id: mod_.id,
                        file_id: file.id,
                        file_name: file.name.clone(),
                        md5: file.md5.clone(),
                        author: mod_.author,
                        name: mod_.name,
                    };

                    if let Some(folder) = update_folder {
                        ModManagerHandler::apply_update(&ww, folder, path, source);
                    } else {
                        ModManagerHandler::install_paths_with_done(
                            &ww,
                            vec![path],
                            Some(source),
                            Some(Box::new(|w| w.set_progress_overlay_active(false))),
                            icon_png,
                        );
                    }
                }
                Ok(None) => {
                    info!("GameBanana Browser Backend: download of '{}' cancelled", file.name);
                }
                Err(e) => {
                    warn!("GameBanana Browser Backend: could not download '{}': {e}", file.name);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ww.upgrade() {
                            w.set_progress_overlay_active(false);
                            show_toast(&w, "error", format!("Download failed - {e}"));
                        }
                    });
                }
            }
        });
    }

    // [CALLBACKS]

    fn bind(window: &slint::Weak<MainWindow>) {
        let w = window.unwrap();

        let ww = window.clone();
        w.on_gb_opened(move || {
            let Some(win) = ww.upgrade() else { return };
            win.set_gb_show_nsfw(show_nsfw());
            win.set_gb_search_text("".into());
            win.set_gb_selected_character(-1);
            win.set_gb_files_visible(false);
            win.set_gb_preview_visible(false);
            {
                let mut state = STATE.lock().unwrap();
                state.mode = Mode::Feed;
                state.preview = None;
            }
            Self::load(&ww, true);
        });

        let ww = window.clone();
        w.on_gb_search_changed(move |text| {
            let query = text.trim().to_string();
            let mode = if query.is_empty() {
                Mode::Feed
            } else if query.len() < MIN_SEARCH_LEN {
                Mode::Partial(query)
            } else {
                Mode::Search(query)
            };
            Self::set_mode(&ww, mode);
        });

        let ww = window.clone();
        w.on_gb_character_selected(move |index| {
            let mode = usize::try_from(index)
                .ok()
                .and_then(|i| CHARACTERS.get(i))
                .map_or(Mode::Feed, |(name, id)| {
                    trace!("GameBanana Browser Backend: filtering by character '{name}'");
                    Mode::Category(*id)
                });
            if let Some(win) = ww.upgrade() {
                win.set_gb_search_text("".into());
            }
            Self::set_mode(&ww, mode);
        });

        let ww = window.clone();
        w.on_gb_load_more(move || {
            Self::load(&ww, false);
        });

        let ww = window.clone();
        w.on_gb_nsfw_toggled(move |enabled| {
            let Some(win) = ww.upgrade() else { return };
            config::set(key::GB_NSFW, enabled);
            Self::rebuild_model(&win);
        });

        let ww = window.clone();
        w.on_gb_clear_cache(move || {
            let ww = ww.clone();
            RUNTIME.spawn(async move {
                let mut failure = match API.clear_cache().await {
                    Ok(()) => None,
                    Err(e) => {
                        error!("GameBanana Browser Backend: could not clear the page cache: {e}");
                        Some(e.to_string())
                    }
                };

                let dir = get_gamebanana_download_dir();
                match tokio::fs::remove_dir_all(&dir).await {
                    Ok(()) => {}
                    // Nothing has been downloaded yet, so there is nothing to clear
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => {
                        error!(
                            "GameBanana Browser Backend: could not clear the downloads in '{}': {e}",
                            dir.display()
                        );
                        failure.get_or_insert_with(|| e.to_string());
                    }
                }

                let _ = slint::invoke_from_event_loop(move || {
                    let Some(win) = ww.upgrade() else { return };
                    if let Some(e) = failure {
                        show_toast(&win, "error", format!("Could not clear cache - {e}"));
                    } else {
                        info!("GameBanana Browser Backend: cache cleared");
                        show_toast(&win, "success", "Cache cleared".into());
                    }
                    // Refetch the active feed, search or category from the API
                    Self::load(&ww, true);
                });
            });
        });

        let ww = window.clone();
        w.on_gb_install(move |id| {
            let Ok(mod_id) = u32::try_from(id) else {
                return;
            };
            let mod_ = STATE
                .lock()
                .unwrap()
                .mods
                .iter()
                .find(|(m, _)| m.id == mod_id)
                .map(|(m, t)| GbMod {
                    id: m.id,
                    author: m.author.clone(),
                    name: m.name.clone(),
                    thumb: t.clone(),
                })
                .unwrap_or_default();

            let ww2 = ww.clone();
            RUNTIME.spawn(async move {
                let files = API.get_mod_files(mod_id).await;
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(win) = ww2.upgrade() else { return };
                    let Some(files) = files else {
                        warn!(
                            "GameBanana Browser Backend: could not fetch the files of mod {mod_id}"
                        );
                        show_toast(&win, "error", "Could not fetch this mod's files".into());
                        return;
                    };
                    match files.len() {
                        0 => {
                            warn!(
                                "GameBanana Browser Backend: mod {mod_id} has no downloadable files"
                            );
                            show_toast(&win, "error", "This mod has no files to download".into());
                        }
                        1 => Self::download_and_install(&ww2, mod_, files[0].clone()),
                        _ => {
                            let items: Vec<GbFileItem> = files
                                .iter()
                                .map(|f| GbFileItem {
                                    name: f.name.as_str().into(),
                                    size: format_bytes(f.size).into(),
                                    downloads: i32::try_from(f.download_count).unwrap_or(i32::MAX),
                                })
                                .collect();
                            let mut state = STATE.lock().unwrap();
                            state.files = files;
                            state.files_mod = mod_.clone();
                            drop(state);
                            win.set_gb_files(Rc::new(VecModel::from(items)).into());
                            win.set_gb_files_mod_name(mod_.name.as_str().into());
                            win.set_gb_files_visible(true);
                        }
                    }
                });
            });
        });

        let ww = window.clone();
        w.on_gb_file_chosen(move |index| {
            let Some(win) = ww.upgrade() else { return };
            win.set_gb_files_visible(false);
            let picked = usize::try_from(index).ok().and_then(|i| {
                let state = STATE.lock().unwrap();
                state
                    .files
                    .get(i)
                    .cloned()
                    .map(|f| (state.files_mod.clone(), f))
            });
            if let Some((mod_, file)) = picked {
                Self::download_and_install(&ww, mod_, file);
            }
        });

        let ww = window.clone();
        w.on_gb_preview_requested(move |id| {
            let Some(win) = ww.upgrade() else { return };
            let Ok(mod_id) = u32::try_from(id) else {
                return;
            };

            let opened = {
                let mut state = STATE.lock().unwrap();
                let Some((m, thumb)) = state.mods.iter().find(|(m, _)| m.id == mod_id) else {
                    return;
                };
                let urls = m.preview_urls.clone();
                let name = m.name.clone();
                let thumb = thumb.clone();
                state.preview = Some(PreviewState {
                    mod_id,
                    urls: urls.clone(),
                    index: 0,
                    cached_images: HashMap::new(),
                });

                drop(state);
                (urls, name, thumb)
            };
            let (urls, name, thumb) = opened;

            win.set_gb_preview_name(name.as_str().into());
            win.set_gb_preview_index(0);
            win.set_gb_preview_count(i32::try_from(urls.len().max(1)).unwrap_or(1));
            win.set_gb_preview_loading(false);
            Self::set_preview_image(&win, thumb.as_ref());
            win.set_gb_preview_visible(true);

            if !urls.is_empty() {
                Self::fetch_preview(&ww, mod_id, 0);
            }
        });

        let ww = window.clone();
        w.on_gb_preview_nav(move |new_index| {
            let Some(win) = ww.upgrade() else { return };
            let Ok(new_index) = usize::try_from(new_index) else {
                return;
            };

            let mut state = STATE.lock().unwrap();
            let Some(p) = state.preview.as_mut() else {
                return;
            };
            if new_index >= p.urls.len() || new_index == p.index {
                return;
            }
            p.index = new_index;
            let mod_id = p.mod_id;
            let cached = p.cached_images.get(&new_index).cloned();
            drop(state);

            win.set_gb_preview_index(i32::try_from(new_index).unwrap_or(0));
            if let Some(t) = cached {
                Self::set_preview_image(&win, Some(&t));
                win.set_gb_preview_loading(false);
            } else {
                // Keep showing the current image while the next one loads
                Self::fetch_preview(&ww, mod_id, new_index);
            }
        });

        let ww = window.clone();
        w.on_progress_overlay_cancel(move || {
            if INSTALL_STATE
                .compare_exchange(
                    INSTALL_RUNNING,
                    INSTALL_CANCELLED,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_err()
            {
                return;
            }
            INSTALL_CANCEL_SIGNAL.notify_waiters();
            if let Some(win) = ww.upgrade() {
                win.set_progress_overlay_active(false);
                show_toast(&win, "info", "Install cancelled".into());
            }
        });

        w.on_gb_open_page(move |id| {
            let Ok(mod_id) = u32::try_from(id) else {
                return;
            };
            let url = STATE
                .lock()
                .unwrap()
                .mods
                .iter()
                .find(|(m, _)| m.id == mod_id)
                .map(|(m, _)| m.mod_url.clone());
            let Some(url) = url else { return };
            if let Err(e) = open::that(&url) {
                warn!("GameBanana Browser Backend: could not open '{url}': {e}");
            }
        });
    }
}
