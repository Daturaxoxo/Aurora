use super::{
    Game, GameDescriptor, InstallationFacts, identity::GameId, identity::SafeRelativePath,
};
use anyhow::{Result, anyhow};
use std::{path::Path, sync::LazyLock};
pub static SP: LazyLock<SilverPalace> = LazyLock::new(SilverPalace::new);

pub struct SilverPalace {
    descriptor: GameDescriptor,
}

impl SilverPalace {
    fn new() -> Self {
        Self {
            descriptor: GameDescriptor {
                id: GameId::parse("sp").expect("Silver Palace id is valid"),
                display_name: "Silver Palace",
                aliases: &["Silver Palace"], // TODO: confirm the folder name once it releases -daturas
                launchers: Vec::new(),
                markers: Vec::new(),
                processes: &[],
                game_executable: None,
                binaries: None,
                payload_dir: SafeRelativePath::parse("sp")
                    .expect("Silver Palace payload dir is valid"),
                payload_files: Vec::new(),
                steam: None,
                engine: None,
            },
        }
    }
}

impl Game for SilverPalace {
    fn descriptor(&self) -> &GameDescriptor {
        &self.descriptor
    }

    fn inspect_installation(&self, root: &Path) -> Result<InstallationFacts> {
        Err(anyhow!(
            "{} is not supported yet, {} cannot be used",
            self.descriptor.display_name,
            root.display()
        ))
    }
}
