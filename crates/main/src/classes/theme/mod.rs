pub mod model;
pub mod wallpaper;
use crate::classes::iconpack::IconPackHandler;
use crate::classes::toast::ToastHandler;
use crate::translations::tr;
use crate::{MainWindow, Palette, ThemeChoice};
use anyhow::{Context, Result, anyhow};
use log::*;
use model::Theme;
use shared::config::{self, key};
use slint::{ComponentHandle as _, Model as _, ModelRc, SharedPixelBuffer, VecModel};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

const PREVIEW_EDGE: u32 = 320;

thread_local! {
    static THEMES: RefCell<Vec<Theme>> = const { RefCell::new(Vec::new()) };
    static PLAYBACK: RefCell<Option<Rc<Playback>>> = const { RefCell::new(None) };
}

/// The animation on screen. Holding this keeps the decoder thread alive; drop it
/// and the worker's next send fails, which is how playback stops.
struct Playback {
    timer: slint::Timer,
    frames: std::sync::mpsc::Receiver<wallpaper::Frame>,
    /// What the timer is currently set to, so it is only re-armed when the next
    /// frame is held for a different length of time.
    interval: Cell<Duration>,
    /// Whether playback is currently held because nothing is on screen.
    paused: Cell<bool>,
}

pub struct ThemeHandler;

