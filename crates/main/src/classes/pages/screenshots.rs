use crate::{MainWindow, ScreenshotItem};

use chrono::{DateTime, Local, NaiveDateTime};
use log::*;
use once_cell::sync::Lazy;
use shared::classes::info::paths::PICTURE_FOLDER;
use shared::config::{self, key};
use shared::utils::open_folder;
use slint::{Model, VecModel};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

const THUMB_MAX_W: u32 = 512;
const THUMB_MAX_H: u32 = 512;

/// Read size used when hashing a file's contents.
const HASH_CHUNK: usize = 128 * 1024;

const IGNORED_PLAYER_ID: &str = "66666";

const MIN_COPY_FEEDBACK: Duration = Duration::from_millis(150);

#[derive(Debug, Clone)]
struct Screenshot {
    path: PathBuf,
    duplicates: Vec<PathBuf>,
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
    duplicates: HashMap<PathBuf, Vec<PathBuf>>,
    sort_mode: SortMode,
    favorites_only: bool,
    selected: HashSet<PathBuf>,
    pending_delete: Vec<PathBuf>,
}

static STATE: Lazy<Mutex<State>> = Lazy::new(|| Mutex::new(State::default()));

static GENERATION: AtomicU64 = AtomicU64::new(0);

const THUMB_CACHE_MAX: usize = 200;

#[derive(Default)]
struct ThumbCache {
    images: HashMap<PathBuf, slint::Image>,
    order: VecDeque<PathBuf>,
}

impl ThumbCache {
    fn get(&self, path: &Path) -> Option<&slint::Image> {
        self.images.get(path)
    }

    fn contains_key(&self, path: &Path) -> bool {
        self.images.contains_key(path)
    }

    fn retain(&mut self, listed: &HashSet<PathBuf>) {
        self.images.retain(|p, _| listed.contains(p));
        self.order.retain(|p| listed.contains(p));
    }

    fn insert(&mut self, path: PathBuf, image: slint::Image) {
        if self.images.insert(path.clone(), image).is_none() {
            self.order.push_back(path);
        }

        while self.order.len() > THUMB_CACHE_MAX {
            let Some(evicted) = self.order.pop_front() else {
                break;
            };
            self.images.remove(&evicted);
        }
    }
}

thread_local! {
    static THUMB_CACHE: RefCell<ThumbCache> = RefCell::new(ThumbCache::default());
}
static PREVIEW_GENERATION: AtomicU64 = AtomicU64::new(0);
static COPY_GENERATION: AtomicU64 = AtomicU64::new(0);

mod copy_state {
    pub const COPIED: i32 = 2;
    pub const FAILED: i32 = 3;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Source {
    Selfie,
    Pictures,
}

fn selfie_folder() -> Option<PathBuf> {
    let game_path = PathBuf::from(config::get(key::GAME_PATH).as_str()?);
    let selfie_folder = game_path
        .join("Client")
        .join("WindowsNoEditor")
        .join("Selfie");

    selfie_folder.is_dir().then_some(selfie_folder)
}

fn pictures_folder() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    let base = shared::classes::steam::aurora_prefix()?
        .join("drive_c")
        .join("users")
        .join("steamuser")
        .join("Pictures");
    #[cfg(not(target_os = "linux"))]
    let base = dirs::picture_dir()?;

    let folder = base.join(PICTURE_FOLDER);
    folder.is_dir().then_some(folder)
}

fn pngs_in(dir: &Path) -> Vec<PathBuf> {
    let mut entries = vec![];

    let Ok(shots) = dir.read_dir() else {
        warn!("Could not read {}", dir.display());
        return entries;
    };

    for shot in shots.flatten() {
        let path = shot.path();
        if shot.file_type().is_ok_and(|t| t.is_file())
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("png"))
        {
            entries.push(path);
        }
    }

    entries
}

