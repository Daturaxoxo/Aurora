use crate::{MainWindow, ScreenshotItem};

use chrono::{DateTime, Local, NaiveDateTime};
use log::*;
use once_cell::sync::Lazy;
use shared::config;
use slint::{Model, VecModel};
use sysinfo::{CpuRefreshKind, RefreshKind, System};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

const FAVORITES_KEY: &str = "screenshot_favorites";
const THUMB_MAX_W: u32 = 512;
const THUMB_MAX_H: u32 = 512;

#[derive(Debug, Clone)]
struct Screenshot {
    path: PathBuf,
    file_name: String,
    timestamp: i64,
    date: String,
    favorite: bool,
}

#[derive(Default, Debug, Clone, Copy)]
enum SortMode {
    #[default]
    Newest,
    Oldest,
    Name,
}

#[derive(Default)]
struct State {
    displayed: Vec<PathBuf>,
    sort_mode: SortMode,
    favorites_only: bool,
    selected: HashSet<PathBuf>,
    pending_delete: Vec<PathBuf>,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

static GENERATION: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static THUMB_CACHE: RefCell<HashMap<PathBuf, slint::Image>> =
        RefCell::new(HashMap::new());
}
static PREVIEW_GENERATION: AtomicU64 = AtomicU64::new(0);

fn screenshot_folder() -> Option<PathBuf> {
    dirs::picture_dir().map(|p| p.join("NevernessToEverness"))
}

fn favorites() -> HashSet<String> {
    config::get(FAVORITES_KEY)
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn save_favorites(favs: &HashSet<String>) {
    let mut list: Vec<String> = favs.iter().cloned().collect();
    list.sort();
    config::set(FAVORITES_KEY, list);
}

fn parse_name_timestamp(stem: &str) -> Option<NaiveDateTime> {
    let parts: Vec<u32> = stem
        .split('_')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [yy, mm, dd, h, m, s] = parts.as_slice() else {
        return None;
    };
    chrono::NaiveDate::from_ymd_opt(2000 + *yy as i32, *mm, *dd)?.and_hms_opt(*h, *m, *s)
}

fn created_timestamp(path: &Path) -> Option<NaiveDateTime> {
    let meta = std::fs::metadata(path).ok()?;
    let created = meta.created().or_else(|_| meta.modified()).ok()?;
    Some(DateTime::<Local>::from(created).naive_local())
}

fn scan() -> Vec<Screenshot> {
    let Some(folder) = screenshot_folder() else {
        warn!("Could not resolve the Pictures directory");
        return Vec::new();
    };

    let entries = match std::fs::read_dir(&folder) {
        Ok(e) => e,
        Err(e) => {
            warn!("Could not read '{}': {e}", folder.display());
            return Vec::new();
        }
    };

    let favs = favorites();
    let mut shots = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let is_png = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("png"));
        if !path.is_file() || !is_png {
            continue;
        }

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();

        let dt = parse_name_timestamp(&stem).or_else(|| created_timestamp(&path));
        let (timestamp, date) = dt.map_or((0, String::new()), |dt| {
            (
                dt.and_utc().timestamp(),
                dt.format("%Y-%m-%d %H:%M").to_string(),
            )
        });

        shots.push(Screenshot {
            favorite: favs.contains(&file_name),
            path,
            file_name,
            timestamp,
            date,
        });
    }

    shots
}

fn ordered(
    mut shots: Vec<Screenshot>,
    sort_mode: SortMode,
    favorites_only: bool,
) -> Vec<Screenshot> {
    if favorites_only {
        shots.retain(|s| s.favorite);
    }

    match sort_mode {
        SortMode::Newest => shots.sort_by_key(|s| s.timestamp),
        SortMode::Name => {
            shots.sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()))
        }
        SortMode::Oldest => shots.sort_by_key(|s| std::cmp::Reverse(s.timestamp)),
    }

    shots.sort_by_key(|s| !s.favorite);
    shots
}

