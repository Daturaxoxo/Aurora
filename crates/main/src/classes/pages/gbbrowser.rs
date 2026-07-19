use crate::classes::pages::modmanager::ModManagerHandler;
use crate::{GbFileItem, GbModItem, MainWindow};

use log::*;
use once_cell::sync::Lazy;
use shared::classes::gamebanana::api::GameBananaApi;
use shared::classes::gamebanana::types::{NteMod, NteModFile};
use shared::config;
use shared::utils::format_size;
use slint::{Model, VecModel};

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Mutex;

const NSFW_KEY: &str = "gb_show_nsfw";
const PAGE_SIZE: usize = 15;

/// Must stay in the same order as the character list in gbbrowser.slint.
const CHARACTERS: &[(&str, u32)] = &[
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
    ("Mint", 43040),
    ("Nanally", 43041),
    ("Sakiri", 43042),
    ("Shinku", 46561),
    ("Skia", 43043),
    ("Zero (F)", 43032),
    ("Zero (M)", 43033),
];

static RUNTIME: Lazy<tokio::runtime::Runtime> =
    Lazy::new(|| tokio::runtime::Runtime::new().expect("could not create tokio runtime"));
static API: Lazy<GameBananaApi> = Lazy::new(GameBananaApi::new);

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
            preview: None,
        }
    }
}

static STATE: Lazy<Mutex<GbState>> = Lazy::new(|| Mutex::new(GbState::default()));

fn show_nsfw() -> bool {
    config::get(NSFW_KEY).as_bool().unwrap_or(false)
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
        Self::bind(window);
        info!("[GbBrowser] setup() complete");
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
        // When filtering by a category the API doesn't return download count
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

    fn load(window: &slint::Weak<MainWindow>, reset: bool) {
        let (generation, mode, page) = {
            let mut state = STATE.lock().unwrap();
            if state.loading {
                if !reset {
                    return;
                }
                // A reset supersedes whatever was loading
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
                }
            });

            let hide_downloads = matches!(mode, Mode::Category(_));
            while let Some(m) = rx.recv().await {
                let thumb = decode_thumb(&m.thumbnail);
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

            let result = fetch.await.ok().flatten();
            let end_reached = result.as_ref().is_none_or(|mods| {
                if matches!(mode, Mode::Feed) {
                    mods.is_empty()
                } else {
                    mods.len() < PAGE_SIZE
                }
            });
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
            let thumb = match reqwest::get(&url).await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => decode_thumb(&bytes),
                    Err(e) => {
                        warn!("[GbBrowser] could not read preview '{url}': {e}");
                        None
                    }
                },
                Err(e) => {
                    warn!("[GbBrowser] could not fetch preview '{url}': {e}");
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

                // On failure the enlarged thumbnail simply stays visible
                if let Some(t) = apply {
                    Self::set_preview_image(&w, Some(&t));
                }
                if is_current {
                    w.set_gb_preview_loading(false);
                }
            });
        });
    }

    fn download_and_install(window: &slint::Weak<MainWindow>, file: NteModFile) {
        let ww = window.clone();
        let _ = slint::invoke_from_event_loop({
            let ww = ww.clone();
            let name = file.name.clone();
            move || {
                if let Some(w) = ww.upgrade() {
                    show_toast(&w, "success", format!("Downloading {name}..."));
                }
            }
        });

        RUNTIME.spawn(async move {
            let result: anyhow::Result<std::path::PathBuf> = async {
                let resp = reqwest::get(&file.url).await?.error_for_status()?;
                let bytes = resp.bytes().await?;
                let dir = std::env::temp_dir().join("Aurora/GameBanana");
                tokio::fs::create_dir_all(&dir).await?;
                let path = dir.join(&file.name);
                tokio::fs::write(&path, &bytes).await?;
                Ok(path)
            }
            .await;

            match result {
                Ok(path) => {
                    info!("[GbBrowser] downloaded '{}'", path.display());
                    ModManagerHandler::install_paths(&ww, vec![path]);
                }
                Err(e) => {
                    error!("[GbBrowser] could not download '{}': {e}", file.name);
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = ww.upgrade() {
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
            let mut state = STATE.lock().unwrap();
            if query.is_empty() {
                if state.mode == Mode::Feed {
                    return;
                }
                state.mode = Mode::Feed;
            } else {
                if query.len() < 3 || state.mode == Mode::Search(query.clone()) {
                    return;
                }
                state.mode = Mode::Search(query);
            }
            drop(state);
            Self::load(&ww, true);
        });

        let ww = window.clone();
        w.on_gb_character_selected(move |index| {
            let mode = usize::try_from(index)
                .ok()
                .and_then(|i| CHARACTERS.get(i))
                .map_or(Mode::Feed, |(name, id)| {
                    trace!("[GbBrowser] filtering by character '{name}'");
                    Mode::Category(*id)
                });
            let mut state = STATE.lock().unwrap();
            if state.mode == mode {
                return;
            }
            state.mode = mode;
            drop(state);
            Self::load(&ww, true);
        });

        let ww = window.clone();
        w.on_gb_load_more(move || {
            Self::load(&ww, false);
        });

        let ww = window.clone();
        w.on_gb_nsfw_toggled(move |enabled| {
            let Some(win) = ww.upgrade() else { return };
            config::set(NSFW_KEY, enabled);
            Self::rebuild_model(&win);
        });

        let ww = window.clone();
        w.on_gb_install(move |id| {
            let Ok(mod_id) = u32::try_from(id) else {
                return;
            };
            let mod_name = STATE
                .lock()
                .unwrap()
                .mods
                .iter()
                .find(|(m, _)| m.id == mod_id)
                .map(|(m, _)| m.name.clone())
                .unwrap_or_default();

            let ww2 = ww.clone();
            RUNTIME.spawn(async move {
                let files = API.get_mod_files(mod_id).await.unwrap_or_default();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(win) = ww2.upgrade() else { return };
                    match files.len() {
                        0 => {
                            warn!("[GbBrowser] mod {mod_id} has no downloadable files");
                            show_toast(&win, "error", "This mod has no files to download".into());
                        }
                        1 => Self::download_and_install(&ww2, files[0].clone()),
                        _ => {
                            let items: Vec<GbFileItem> = files
                                .iter()
                                .map(|f| GbFileItem {
                                    name: f.name.as_str().into(),
                                    size: format_size(f.size).into(),
                                    downloads: i32::try_from(f.download_count).unwrap_or(i32::MAX),
                                })
                                .collect();
                            STATE.lock().unwrap().files = files;
                            win.set_gb_files(Rc::new(VecModel::from(items)).into());
                            win.set_gb_files_mod_name(mod_name.as_str().into());
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
            let file = usize::try_from(index)
                .ok()
                .and_then(|i| STATE.lock().unwrap().files.get(i).cloned());
            if let Some(file) = file {
                Self::download_and_install(&ww, file);
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
                error!("[GbBrowser] could not open '{url}': {e}");
            }
        });
    }
}
