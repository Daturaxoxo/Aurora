use super::Nte;
use crate::classes::games::capabilities::{CharacterCatalogue, CharacterSupport};

const CATALOGUE: CharacterCatalogue = CharacterCatalogue {
    assets: "nte",
    aliases: &[("zero", "mc")],
    hidden: &["zero"],
    renamed: &[("mc", "Zero")],
};

impl CharacterSupport for Nte {
    fn catalogue(&self) -> &CharacterCatalogue {
        &CATALOGUE
    }
}