fn selfie_screenshot_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![];

    let Some(selfie_folder) = selfie_folder() else {
        return dirs;
    };
    let Ok(players) = selfie_folder.read_dir() else {
        warn!("Could not read {}", selfie_folder.display());
        return dirs;
    };

    for player in players.flatten() {
        if !player.file_type().is_ok_and(|t| t.is_dir())
            || player.file_name().eq_ignore_ascii_case(IGNORED_PLAYER_ID)
        {
            continue;
        }

        let screenshots_folder = player.path().join("ScreenShots");
        if screenshots_folder.is_dir() {
            dirs.push(screenshots_folder);
        } else {
            warn!(
                "Screenshots folder not found: {}",
                screenshots_folder.display()
            );
        }
    }

    dirs
}

fn selfie_screenshots() -> Vec<PathBuf> {
    selfie_screenshot_dirs()
        .iter()
        .flat_map(|dir| pngs_in(dir))
        .collect()
}

fn picture_screenshots() -> Vec<PathBuf> {
    pictures_folder()
        .as_deref()
        .map(pngs_in)
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileId {
    size: u64,
    modified: Option<SystemTime>,
}

impl FileId {
    fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            size: meta.len(),
            modified: meta.modified().ok(),
        })
    }
}

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    source: Source,
    id: FileId,
}

static HASH_CACHE: Lazy<Mutex<HashMap<PathBuf, (FileId, u64)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn hash_file(path: &Path) -> std::io::Result<u64> {
    use std::hash::Hasher as _;
    use std::io::Read as _;

    let mut file = std::fs::File::open(path)?;
    let mut hasher = twox_hash::XxHash3_64::new();
    let mut buffer = vec![0u8; HASH_CHUNK];

    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.write(&buffer[..read]);
    }

    Ok(hasher.finish())
}

fn content_hash(path: &Path, id: FileId) -> Option<u64> {
    if let Some((cached_id, hash)) = HASH_CACHE.lock().unwrap().get(path)
        && *cached_id == id
    {
        return Some(*hash);
    }

    let hash = hash_file(path)
        .map_err(|e| warn!("Could not hash '{}': {e}", path.display()))
        .ok()?;

    HASH_CACHE
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), (id, hash));

    Some(hash)
}

fn unique(candidates: Vec<Candidate>) -> Vec<(PathBuf, Vec<PathBuf>)> {
    use rayon::prelude::*;

    let mut by_size: HashMap<u64, Vec<Candidate>> = HashMap::new();
    for candidate in candidates {
        by_size
            .entry(candidate.id.size)
            .or_default()
            .push(candidate);
    }

    let mut groups: Vec<Vec<Candidate>> = Vec::new();
    let mut contested: Vec<Candidate> = Vec::new();
    for (_, mut same_size) in by_size {
        if same_size.len() == 1 {
            groups.push(std::mem::take(&mut same_size));
        } else {
            contested.append(&mut same_size);
        }
    }

    let hashed: Vec<(Option<u64>, Candidate)> = contested
        .into_par_iter()
        .map(|candidate| (content_hash(&candidate.path, candidate.id), candidate))
        .collect();

    let mut by_hash: HashMap<(u64, u64), Vec<Candidate>> = HashMap::new();
    for (hash, candidate) in hashed {
        let Some(hash) = hash else {
            groups.push(vec![candidate]);
            continue;
        };
        by_hash
            .entry((candidate.id.size, hash))
            .or_default()
            .push(candidate);
    }
    groups.extend(by_hash.into_values());

    groups
        .into_iter()
        .filter_map(|mut group| {
            group.sort_by(|a, b| a.source.cmp(&b.source).then_with(|| a.path.cmp(&b.path)));
            let mut paths = group.into_iter().map(|c| c.path);
            let primary = paths.next()?;
            Some((primary, paths.collect()))
        })
        .collect()
}

