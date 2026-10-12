use log::*;
use shared::classes::games::{Game, capabilities::CharacterCatalogue};
use std::cell::RefCell;
use std::collections::HashMap;

include!(concat!(env!("OUT_DIR"), "/character_icons.rs"));

thread_local! {
    static CACHE: RefCell<HashMap<(&'static str, &'static str), Option<slint::Image>>> =
        RefCell::new(HashMap::new());
}

type Icons = &'static [(&'static str, &'static [u8])];

#[derive(Clone, Copy)]
pub struct Characters {
    catalogue: &'static CharacterCatalogue,
    icons: Icons,
}

pub fn of(game: &'static dyn Game) -> Option<Characters> {
    let catalogue = game.characters()?.catalogue();
    let icons = CHARACTER_ICONS
        .iter()
        .find(|(assets, _)| *assets == catalogue.assets)
        .map_or(&[][..], |(_, icons)| *icons);
    Some(Characters { catalogue, icons })
}

impl Characters {
    fn known(&self, slug: &str) -> Option<&'static str> {
        self.icons
            .iter()
            .find(|(name, _)| *name == slug)
            .map(|(name, _)| *name)
    }

    #[must_use]
    pub fn character_for(&self, text: &str) -> Option<&'static str> {
        let text = text.trim().to_lowercase();
        if text.is_empty() {
            return None;
        }

        // Whichever name comes first in the text wins
        self.catalogue
            .aliases
            .iter()
            .filter_map(|(alias, target)| Some((*alias, self.known(target)?)))
            .chain(self.icons.iter().map(|(slug, _)| (*slug, *slug)))
            .filter_map(|(needle, slug)| Some((text.find(needle)?, needle.len(), slug)))
            .min_by_key(|&(at, len, _)| (at, std::cmp::Reverse(len)))
            .map(|(_, _, slug)| slug)
    }

    #[must_use]
    pub fn icon(&self, slug: &str) -> Option<slint::Image> {
        let (slug, bytes) = self.icons.iter().find(|(name, _)| *name == slug)?;
        let key = (self.catalogue.assets, *slug);

        CACHE.with(|cache| {
            if let Some(cached) = cache.borrow().get(&key) {
                return cached.clone();
            }

            let image = decode(slug, bytes);
            cache.borrow_mut().insert(key, image.clone());
            image
        })
    }

    #[must_use]
    pub fn icon_for(&self, text: &str) -> Option<slint::Image> {
        self.icon(self.character_for(text)?)
    }

    #[must_use]
    pub fn icon_bytes(&self, slug: &str) -> Option<&'static [u8]> {
        self.icons
            .iter()
            .find(|(name, _)| *name == slug)
            .map(|(_, bytes)| *bytes)
    }

    // (slug, name) pairs for the icon picker
    pub fn choices(&self) -> Vec<(&'static str, String)> {
        self.icons
            .iter()
            .map(|(slug, _)| *slug)
            .filter(|slug| !self.catalogue.hidden.contains(slug))
            .map(|slug| {
                let name = self
                    .catalogue
                    .renamed
                    .iter()
                    .find(|(candidate, _)| *candidate == slug)
                    .map_or_else(|| display_name(slug), |(_, name)| (*name).to_string());
                (slug, name)
            })
            .collect()
    }
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
    use shared::classes::games::{
        GameDescriptor, InstallationFacts, capabilities::CharacterSupport, identity::GameId,
        identity::SafeRelativePath, nte::NTE,
    };
    use std::path::Path;

    fn nte() -> Characters {
        of(&*NTE).expect("NTE has characters")
    }

    #[test]
    fn every_character_icon_decodes() {
        assert!(!CHARACTER_ICONS.is_empty(), "no character icons embedded");
        for (_, icons) in CHARACTER_ICONS {
            for (slug, bytes) in *icons {
                assert!(decode(slug, bytes).is_some(), "{slug} did not decode");
            }
        }
    }

    #[test]
    fn matches_the_shapes_mods_actually_use() {
        let nte = nte();
        assert_eq!(nte.character_for("Shinku"), Some("shinku"));
        assert_eq!(nte.character_for("shinku.png"), Some("shinku"));
        assert_eq!(nte.character_for("Shinku - Summer Dress_P"), Some("shinku"));
        assert_eq!(nte.character_for("MyShinkuRetexture"), Some("shinku"));
        assert_eq!(nte.character_for("Nothing In Particular"), None);
        assert_eq!(nte.character_for(""), None);
    }

    #[test]
    fn slugs_read_back_as_names() {
        assert_eq!(display_name("shinku"), "Shinku");
        assert_eq!(display_name("mc"), "MC");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn both_zeros_draw_as_the_mc() {
        let nte = nte();
        assert_eq!(nte.character_for("Zero (F)"), Some("mc"));
        assert_eq!(nte.character_for("Zero (M)"), Some("mc"));
        assert_eq!(nte.character_for("zero"), Some("mc"));
        assert_eq!(nte.character_for("MC"), Some("mc"));
    }

    #[test]
    fn picker_hides_and_renames() {
        let choices = nte().choices();
        assert!(choices.iter().all(|(slug, _)| *slug != "zero"));
        assert!(choices.contains(&("mc", "Zero".to_string())));
    }

    #[test]
    fn every_gamebanana_character_has_an_icon() {
        let nte = nte();
        let remote = NTE
            .gamebanana()
            .expect("NTE browses GameBanana")
            .characters();
        assert!(!remote.is_empty());
        for character in remote {
            assert!(
                nte.character_for(character.name).is_some(),
                "no icon matches '{}'",
                character.name
            );
        }
    }

    struct Local(GameDescriptor);

    impl Game for Local {
        fn descriptor(&self) -> &GameDescriptor {
            &self.0
        }
        fn inspect_installation(&self, root: &Path) -> anyhow::Result<InstallationFacts> {
            Ok(InstallationFacts::new(root.to_path_buf(), "test", "test"))
        }
        fn characters(&self) -> Option<&dyn CharacterSupport> {
            Some(self)
        }
    }

    impl CharacterSupport for Local {
        fn catalogue(&self) -> &CharacterCatalogue {
            &CharacterCatalogue {
                assets: "synthetic",
                aliases: &[],
                hidden: &[],
                renamed: &[],
            }
        }
    }

    #[test]
    fn characters_work_without_remote_browsing() {
        let game: &'static Local = Box::leak(Box::new(Local(GameDescriptor {
            id: GameId::parse("synthetic").unwrap(),
            display_name: "Synthetic",
            aliases: &[],
            launchers: Vec::new(),
            markers: Vec::new(),
            processes: &[],
            game_executable: None,
            binaries: None,
            payload_dir: SafeRelativePath::parse("synthetic").unwrap(),
            payload_files: Vec::new(),
            steam: None,
            engine: None,
        })));
        assert!(game.gamebanana().is_none());

        let characters = of(game).expect("local characters are available");
        assert!(characters.choices().is_empty());
        assert_eq!(characters.character_for("Shinku"), None);
    }
}