impl ThemeHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        if let Err(e) = std::fs::create_dir_all(model::user_dir()) {
            warn!("could not create the themes folder: {e}");
        }

        Self::reload(window);
        Self::bind(window);
    }

    pub fn reload(window: &slint::Weak<MainWindow>) {
        let themes = model::list();
        let selected_id = config::get(key::THEME)
            .as_str()
            .unwrap_or(model::DEFAULT_ID)
            .to_string();
        let selected = model::find(&themes, &selected_id);

        if selected.id != selected_id {
            warn!(
                "theme '{selected_id}' is gone; falling back to '{}'",
                selected.id
            );
            config::set(key::THEME, selected.id.clone());
        }
        THEMES.with(|cell| *cell.borrow_mut() = themes);

        Self::apply(window, &selected);
        Self::publish_choices(window, &selected.id);
        Self::load_previews(window);
    }

    fn apply(window: &slint::Weak<MainWindow>, theme: &Theme) {
        let Some(w) = window.upgrade() else {
            error!("[Theme] cannot apply '{}': the window is gone", theme.id);
            return;
        };

        let palette = w.global::<Palette>();
        palette.set_background_top(theme.background.0);
        palette.set_background_bottom(theme.background.1);
        palette.set_border_top(theme.border.0);
        palette.set_border_bottom(theme.border.1);
        palette.set_shadow_color(theme.shadow);
        palette.set_text_primary(theme.text_primary);
        palette.set_text_secondary(theme.text_secondary);
        palette.set_surface(theme.surface);
        palette.set_backdrop(theme.backdrop);
        palette.set_scrim_top(theme.scrim.0);
        palette.set_scrim_bottom(theme.scrim.1);
        palette.set_accent(theme.accent);
        palette.set_accent_strong(theme.accent_strong);
        palette.set_danger(theme.danger);
        palette.set_warning(theme.warning);
        palette.set_success(theme.success);
        palette.set_favorite(theme.favorite);
        w.set_theme_name(theme.name.as_str().into());
        w.set_theme_author(theme.author.as_str().into());
        stop_playback();

        let Some(image) = theme.image.clone() else {
            palette.set_background_image(slint::Image::default());
            return;
        };

        let ww = window.clone();
        let id = theme.id.clone();
        std::thread::spawn(move || {
            let opened = wallpaper::bytes_for(&image).and_then(wallpaper::open);

            let opened = match opened {
                Ok(opened) => opened,
                Err(e) => {
                    warn!("[Theme] '{id}' has no usable wallpaper: {e:#}");
                    return;
                }
            };

            let _ = slint::invoke_from_event_loop(move || {
                let Some(w) = ww.upgrade() else { return };
                install_wallpaper(&w, opened);
            });
        });
    }

    fn publish_choices(window: &slint::Weak<MainWindow>, selected_id: &str) {
        let Some(w) = window.upgrade() else { return };

        let existing: std::collections::HashMap<String, slint::Image> = w
            .get_theme_choices()
            .iter()
            .map(|choice| (choice.id.to_string(), choice.preview))
            .collect();

        let choices: Vec<ThemeChoice> = THEMES.with(|cell| {
            cell.borrow()
                .iter()
                .map(|theme| ThemeChoice {
                    id: theme.id.as_str().into(),
                    name: theme.name.as_str().into(),
                    author: theme.author.as_str().into(),
                    preview: existing.get(&theme.id).cloned().unwrap_or_default(),
                    backdrop: theme.backdrop,
                    background_top: theme.background.0,
                    background_bottom: theme.background.1,
                    border_top: theme.border.0,
                    scrim_top: theme.scrim.0,
                    scrim_bottom: theme.scrim.1,
                    text_primary: theme.text_primary,
                    text_secondary: theme.text_secondary,
                    accent: theme.accent,
                    builtin: theme.builtin,
                    selected: theme.id == selected_id,
                })
                .collect()
        });

        w.set_theme_choices(ModelRc::new(VecModel::from(choices)));
    }

    fn load_previews(window: &slint::Weak<MainWindow>) {
        let pending: Vec<(String, model::ImageRef)> = THEMES.with(|cell| {
            cell.borrow()
                .iter()
                .filter_map(|theme| Some((theme.id.clone(), theme.image.clone()?)))
                .collect()
        });

        if pending.is_empty() {
            return;
        }

        let ww = window.clone();
        std::thread::spawn(move || {
            for (id, image) in pending {
                let decoded = wallpaper::bytes_for(&image)
                    .and_then(|bytes| wallpaper::thumbnail(&bytes, PREVIEW_EDGE));

                let decoded = match decoded {
                    Ok(decoded) => decoded,
                    Err(e) => {
                        debug!("[Theme] no preview for '{id}': {e:#}");
                        continue;
                    }
                };
                let ww = ww.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = ww.upgrade() else { return };
                    let image = to_image(&decoded.pixels, decoded.width, decoded.height);
                    set_preview(&w, &id, &image);
                });
            }
        });
    }

    fn bind(window: &slint::Weak<MainWindow>) {
        let Some(w) = window.upgrade() else {
            error!("[Theme] bind() failed - the window is gone");
            return;
        };

        let ww = window.clone();
        w.on_theme_selected(move |id| {
            let id = id.to_string();
            let theme =
                THEMES.with(|cell| cell.borrow().iter().find(|theme| theme.id == id).cloned());

            let Some(theme) = theme else {
                warn!("[Theme] '{id}' is no longer in the list");
                return;
            };

            info!("[Theme] switching to '{}'", theme.id);
            config::set(key::THEME, theme.id.clone());
            Self::apply(&ww, &theme);
            Self::publish_choices(&ww, &theme.id);
            IconPackHandler::apply_from_theme(&ww, theme.icon_pack.as_deref());
        });

        let ww = window.clone();
        w.on_theme_import(move || {
            let ww = ww.clone();
            std::thread::spawn(move || {
                let picked = rfd::FileDialog::new()
                    .set_title("Select Aurora Themes")
                    .add_filter("Aurora themes", &[model::EXTENSION])
                    .pick_files();

                let Some(files) = picked else { return };
                Self::import_paths(&ww, files);
            });
        });

        let ww = window.clone();
        w.on_theme_delete(move |id| {
            let id = id.to_string();
            let path = THEMES.with(|cell| {
                cell.borrow()
                    .iter()
                    .find(|theme| theme.id == id)
                    .and_then(|theme| theme.path.clone())
            });

            let Some(path) = path else {
                warn!("[Theme] '{id}' has no file to remove");
                return;
            };

            match std::fs::remove_file(&path) {
                Ok(()) => info!("[Theme] removed '{}'", path.display()),
                Err(e) => {
                    error!("[Theme] could not remove '{}': {e}", path.display());
                    ToastHandler::show(&ww, tr("toast-theme-delete-failed"), "error");
                    return;
                }
            }

            if config::get(key::THEME).as_str() == Some(id.as_str()) {
                config::set(key::THEME, model::DEFAULT_ID);
            }
            Self::reload(&ww);
        });

        w.on_theme_open_folder(move || {
            let folder = model::user_dir();
            let _ = std::fs::create_dir_all(&folder);
            if let Err(e) = shared::utils::open_folder(&folder) {
                error!("[Theme] could not open '{}': {e}", folder.display());
            }
        });
    }

    pub fn import_paths(window: &slint::Weak<MainWindow>, paths: Vec<PathBuf>) {
        let mut imported = Vec::new();
        let mut failed = 0;

        for path in paths {
            match Self::import_one(&path) {
                Ok(id) => imported.push(id),
                Err(e) => {
                    failed += 1;
                    error!("[Theme] could not import '{}': {e:#}", path.display());
                }
            }
        }

        if let Some(last) = imported.last() {
            config::set(key::THEME, last.clone());
        }

        let ww = window.clone();
        let count = imported.len();
        let selected = imported.last().cloned();
        let _ = slint::invoke_from_event_loop(move || {
            Self::reload(&ww);

            if let Some(id) = selected {
                let pack = THEMES.with(|cell| {
                    cell.borrow()
                        .iter()
                        .find(|theme| theme.id == id)
                        .and_then(|theme| theme.icon_pack.clone())
                });
                IconPackHandler::apply_from_theme(&ww, pack.as_deref());
            }

            if count > 0 {
                ToastHandler::show(&ww, tr("toast-theme-imported"), "success");
            }
            if failed > 0 {
                ToastHandler::show(&ww, tr("toast-theme-import-failed"), "error");
            }
        });
    }

    fn import_one(path: &Path) -> Result<String> {
        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(model::EXTENSION))
        {
            return Err(anyhow!(
                "'{}' is not a .{} file",
                path.display(),
                model::EXTENSION
            ));
        }

        let theme = model::parse(path)?;
        let file_name = path
            .file_name()
            .ok_or_else(|| anyhow!("'{}' has no file name", path.display()))?;
        let dir = model::user_dir();
        std::fs::create_dir_all(&dir).with_context(|| format!("creating '{}'", dir.display()))?;
        let destination = dir.join(file_name);

        if destination != path {
            std::fs::copy(path, &destination).with_context(|| {
                format!(
                    "copying '{}' to '{}'",
                    path.display(),
                    destination.display()
                )
            })?;
        }

        info!(
            "[Theme] imported '{}' by '{}' as '{}'",
            theme.name,
            theme.author,
            destination.display()
        );
        Ok(theme.id)
    }
}