fn collect() -> Vec<(PathBuf, Vec<PathBuf>)> {
    let candidates: Vec<Candidate> = selfie_screenshots()
        .into_iter()
        .map(|path| (path, Source::Selfie))
        .chain(
            picture_screenshots()
                .into_iter()
                .map(|path| (path, Source::Pictures)),
        )
        .filter_map(|(path, source)| {
            let id = FileId::of(&path)?;
            Some(Candidate { path, source, id })
        })
        .collect();

    let seen: HashSet<PathBuf> = candidates.iter().map(|c| c.path.clone()).collect();
    HASH_CACHE
        .lock()
        .unwrap()
        .retain(|path, _| seen.contains(path));

    unique(candidates)
}

fn browsable_folder() -> Option<PathBuf> {
    let mut dirs: Vec<(usize, PathBuf)> = selfie_screenshot_dirs()
        .into_iter()
        .map(|dir| (pngs_in(&dir).len(), dir))
        .filter(|(count, _)| *count > 0)
        .collect();

    dirs.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    if dirs.len() > 1 {
        info!(
            "{} player folders hold screenshots; opening the fullest",
            dirs.len()
        );
    }

    match dirs.into_iter().next() {
        Some((_, dir)) => Some(dir),
        None => pictures_folder(),
    }
}

