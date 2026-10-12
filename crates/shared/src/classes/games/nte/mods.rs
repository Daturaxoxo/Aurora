use std::path::Path;

use super::{Nte, path, paths};
use crate::classes::games::capabilities::{ModLayout, ModManagerSupport, ModNaming, ModuleSupport};

const MODS: ModNaming = ModNaming {
    extensions: &["pak", "utoc", "ucas"],
    toggle_extensions: &["pak"],
    disabled_suffix: ".disabled",
    display_suffixes: &["_P"],
};

const MODULES: ModNaming = ModNaming {
    extensions: &["asi"],
    toggle_extensions: &["asi"],
    disabled_suffix: ".disabled",
    display_suffixes: &[],
};

impl ModManagerSupport for Nte {
    fn layout(&self, game_root: &Path) -> ModLayout {
        ModLayout {
            roots: vec![path(paths::CLIENT_PAK_DIR).resolve_under(game_root)],
            naming: MODS,
        }
    }

    fn naming(&self) -> &ModNaming {
        &MODS
    }
}

impl ModuleSupport for Nte {
    fn naming(&self) -> &ModNaming {
        &MODULES
    }
}