fn to_image(pixels: &[u8], width: u32, height: u32) -> slint::Image {
    let buffer = SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(pixels, width, height);
    slint::Image::from_rgba8(buffer)
}

fn set_preview(w: &MainWindow, id: &str, image: &slint::Image) {
    let model = w.get_theme_choices();
    for index in 0..model.row_count() {
        if let Some(mut choice) = model.row_data(index)
            && choice.id == id
        {
            choice.preview = image.clone();
            model.set_row_data(index, choice);
            break;
        }
    }
}

fn install_wallpaper(w: &MainWindow, wallpaper: wallpaper::Wallpaper) {
    match wallpaper {
        wallpaper::Wallpaper::Still(still) => {
            stop_playback();
            w.global::<Palette>().set_background_image(to_image(
                &still.pixels,
                still.width,
                still.height,
            ));
        }
        wallpaper::Wallpaper::Animated {
            width,
            height,
            frames,
        } => start_playback(w, width, height, frames),
    }
}

fn stop_playback() {
    PLAYBACK.with(|cell| {
        if let Some(playback) = cell.borrow_mut().take() {
            playback.timer.stop();
        }
    });
}

fn start_playback(
    w: &MainWindow,
    width: u32,
    height: u32,
    frames: std::sync::mpsc::Receiver<wallpaper::Frame>,
) {
    stop_playback();
    let Ok(first) = frames.recv() else {
        warn!("[Theme] the animation ended before it produced a frame");
        return;
    };
    w.global::<Palette>()
        .set_background_image(to_image(&first.pixels, first.width, first.height));

    info!("[Theme] playing an animated wallpaper at {width}x{height}");

    let playback = Rc::new(Playback {
        timer: slint::Timer::default(),
        frames,
        interval: Cell::new(first.delay),
        paused: Cell::new(false),
    });

    let weak_window = w.as_weak();
    let weak_playback = Rc::downgrade(&playback);

    playback
        .timer
        .start(slint::TimerMode::Repeated, first.delay, move || {
            let (Some(playback), Some(w)) = (weak_playback.upgrade(), weak_window.upgrade()) else {
                return;
            };

            let window = w.window();
            let hidden = !window.is_visible() || window.is_minimized();
            if hidden != playback.paused.get() {
                playback.paused.set(hidden);
                debug!(
                    "[Theme] the wallpaper {} (visible={}, minimized={})",
                    if hidden { "paused" } else { "resumed" },
                    window.is_visible(),
                    window.is_minimized()
                );
            }
            if hidden {return}

            let frame = match playback.frames.try_recv() {
                Ok(frame) => frame,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    warn!("[Theme] the wallpaper decoder stopped; the last frame stays on screen");
                    playback.timer.stop();
                    return;
                }
            };

            w.global::<Palette>().set_background_image(to_image(
                &frame.pixels,
                frame.width,
                frame.height,
            ));

            if frame.delay != playback.interval.get() {
                playback.interval.set(frame.delay);
                playback.timer.set_interval(frame.delay);
            }
        });

    PLAYBACK.with(|cell| *cell.borrow_mut() = Some(playback));
}