fn favorite_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn favorites() -> HashSet<String> {
    config::get(key::SCREENSHOT_FAVORITES)
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn update_favorites(f: impl FnOnce(&mut HashSet<String>)) {
    config::modify(|data| {
        let mut favs: HashSet<String> = data
            .get(key::SCREENSHOT_FAVORITES)
            .and_then(serde_json::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        f(&mut favs);

        let mut list: Vec<String> = favs.into_iter().collect();
        list.sort();
        data.insert(
            key::SCREENSHOT_FAVORITES.to_string(),
            serde_json::Value::from(list),
        );
    });
}

fn same_file(a: &Path, b: &Path) -> bool {
    let (Ok(a), Ok(b)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) else {
        return false;
    };
    a == b
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
    chrono::NaiveDate::from_ymd_opt(2000 + (*yy).cast_signed(), *mm, *dd)?.and_hms_opt(*h, *m, *s)
}

fn created_timestamp(path: &Path) -> Option<NaiveDateTime> {
    let meta = std::fs::metadata(path).ok()?;
    let created = meta.created().or_else(|_| meta.modified()).ok()?;
    Some(DateTime::<Local>::from(created).naive_local())
}

fn scan() -> Vec<Screenshot> {
    let screenshots = collect();
    if screenshots.is_empty() {
        warn!("Couldn't get screenshots");
        return Vec::new();
    }

    let favs = favorites();
    let mut shots = Vec::new();

    for (path, duplicates) in screenshots {
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
            favorite: favs.contains(&favorite_key(&path)),
            path,
            duplicates,
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
        SortMode::Newest => shots.sort_by_key(|s| std::cmp::Reverse(s.timestamp)),
        SortMode::Name => shots.sort_by_key(|a| a.file_name.to_lowercase()),
        SortMode::Oldest => shots.sort_by_key(|s| s.timestamp),
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

/// Encodes an image as an uncompressed BMP
#[cfg(windows)]
fn bmp_bytes(path: &Path) -> anyhow::Result<Vec<u8>> {
    use image::ImageEncoder as _;

    let rgb = image::open(path)?.into_rgb8();
    let (w, h) = rgb.dimensions();

    let mut bmp = Vec::new();
    image::codecs::bmp::BmpEncoder::new(&mut bmp).write_image(
        rgb.as_raw(),
        w,
        h,
        image::ExtendedColorType::Rgb8,
    )?;

    Ok(bmp)
}

#[cfg(windows)]
fn copy_image_to_clipboard(path: &Path) -> bool {
    let png = match std::fs::read(path) {
        Ok(png) => png,
        Err(e) => {
            error!("Could not read '{}' for clipboard: {e}", path.display());
            return false;
        }
    };

    let Some(png_format) = clipboard_win::register_format("PNG") else {
        error!("Could not register the PNG clipboard format");
        return false;
    };

    let bmp = bmp_bytes(path)
        .map_err(|e| warn!("Could not build a bitmap for '{}': {e}", path.display()))
        .ok();

    let _clipboard = match clipboard_win::Clipboard::new_attempts(10) {
        Ok(clipboard) => clipboard,
        Err(e) => {
            error!("Could not open the clipboard: {e}");
            return false;
        }
    };

    if let Err(e) = clipboard_win::raw::empty() {
        error!("Could not empty the clipboard: {e}");
        return false;
    }

    if let Err(e) = clipboard_win::raw::set_without_clear(png_format.get(), &png) {
        error!("Could not copy to clipboard: {e}");
        return false;
    }

    if let Some(bmp) = bmp
        && let Err(e) = clipboard_win::raw::set_bitmap(&bmp)
    {
        warn!("Could not add a bitmap to the clipboard: {e}");
    }

    true
}

#[cfg(not(windows))]
fn copy_image_to_clipboard(path: &Path) -> bool {
    let rgba = load_rgba(path, u32::MAX, u32::MAX)
        .map_err(|e| {
            error!("Could not read '{}' for clipboard: {e}", path.display());
        })
        .ok();
    let Some((raw, w, h)) = rgba else {
        return false;
    };

    let image_data = arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: raw.into(),
    };
    match arboard::Clipboard::new().and_then(|mut c| c.set_image(image_data)) {
        Ok(()) => true,
        Err(e) => {
            error!("Could not copy to clipboard: {e}");
            false
        }
    }
}

pub struct ScreenshotHandler;

impl ScreenshotHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("[Screenshots] setup() called");
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
                    state.duplicates = shots
                        .iter()
                        .filter(|s| !s.duplicates.is_empty())
                        .map(|s| (s.path.clone(), s.duplicates.clone()))
                        .collect();
                    let existing: HashSet<PathBuf> = state.displayed.iter().cloned().collect();
                    state.selected.retain(|p| existing.contains(p));
                    let selected = state.selected.clone();
                    drop(state);

                    THUMB_CACHE.with(|cache| {
                        let mut cache = cache.borrow_mut();
                        cache.retain(&all_paths);

                        let items: Vec<ScreenshotItem> = shots
                            .iter()
                            .map(|s| ScreenshotItem {
                                file_name: s.file_name.clone().into(),
                                date: s.date.clone().into(),
                                image: cache.get(&s.path).cloned().unwrap_or_default(),
                                favorite: s.favorite,
                                selected: selected.contains(&s.path),
                            })
                            .collect();
                        let jobs: Vec<(usize, PathBuf)> = shots
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| !cache.contains_key(&s.path))
                            .map(|(i, s)| (i, s.path.clone()))
                            .collect();
                        (items, jobs, selected.len())
                    })
                };

                w.set_screenshots(Rc::new(VecModel::from(items)).into());
                w.set_screenshot_selected_count(i32::try_from(selected_count).unwrap_or(0));
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

    fn copies(path: &Path) -> Vec<PathBuf> {
        let mut paths = vec![path.to_path_buf()];
        if let Some(duplicates) = STATE.lock().unwrap().duplicates.get(path) {
            paths.extend(duplicates.iter().cloned());
        }
        paths
    }

    fn copy_to_clipboard(path: &Path) -> bool {
        let started = Instant::now();
        let copied = copy_image_to_clipboard(path);
        if copied {
            info!(
                "Copied '{}' to clipboard in {}ms",
                path.display(),
                started.elapsed().as_millis()
            );
        }
        copied
    }

    fn report_copy(window: &slint::Weak<MainWindow>, generation: u64, copied: bool) {
        let ww = window.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if COPY_GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let Some(w) = ww.upgrade() else { return };
            w.set_screenshot_copy_state(if copied {
                copy_state::COPIED
            } else {
                copy_state::FAILED
            });
        });
    }

    pub fn cancel_delete() {
        STATE.lock().unwrap().pending_delete.clear();
    }

    pub fn confirm_delete(window: &slint::Weak<MainWindow>) {
        let pending = std::mem::take(&mut STATE.lock().unwrap().pending_delete);
        if pending.is_empty() {
            return;
        }

        let ww = window.clone();
        std::thread::spawn(move || {
            for path in &pending {
                for copy in Self::copies(path) {
                    if let Err(e) = std::fs::remove_file(&copy) {
                        error!("Could not delete '{}': {e}", copy.display());
                    } else {
                        info!("Deleted '{}'", copy.display());
                    }
                }
            }

            let favs = favorites();
            let stale: Vec<String> = pending
                .iter()
                .map(|path| favorite_key(path))
                .filter(|key| favs.contains(key))
                .collect();
            if !stale.is_empty() {
                update_favorites(|favs| {
                    for key in &stale {
                        favs.remove(key);
                    }
                });
            }

            {
                let mut state = STATE.lock().unwrap();
                for path in &pending {
                    state.selected.remove(path);
                }
            }

            let _ = slint::invoke_from_event_loop(move || {
                Self::reload(&ww);
            });
        });
    }

    fn show_delete_popup(w: &MainWindow, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            STATE.lock().unwrap().pending_delete.clear();
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
            let key = favorite_key(&path);
            update_favorites(move |favs| {
                if !favs.remove(&key) {
                    favs.insert(key);
                }
            });
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
            let Some(folder) = browsable_folder() else {
                warn!("No screenshot folder to open");
                return;
            };
            if let Err(e) = open_folder(&folder) {
                error!("Could not open folder '{}': {e}", folder.display());
            }
        });

        let ww = window.clone();
        w.on_screenshot_copy(move |index| {
            let generation = COPY_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
            let ww = ww.clone();

            let Some(path) = Self::path_at(index) else {
                Self::report_copy(&ww, generation, false);
                return;
            };

            std::thread::spawn(move || {
                let started = Instant::now();
                let copied = Self::copy_to_clipboard(&path);

                if let Some(remaining) = MIN_COPY_FEEDBACK.checked_sub(started.elapsed()) {
                    std::thread::sleep(remaining);
                }
                Self::report_copy(&ww, generation, copied);
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

            if path.with_file_name(&name) == path {
                return;
            }

            let mut renamed = false;
            for copy in Self::copies(&path) {
                let target = copy.with_file_name(&name);
                if target == copy {
                    continue;
                }
                if target.exists() && !same_file(&target, &copy) {
                    warn!(
                        "Rename target '{}' already exists, ignoring",
                        target.display()
                    );
                    continue;
                }

                match std::fs::rename(&copy, &target) {
                    Ok(()) => {
                        info!("Renamed '{}' → '{name}'", copy.display());
                        renamed = true;
                    }
                    Err(e) => error!("Could not rename '{}': {e}", copy.display()),
                }
            }

            if renamed {
                let old_key = favorite_key(&path);
                let new_key = favorite_key(&path.with_file_name(&name));
                if favorites().contains(&old_key) {
                    update_favorites(move |favs| {
                        favs.remove(&old_key);
                        favs.insert(new_key);
                    });
                }
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
            if let Ok(i) = usize::try_from(index)
                && let Some(mut row) = model.row_data(i)
            {
                row.selected = selected;
                model.set_row_data(i, row);
            }
            win.set_screenshot_selected_count(i32::try_from(count).unwrap_or(0));
        });

        let ww = window.clone();
        w.on_screenshot_selection_cleared(move || {
            STATE.lock().unwrap().selected.clear();
            let Some(win) = ww.upgrade() else { return };

            let model = win.get_screenshots();
            for i in 0..model.row_count() {
                if let Some(mut row) = model.row_data(i)
                    && row.selected
                {
                    row.selected = false;
                    model.set_row_data(i, row);
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

            if let Ok(i) = usize::try_from(index)
                && let Some(row) = win.get_screenshots().row_data(i)
            {
                win.set_screenshot_preview_image(row.image);
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
