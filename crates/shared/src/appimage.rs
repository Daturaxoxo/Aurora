use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use log::*;

use crate::classes::games::{
    Game,
    identity::GameId,
    payload::{PayloadLayoutKind, PayloadRoots, ResolvedPayloadLayout},
    registry,
    upgrade::{CommittedLayout, synced_layout},
};

// TODO(lane B, b7-3): a failed Linux migration must also count as a failed target. Either run the
// migration before REPORT is set, or give GameSync a migration error that target_failed() checks.
static REPORT: OnceLock<SyncReport> = OnceLock::new();

#[derive(Debug)]
pub struct GameSync {
    pub id: GameId,
    pub prior: CommittedLayout,
    pub target_error: Option<String>,
}

#[derive(Debug, Default)]
pub struct SyncReport {
    pub games: Vec<GameSync>,
}

impl SyncReport {
    fn game(&self, id: &GameId) -> Option<&GameSync> {
        self.games.iter().find(|game| game.id == *id)
    }

    pub fn prior(&self, id: &GameId) -> Option<CommittedLayout> {
        self.game(id).map(|game| game.prior)
    }

    pub fn target_failed(&self, id: &GameId) -> bool {
        self.game(id).is_some_and(|game| game.target_error.is_some())
    }
}

pub fn report() -> Option<&'static SyncReport> {
    REPORT.get()
}

fn bundled_bin() -> Option<PathBuf> {
    ipc::appimage_mount().map(|dir| dir.join("usr/lib/Aurora/Bin"))
}

fn payload_games() -> Vec<&'static dyn Game> {
    registry()
        .iter()
        .copied()
        .filter(|game| game.descriptor().engine.is_some())
        .collect()
}

pub fn sync_bin() {
    let Some(src_root) = bundled_bin() else {
        return;
    };
    let report = sync_into(&src_root, &ipc::state_root().join("Bin"), &payload_games());
    let _ = REPORT.set(report);
}

fn sync_into(src_root: &Path, dst_root: &Path, games: &[&dyn Game]) -> SyncReport {
    let roots = PayloadRoots::new(dst_root.to_path_buf());
    let prior: Vec<CommittedLayout> = games
        .iter()
        .map(|game| synced_layout(&roots.target(*game)))
        .collect();

    let synced = copy_tree(src_root, dst_root);
    if let Err(e) = &synced {
        error!("Could not sync the bundled Bin payload: {e:#}");
    }

    let games = games
        .iter()
        .zip(prior)
        .map(|(game, prior)| {
            let target_error = match &synced {
                Err(e) => Some(format!("{e:#}")),
                Ok(()) => ResolvedPayloadLayout::inspect(
                    *game,
                    &roots.target(*game),
                    PayloadLayoutKind::Target,
                )
                .err()
                .map(|e| format!("{e:#}")),
            };
            let id = game.descriptor().id.clone();
            if let Some(e) = &target_error {
                warn!("{id} target payload is unusable, only a complete flat layout may be used: {e}");
            }
            GameSync {
                id,
                prior,
                target_error,
            }
        })
        .collect();

    SyncReport { games }
}

fn copy_tree(src_root: &Path, dst_root: &Path) -> Result<()> {
    if !src_root.is_dir() {
        return Err(anyhow!(
            "the AppImage has no bundled Bin at {}",
            src_root.display()
        ));
    }

    let mut copied = 0usize;

    for entry in crate::utils::read_dir_recursive(&src_root.to_path_buf()) {
        let src = entry.path();
        if !src.is_file() {
            continue;
        }
        let rel = src
            .strip_prefix(src_root)
            .map_err(|e| anyhow!("{} is not under the bundled Bin: {e}", src.display()))?;
        let dst = dst_root.join(rel);

        if !needs_copy(&src, &dst) {
            continue;
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(&src, &dst)?;
        copied += 1;
    }

    if copied > 0 {
        info!(
            "Synced {copied} bundled file(s) into {}",
            dst_root.display()
        );
    }
    Ok(())
}

fn needs_copy(src: &Path, dst: &Path) -> bool {
    let (Ok(src_meta), Ok(dst_meta)) = (src.metadata(), dst.metadata()) else {
        return true;
    };
    if src_meta.len() != dst_meta.len() {
        return true;
    }
    match (ipc::manifest::hash_file(src), ipc::manifest::hash_file(dst)) {
        (Ok(a), Ok(b)) => a != b,
        _ => true,
    }
}