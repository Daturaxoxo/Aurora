use crate::{IconPackChoice, LogWindow, MainWindow};
use log::*;
use shared::config::{self, key};
use slint::{Color, ComponentHandle as _, ModelRc, SharedPixelBuffer, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;

include!(concat!(env!("OUT_DIR"), "/icon_packs.rs"));
pub const DEFAULT_PACK: &str = "light";
const SAMPLES: [&str; 4] = ["settings", "folder", "download", "search"];
const OPAQUE_ENOUGH: u8 = 200;

thread_local! {
    static ACTIVE: RefCell<String> = const { RefCell::new(String::new()) };
    static CACHE: RefCell<HashMap<(&'static str, &'static str), slint::Image>> = RefCell::new(HashMap::new());
    static TINTS: RefCell<HashMap<String, (Color, Color)>> = RefCell::new(HashMap::new());
}

pub struct IconPackHandler;

impl IconPackHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let saved = config::get(key::ICON_PACK)
            .as_str()
            .unwrap_or(DEFAULT_PACK)
            .to_string();
        let pack = resolve_pack(&saved);

        if pack != saved {
            warn!("[Icons] pack '{saved}' is gone; falling back to '{pack}'");
            config::set(key::ICON_PACK, pack.clone());
        }

        Self::apply(window, &pack);
        Self::bind(window);
    }

    pub fn apply(window: &slint::Weak<MainWindow>, pack: &str) {
        let pack = resolve_pack(pack);
        ACTIVE.with(|cell| *cell.borrow_mut() = pack.clone());

        let Some(w) = window.upgrade() else {
            error!("[Icons] cannot apply '{pack}': the window is gone");
            return;
        };

        apply_icons!(&w, |slug: &str| image_for(&pack, slug), tint_for(&pack));
        w.set_icon_pack_name(display_name(&pack).into());
        Self::publish_choices(&w, &pack);
        crate::classes::logwindow::apply_icons();
    }

    pub fn apply_from_theme(window: &slint::Weak<MainWindow>, pack: Option<&str>) {
        let Some(pack) = pack else { return };

        if !exists(pack) {
            warn!("[Icons] the theme asks for icon pack '{pack}', which does not exist");
            return;
        }

        if ACTIVE.with(|cell| *cell.borrow() == pack) {
            return;
        }

        info!("[Icons] the theme brings icon pack '{pack}'");
        config::set(key::ICON_PACK, pack.to_string());
        Self::apply(window, pack);
    }

    pub fn apply_to_log_window(window: &LogWindow) {
        let pack = ACTIVE.with(|cell| cell.borrow().clone());
        apply_icons!(window, |slug: &str| image_for(&pack, slug), tint_for(&pack));
    }

    fn bind(window: &slint::Weak<MainWindow>) {
        let Some(w) = window.upgrade() else {
            error!("[Icons] bind() failed - the window is gone");
            return;
        };

        let ww = window.clone();
        w.on_icon_pack_selected(move |id| {
            let id = id.to_string();

            if !exists(&id) {
                warn!("[Icons] '{id}' is no longer installed");
                return;
            }

            info!("[Icons] switching to '{id}'");
            config::set(key::ICON_PACK, id.clone());
            Self::apply(&ww, &id);
        });
    }

    fn publish_choices(w: &MainWindow, selected: &str) {
        let choices: Vec<IconPackChoice> = ICON_PACKS
            .iter()
            .map(|(pack, _)| {
                let (tint, contrast) = tint_for(pack);

                IconPackChoice {
                    id: (*pack).into(),
                    name: display_name(pack).into(),
                    selected: *pack == selected,
                    tint,
                    contrast,
                    sample_1: sample(pack, SAMPLES[0]),
                    sample_2: sample(pack, SAMPLES[1]),
                    sample_3: sample(pack, SAMPLES[2]),
                    sample_4: sample(pack, SAMPLES[3]),
                }
            })
            .collect();

        w.set_icon_pack_choices(ModelRc::new(VecModel::from(choices)));
    }
}

fn exists(pack: &str) -> bool {
    ICON_PACKS.iter().any(|(name, _)| *name == pack)
}

