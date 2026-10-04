use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::{Result, anyhow};

use super::{
    EngineKind, Game, GameDescriptor, InstallationFacts, LauncherIdentifier, SteamMetadata,
    identity::{GameId, SafeRelativePath},
};

pub const PAYLOAD_FILES: &[&str] = &[
    "core.dll",
    "Plugins/check.asi",
    "Wrappers/wrap.dll",
    "Addons/Shipped/shipped.auadd",
];

pub struct Fixture {
    descriptor: GameDescriptor,
}

fn path(raw: &str) -> SafeRelativePath {
    SafeRelativePath::parse(raw).unwrap()
}

impl Fixture {
    pub fn new() -> Self {
        Self {
            descriptor: GameDescriptor {
                id: GameId::parse("fixture").unwrap(),
                display_name: "Fixture Game",
                aliases: &["Fixture Game", "FixtureGame"],
                launchers: vec![LauncherIdentifier {
                    file: path("Fixture.exe"),
                    variant: "fixture",
                }],
                markers: vec![path("Content/Paks")],
                processes: &["fixture.exe"],
                game_executable: path("Fixture.exe"),
                binaries: path("Binaries/Win64"),
                payload_dir: path("fixture"),
                payload_files: PAYLOAD_FILES.iter().copied().map(path).collect(),
                steam: Some(SteamMetadata { app_id: "123" }),
                engine: EngineKind::Pak,
            },
        }
    }
}

impl Game for Fixture {
    fn descriptor(&self) -> &GameDescriptor {
        &self.descriptor
    }

    fn inspect_installation(&self, root: &Path) -> Result<InstallationFacts> {
        if root.join("Fixture.exe").is_file() {
            Ok(InstallationFacts::new(
                root.to_path_buf(),
                "fixture",
                "standalone",
            ))
        } else {
            Err(anyhow!("{} has no launcher", root.display()))
        }
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
pub struct TestDir(PathBuf);

impl TestDir {
    pub fn new(label: &str) -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("aurora-games-{label}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}