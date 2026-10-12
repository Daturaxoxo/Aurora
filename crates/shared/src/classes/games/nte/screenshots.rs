use super::{Nte, paths::PICTURE_FOLDER};
use crate::classes::games::{
    InstallationFacts,
    capabilities::{
        ScreenshotMutation, ScreenshotNesting, ScreenshotSource, ScreenshotSourceKind,
        ScreenshotSupport,
    },
    platform::PlatformPaths,
};

const SELFIE: &[&str] = &["Client", "WindowsNoEditor", "Selfie"];
const IGNORED_PLAYERS: &[&str] = &["66666"];

impl ScreenshotSupport for Nte {
    fn sources(
        &self,
        installation: Option<&InstallationFacts>,
        platform: &PlatformPaths,
    ) -> Vec<ScreenshotSource> {
        let selfie = installation.map(|facts| ScreenshotSource {
            kind: ScreenshotSourceKind::Game,
            dir: SELFIE
                .iter()
                .fold(facts.root().to_path_buf(), |dir, part| dir.join(part)),
            nesting: ScreenshotNesting::PerPlayer {
                subdir: "ScreenShots",
            },
        });
        let pictures = platform.pictures.as_ref().map(|base| ScreenshotSource {
            kind: ScreenshotSourceKind::Pictures,
            dir: base.join(PICTURE_FOLDER),
            nesting: ScreenshotNesting::Flat,
        });

        selfie.into_iter().chain(pictures).collect()
    }

    fn ignored_directories(&self) -> &[&'static str] {
        IGNORED_PLAYERS
    }

    fn permits(&self, mutation: ScreenshotMutation) -> bool {
        matches!(
            mutation,
            ScreenshotMutation::Delete | ScreenshotMutation::Rename
        )
    }
}
