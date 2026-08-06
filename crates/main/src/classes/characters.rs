use log::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

include!(concat!(env!("OUT_DIR"), "/character_icons.rs"));

thread_local! {
    static CACHE: RefCell<HashMap<&'static str, Option<slint::Image>>> =
        RefCell::new(HashMap::new());
}

const ALIASES: &[(&str, &str)] = &[("zero", "mc")];
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn resolve(name: &str) -> Option<&'static str> {
    let name = ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map_or(name, |(_, target)| *target);

    CHARACTER_ICONS
        .iter()
        .find(|(slug, _)| *slug == name)
        .map(|(slug, _)| *slug)
}
#[must_use]
pub fn slug_for(text: &str) -> Option<&'static str> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let stem = Path::new(text)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(text)
        .to_lowercase();

    if let Some(slug) = resolve(&stem) {
        return Some(slug);
    }

    words(text).iter().find_map(|word| resolve(word))
}

#[must_use]
pub fn icon(slug: &str) -> Option<slint::Image> {
    let (slug, bytes) = CHARACTER_ICONS.iter().find(|(name, _)| *name == slug)?;

    CACHE.with(|cache| {
        if let Some(cached) = cache.borrow().get(slug) {
            return cached.clone();
        }

        let image = decode(slug, bytes);
        cache.borrow_mut().insert(slug, image.clone());
        image
    })
}

#[must_use]
pub fn icon_for(text: &str) -> Option<slint::Image> {
    icon(slug_for(text)?)
}

fn decode(slug: &str, bytes: &[u8]) -> Option<slint::Image> {
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| warn!("[Characters] could not decode the icon for '{slug}': {e}"))
        .ok()?
        .into_rgba8();

    let (width, height) = decoded.dimensions();
    let buffer =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&decoded, width, height);

    Some(slint::Image::from_rgba8(buffer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_character_icon_decodes() {
        assert!(!CHARACTER_ICONS.is_empty(), "no character icons embedded");
        for (slug, bytes) in CHARACTER_ICONS {
            assert!(decode(slug, bytes).is_some(), "{slug} did not decode");
        }
    }

    #[test]
    fn matches_the_shapes_mods_actually_use() {
        assert_eq!(slug_for("Shinku"), Some("shinku"));
        assert_eq!(slug_for("shinku.png"), Some("shinku"));
        assert_eq!(slug_for("Shinku - Summer Dress_P"), Some("shinku"));
        assert_eq!(slug_for("Nothing In Particular"), None);
        assert_eq!(slug_for(""), None);
    }

    #[test]
    fn both_zeros_draw_as_the_mc() {
        assert_eq!(slug_for("Zero (F)"), Some("mc"));
        assert_eq!(slug_for("Zero (M)"), Some("mc"));
        assert_eq!(slug_for("zero"), Some("mc"));
        assert_eq!(slug_for("MC"), Some("mc"));
    }

    #[test]
    fn every_gamebanana_character_has_an_icon() {
        for (name, _) in crate::classes::pages::gbbrowser::CHARACTERS {
            assert!(slug_for(name).is_some(), "no icon matches '{name}'");
        }
    }
}