fn resolve_pack(pack: &str) -> String {
    if exists(pack) {
        return pack.to_string();
    }
    if exists(DEFAULT_PACK) {
        return DEFAULT_PACK.to_string();
    }
    ICON_PACKS
        .first()
        .map(|(name, _)| (*name).to_string())
        .unwrap_or_default()
}

fn display_name(pack: &str) -> String {
    let mut chars = pack.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn bytes_for(pack: &str, slug: &str) -> Option<(&'static str, &'static str, &'static [u8])> {
    let (pack, icons) = ICON_PACKS.iter().find(|(name, _)| *name == pack)?;
    let (slug, bytes) = icons.iter().find(|(name, _)| *name == slug)?;
    Some((pack, slug, bytes))
}

fn image_for(pack: &str, slug: &str) -> slint::Image {
    let found = bytes_for(pack, slug)
        .or_else(|| bytes_for(DEFAULT_PACK, slug))
        .or_else(|| {
            ICON_PACKS
                .iter()
                .find_map(|(name, _)| bytes_for(name, slug))
        });

    let Some((pack, slug, bytes)) = found else {
        warn!("[Icons] no pack provides '{slug}'");
        return slint::Image::default();
    };

    if let Some(cached) = CACHE.with(|cell| cell.borrow().get(&(pack, slug)).cloned()) {
        return cached;
    }

    let Some(image) = decode(bytes) else {
        error!("[Icons] '{pack}/{slug}' could not be decoded");
        return slint::Image::default();
    };

    CACHE.with(|cell| cell.borrow_mut().insert((pack, slug), image.clone()));
    image
}

fn tint_for(pack: &str) -> (Color, Color) {
    if let Some(cached) = TINTS.with(|cell| cell.borrow().get(pack).copied()) {
        return cached;
    };

    let mut counts: HashMap<(u8, u8, u8), usize> = HashMap::new();
    for slug in SAMPLES {
        let Some((_, _, bytes)) = bytes_for(pack, slug) else {
            continue;
        };
        let Ok(decoded) = image::load_from_memory(bytes) else {
            continue;
        };

        for pixel in decoded.into_rgba8().pixels() {
            if pixel[3] > OPAQUE_ENOUGH {
                *counts.entry((pixel[0], pixel[1], pixel[2])).or_default() += 1;
            }
        }
    }

    let tint = counts
        .into_iter()
        .max_by_key(|(_, seen)| *seen)
        .map_or(Color::from_rgb_u8(255, 255, 255), |((r, g, b), _)| {
            Color::from_rgb_u8(r, g, b)
        });

    let luminance = 0.2126 * f32::from(tint.red())
        + 0.7152 * f32::from(tint.green())
        + 0.0722 * f32::from(tint.blue());
    let contrast = if luminance > 140.0 {
        Color::from_rgb_u8(0, 0, 0)
    } else {
        Color::from_rgb_u8(255, 255, 255)
    };

    debug!("[Icons] '{pack}' tints at {tint:?}");
    TINTS.with(|cell| cell.borrow_mut().insert(pack.to_string(), (tint, contrast)));
    (tint, contrast)
}

fn sample(pack: &str, slug: &str) -> slint::Image {
    let Some((pack, slug, bytes)) = bytes_for(pack, slug) else {
        return slint::Image::default();
    };

    if let Some(cached) = CACHE.with(|cell| cell.borrow().get(&(pack, slug)).cloned()) {
        return cached;
    }

    let image = decode(bytes).unwrap_or_default();
    CACHE.with(|cell| cell.borrow_mut().insert((pack, slug), image.clone()));
    image
}

fn decode(bytes: &[u8]) -> Option<slint::Image> {
    let rgba = image::load_from_memory(bytes)
        .map_err(|e| warn!("[Icons] an icon could not be decoded: {e}"))
        .ok()?
        .into_rgba8();
    let (width, height) = rgba.dimensions();

    Some(slint::Image::from_rgba8(
        SharedPixelBuffer::clone_from_slice(rgba.as_raw(), width, height),
    ))
}
