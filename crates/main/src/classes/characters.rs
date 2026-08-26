use log::*;
use std::cell::RefCell;
use std::collections::HashMap;

include!(concat!(env!("OUT_DIR"), "/character_icons.rs"));

thread_local! {
    static CACHE: RefCell<HashMap<&'static str, Option<slint::Image>>> =
        RefCell::new(HashMap::new());
}

const ALIASES: &[(&str, &str)] = &[("zero", "mc")];

fn known(slug: &str) -> Option<&'static str> {
    CHARACTER_ICONS
        .iter()
        .find(|(name, _)| *name == slug)
        .map(|(name, _)| *name)
}

#[must_use]
pub fn character_for(text: &str) -> Option<&'static str> {
    let text = text.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }

    // Whichever name comes first in the text wins
    ALIASES
        .iter()
        .filter_map(|(alias, target)| Some((*alias, known(target)?)))
        .chain(CHARACTER_ICONS.iter().map(|(slug, _)| (*slug, *slug)))
        .filter_map(|(needle, slug)| Some((text.find(needle)?, needle.len(), slug)))
        .min_by_key(|&(at, len, _)| (at, std::cmp::Reverse(len)))
        .map(|(_, _, slug)| slug)
}

#[must_use]
pub fn display_name(slug: &str) -> String {
    if slug.len() <= 2 {
        return slug.to_uppercase();
    }

    let mut chars = slug.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
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
    icon(character_for(text)?)
}

pub fn slugs() -> impl Iterator<Item = &'static str> {
    CHARACTER_ICONS.iter().map(|(slug, _)| *slug)
}

#[must_use]
pub fn icon_bytes(slug: &str) -> Option<&'static [u8]> {
    CHARACTER_ICONS
        .iter()
        .find(|(name, _)| *name == slug)
        .map(|(_, bytes)| *bytes)
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
        assert_eq!(character_for("Shinku"), Some("shinku"));
        assert_eq!(character_for("shinku.png"), Some("shinku"));
        assert_eq!(character_for("Shinku - Summer Dress_P"), Some("shinku"));
        assert_eq!(character_for("MyShinkuRetexture"), Some("shinku"));
        assert_eq!(character_for("Nothing In Particular"), None);
        assert_eq!(character_for(""), None);
    }

    #[test]
    fn slugs_read_back_as_names() {
        assert_eq!(display_name("shinku"), "Shinku");
        assert_eq!(display_name("mc"), "MC");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn both_zeros_draw_as_the_mc() {
        assert_eq!(character_for("Zero (F)"), Some("mc"));
        assert_eq!(character_for("Zero (M)"), Some("mc"));
        assert_eq!(character_for("zero"), Some("mc"));
        assert_eq!(character_for("MC"), Some("mc"));
    }

    #[test]
    fn every_gamebanana_character_has_an_icon() {
        for (name, _) in crate::classes::pages::gbbrowser::CHARACTERS {
            assert!(character_for(name).is_some(), "no icon matches '{name}'");
        }
    }
}