fn load_rgba(path: &Path, max_w: u32, max_h: u32) -> anyhow::Result<(Vec<u8>, u32, u32)> {
    let img = image::open(path)?;
    let img = if img.width() > max_w || img.height() > max_h {
        img.thumbnail(max_w, max_h)
    } else {
        img
    };
    let rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((rgba.into_raw(), w, h))
}

pub struct ScreenshotHandler;

impl ScreenshotHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("[Screenshots] setup() called");
        let s = System::new_with_specifics(
            RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()),
        );

        rayon::ThreadPoolBuilder::new()
            .num_threads(s.cpus().iter().count() / 2)
            .build_global()
            .unwrap();
        Self::bind(window);
        Self::reload(window);
        info!("[Screenshots] setup() complete");
    }

    /// Rescans the folder and rebuilds the model off the UI thread
    fn reload(window: &slint::Weak<MainWindow>) {
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let ww = window.clone();

        std::thread::spawn(move || {
            let (sort_mode, favorites_only) = {
                let state = STATE.lock().unwrap();
                (state.sort_mode, state.favorites_only)
            };
            let all = scan();
            let all_paths: HashSet<PathBuf> = all.iter().map(|s| s.path.clone()).collect();
            let shots = ordered(all, sort_mode, favorites_only);

            let _ = slint::invoke_from_event_loop(move || {
                if GENERATION.load(Ordering::SeqCst) != generation {
                    return;
                }
                let Some(w) = ww.upgrade() else {
                    error!("Could not load: window handle is dead");
                    return;
                };

                let (items, jobs, selected_count) = {
                    let mut state = STATE.lock().unwrap();

                    state.displayed = shots.iter().map(|s| s.path.clone()).collect();
                    let existing: HashSet<PathBuf> = state.displayed.iter().cloned().collect();
                    state.selected.retain(|p| existing.contains(p));

                    THUMB_CACHE.with(|cache| {
                        let mut cache = cache.borrow_mut();
                        cache.retain(|p, _| all_paths.contains(p));

                        let items: Vec<ScreenshotItem> = shots
                            .iter()
                            .map(|s| ScreenshotItem {
                                file_name: s.file_name.clone().into(),
                                date: s.date.clone().into(),
                                image: cache.get(&s.path).cloned().unwrap_or_default(),
                                favorite: s.favorite,
                                selected: state.selected.contains(&s.path),
                            })
                            .collect();
                        let jobs: Vec<(usize, PathBuf)> = shots
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| !cache.contains_key(&s.path))
                            .map(|(i, s)| (i, s.path.clone()))
                            .collect();
                        (items, jobs, state.selected.len())
                    })
                };

                w.set_screenshots(Rc::new(VecModel::from(items)).into());
                w.set_screenshot_selected_count(selected_count as i32);
                Self::load_thumbnails(&ww, generation, jobs);
            });
        });
    }

    fn load_thumbnails(
        window: &slint::Weak<MainWindow>,
        generation: u64,
        jobs: Vec<(usize, PathBuf)>,
    ) {
        use rayon::prelude::*;

        let ww = window.clone();
        std::thread::spawn(move || {
            jobs.into_par_iter().for_each(|(index, path)| {
                if GENERATION.load(Ordering::SeqCst) != generation {
                    return;
                }

                let rgba = load_rgba(&path, THUMB_MAX_W, THUMB_MAX_H)
                    .map_err(|e| {
                        warn!("Could not load thumbnail '{}': {e}", path.display());
                    })
                    .ok();
                let Some((raw, w, h)) = rgba else { return };

                let ww = ww.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if GENERATION.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    let Some(ui) = ww.upgrade() else { return };

                    let buffer =
                        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&raw, w, h);
                    let image = slint::Image::from_rgba8(buffer);
                    THUMB_CACHE.with(|cache| {
                        cache.borrow_mut().insert(path, image.clone());
                    });

                    let model = ui.get_screenshots();
                    if let Some(mut row) = model.row_data(index) {
                        row.image = image;
                        model.set_row_data(index, row);
                    }
                });
            });
        });
    }

    fn path_at(index: i32) -> Option<PathBuf> {
        let i = usize::try_from(index).ok()?;
        STATE.lock().unwrap().displayed.get(i).cloned()
    }

    pub fn confirm_delete(window: &slint::Weak<MainWindow>) {
        let pending = std::mem::take(&mut STATE.lock().unwrap().pending_delete);
        if pending.is_empty() {
            return;
        }

        let mut favs = favorites();
        let mut favs_changed = false;
        for path in &pending {
            if let Err(e) = std::fs::remove_file(path) {
                error!("Could not delete '{}': {e}", path.display());
            } else {
                info!("Deleted '{}'", path.display());
            }

            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if favs.remove(&name) {
                favs_changed = true;
            }
            STATE.lock().unwrap().selected.remove(path);
        }
        if favs_changed {
            save_favorites(&favs);
        }

        Self::reload(window);
    }

    fn show_delete_popup(w: &MainWindow, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }

        let message = if paths.len() == 1 {
            let name = paths[0]
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("\"{name}\" will be permanently deleted. This cannot be undone.")
        } else {
            format!(
                "{} screenshots will be permanently deleted. This cannot be undone.",
                paths.len()
            )
        };

        STATE.lock().unwrap().pending_delete = paths;

        w.set_popup_id("screenshot-delete".into());
        w.set_popup_title("Delete Screenshots?".into());
        w.set_popup_message(message.into());
        w.set_popup_active(true);
    }

    // [CALLBACKS]

    fn bind(window: &slint::Weak<MainWindow>) {
        let w = window.unwrap();

        let ww = window.clone();
        w.on_screenshot_favorite(move |index| {
            let Some(path) = Self::path_at(index) else {
                return;
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            let mut favs = favorites();
            if !favs.remove(&name) {
                favs.insert(name);
            }
            save_favorites(&favs);
            Self::reload(&ww);
        });

        let ww = window.clone();
        w.on_screenshot_delete(move |index| {
            let Some(path) = Self::path_at(index) else {
                return;
            };
            let Some(win) = ww.upgrade() else { return };
            Self::show_delete_popup(&win, vec![path]);
        });

        let ww = window.clone();
        w.on_screenshot_delete_selected(move || {
            let Some(win) = ww.upgrade() else { return };
            let paths: Vec<PathBuf> = {
                let state = STATE.lock().unwrap();
                state
                    .displayed
                    .iter()
                    .filter(|p| state.selected.contains(*p))
                    .cloned()
                    .collect()
            };
            Self::show_delete_popup(&win, paths);
        });

        w.on_screenshot_open(move |index| {
            let Some(path) = Self::path_at(index) else {
                return;
            };
            if let Err(e) = open::that(&path) {
                error!("Could not open '{}': {e}", path.display());
            }
        });

        let ww = window.clone();
        w.on_screenshots_refresh(move || {
            Self::reload(&ww);
        });

        w.on_open_screenshots_folder(move || {
            let Some(folder) = screenshot_folder() else {
                return;
            };
            if let Err(e) = open::that(&folder) {
                error!("Could not open folder '{}': {e}", folder.display());
            }
        });

        w.on_screenshot_copy(move |index| {
            let Some(path) = Self::path_at(index) else {
                return;
            };
            std::thread::spawn(move || {
                let rgba = load_rgba(&path, u32::MAX, u32::MAX)
                    .map_err(|e| {
                        error!("Could not read '{}' for clipboard: {e}", path.display());
                    })
                    .ok();
                let Some((raw, w, h)) = rgba else { return };

                let image_data = arboard::ImageData {
                    width: w as usize,
                    height: h as usize,
                    bytes: raw.into(),
                };
                match arboard::Clipboard::new().and_then(|mut c| c.set_image(image_data)) {
                    Ok(()) => info!("Copied '{}' to clipboard", path.display()),
                    Err(e) => error!("Could not copy to clipboard: {e}"),
                }
            });
        });

        let ww = window.clone();
        w.on_screenshot_rename(move |index, new_name| {
            let Some(path) = Self::path_at(index) else {
                return;
            };

            let mut name = new_name.trim().to_string();
            if name.is_empty() || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
                warn!("Invalid rename target '{name}', ignoring");
                return;
            }
            if !name.to_lowercase().ends_with(".png") {
                name.push_str(".png");
            }

            let target = path.with_file_name(&name);
            if target == path {
                return;
            }
            if target.exists() {
                warn!("Rename target '{name}' already exists, ignoring");
                return;
            }

            match std::fs::rename(&path, &target) {
                Ok(()) => {
                    info!("Renamed '{}' → '{name}'", path.display());
                    let old_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let mut favs = favorites();
                    if favs.remove(&old_name) {
                        favs.insert(name);
                        save_favorites(&favs);
                    }
                }
                Err(e) => error!("Could not rename '{}': {e}", path.display()),
            }
            Self::reload(&ww);
        });

        let ww = window.clone();
        w.on_screenshot_toggle_select(move |index| {
            let Some(path) = Self::path_at(index) else {
                return;
            };
            let Some(win) = ww.upgrade() else { return };

            let (selected, count) = {
                let mut state = STATE.lock().unwrap();
                let selected = if state.selected.remove(&path) {
                    false
                } else {
                    state.selected.insert(path);
                    true
                };
                (selected, state.selected.len())
            };

            let model = win.get_screenshots();
            if let Ok(i) = usize::try_from(index) {
                if let Some(mut row) = model.row_data(i) {
                    row.selected = selected;
                    model.set_row_data(i, row);
                }
            }
            win.set_screenshot_selected_count(count as i32);
        });

        let ww = window.clone();
        w.on_screenshot_selection_cleared(move || {
            STATE.lock().unwrap().selected.clear();
            let Some(win) = ww.upgrade() else { return };

            let model = win.get_screenshots();
            for i in 0..model.row_count() {
                if let Some(mut row) = model.row_data(i) {
                    if row.selected {
                        row.selected = false;
                        model.set_row_data(i, row);
                    }
                }
            }
            win.set_screenshot_selected_count(0);
        });

        let ww = window.clone();
        w.on_screenshot_sort_changed(move |mode| {
            STATE.lock().unwrap().sort_mode = match mode {
                0 => SortMode::Newest,
                1 => SortMode::Oldest,
                2 => SortMode::Name,
                _ => unreachable!("invalid sort mode"),
            };
            Self::reload(&ww);
        });

        let ww = window.clone();
        w.on_screenshot_favorites_filter_changed(move |enabled| {
            STATE.lock().unwrap().favorites_only = enabled;
            Self::reload(&ww);
        });

        let ww = window.clone();
        w.on_screenshot_preview_requested(move |index| {
            let Some(path) = Self::path_at(index) else {
                return;
            };
            let Some(win) = ww.upgrade() else { return };

            if let Ok(i) = usize::try_from(index) {
                if let Some(row) = win.get_screenshots().row_data(i) {
                    win.set_screenshot_preview_image(row.image);
                }
            }

            let generation = PREVIEW_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            let ww = ww.clone();
            std::thread::spawn(move || {
                let rgba = load_rgba(&path, u32::MAX, u32::MAX)
                    .map_err(|e| {
                        warn!("Could not load preview '{}': {e}", path.display());
                    })
                    .ok();
                let Some((raw, w, h)) = rgba else { return };

                let _ = slint::invoke_from_event_loop(move || {
                    if PREVIEW_GENERATION.load(Ordering::SeqCst) != generation {
                        return;
                    }
                    let Some(win) = ww.upgrade() else { return };
                    let buffer =
                        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&raw, w, h);
                    win.set_screenshot_preview_image(slint::Image::from_rgba8(buffer));
                });
            });
        });

        info!("[Screenshots] bind() complete");
    }
}
