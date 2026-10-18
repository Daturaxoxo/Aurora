use super::{
    Game,
    identity::SafeRelativePath,
    nte,
    payload::{
        LegacyRuntimeMigrationRequest, MigrationReport, PayloadResolutionMode, PayloadRoots,
        RUNTIME_DIRS, UpgradeEligibility, collect_files, is_owned, is_sidecar,
        prepare_legacy_runtime_data, resolve_payload, same_contents,
    },
};
use anyhow::{Context as _, Result};
use ipc::manifest::{LocalManifest, safe_join};
use log::*;
use std::path::{Path, PathBuf};
const BIN: &str = "Bin";
const STAGED_SUFFIX: &str = ".bak";
const STAGING_SUFFIX: &str = ".upgrading";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommittedLayout {
    LegacyManifest,
    TargetManifest,
    UnknownManifest,
    SyncedTarget,
    UnsyncedTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRuntimeData {
    Empty,
    CopiesOfLegacy,
    Diverged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpgradeEvidence {
    pub post_update: bool,
    pub committed: CommittedLayout,
    pub legacy_payload: bool,
    pub target_data: TargetRuntimeData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeDecision {
    Migrate(UpgradeEligibility),
    Normal,
}

pub fn decide(evidence: &UpgradeEvidence) -> UpgradeDecision {
    let kind = match evidence.target_data {
        TargetRuntimeData::Empty => UpgradeEligibility::FirstUpgrade,
        TargetRuntimeData::CopiesOfLegacy | TargetRuntimeData::Diverged => {
            UpgradeEligibility::InterruptedRetry
        }
    };
    let undiverged = evidence.target_data != TargetRuntimeData::Diverged;
    let migrate = match evidence.committed {
        CommittedLayout::LegacyManifest => evidence.post_update,
        CommittedLayout::UnknownManifest => {
            evidence.post_update && evidence.legacy_payload && undiverged
        }
        CommittedLayout::UnsyncedTarget => evidence.legacy_payload,
        CommittedLayout::TargetManifest | CommittedLayout::SyncedTarget => false,
    };
    if migrate {
        UpgradeDecision::Migrate(kind)
    } else {
        UpgradeDecision::Normal
    }
}

pub fn migrate_legacy_runtime_data(
    game: &dyn Game,
    bin: &Path,
    committed: CommittedLayout,
    post_update: bool,
) -> Result<()> {
    let id = &game.descriptor().id;
    let roots = PayloadRoots::new(bin.to_path_buf());
    let owned = &game.descriptor().payload_files;
    let evidence = UpgradeEvidence {
        post_update,
        committed,
        legacy_payload: legacy_payload(bin),
        target_data: target_runtime_data(bin, &roots.target(game), owned)
            .context("could not inspect the target runtime data")?,
    };

    let UpgradeDecision::Migrate(eligibility) = decide(&evidence) else {
        debug!("{id} needs no legacy migration ({evidence:?})");
        return Ok(());
    };
    info!("Moving the legacy {id} runtime data into the target payload ({eligibility:?})");

    let target = resolve_payload(game, &roots, PayloadResolutionMode::TargetOnly)?;
    let request =
        LegacyRuntimeMigrationRequest::new(bin.to_path_buf(), target, owned.clone(), eligibility)?;
    let report = prepare_legacy_runtime_data(&request)
        .with_context(|| format!("could not move the legacy {id} runtime data"))?;

    info!(
        // format: "copy, no need to copy (already there), item owned, skipped"
        "{id} legacy migration: {} copied, {} already present, {} owned and {} skipped",
        report.copied.len(),
        report.identical.len(),
        report.skipped_owned.len(),
        report.skipped_sidecars.len()
    );
    for path in &report.skipped_other {
        warn!("{id} legacy migration skipped {}", path.display());
    }
    Ok(())
}

fn is_flat_payload_path(path: &str, games: &[&dyn Game]) -> bool {
    let mut components = path.split(['/', '\\']);
    let (Some(first), Some(second)) = (components.next(), components.next()) else {
        return false;
    };
    first.eq_ignore_ascii_case(BIN)
        && !games.iter().any(|game| {
            game.descriptor()
                .payload_dir
                .components()
                .first()
                .is_some_and(|dir| dir.eq_ignore_ascii_case(second))
        })
}

fn is_present(install_root: &Path, path: &str) -> bool {
    safe_join(install_root, path).is_some_and(|file| {
        let mut staged = file.clone().into_os_string();
        staged.push(STAGED_SUFFIX);
        file.symlink_metadata().is_ok() || Path::new(&staged).symlink_metadata().is_ok()
    })
}

pub fn manifest_layout(install_root: &Path, games: &[&dyn Game]) -> CommittedLayout {
    match LocalManifest::load(install_root) {
        Ok(Some(manifest)) => {
            let owns_legacy = manifest
                .files
                .keys()
                .any(|path| is_flat_payload_path(path, games) && is_present(install_root, path));
            if owns_legacy {
                CommittedLayout::LegacyManifest
            } else {
                CommittedLayout::TargetManifest
            }
        }
        Ok(None) | Err(_) => CommittedLayout::UnknownManifest,
    }
}

pub fn synced_layout(target_root: &Path) -> CommittedLayout {
    if target_root.symlink_metadata().is_ok_and(|m| m.is_dir()) {
        CommittedLayout::SyncedTarget
    } else {
        CommittedLayout::UnsyncedTarget
    }
}

pub fn staging_root(target_root: &Path) -> PathBuf {
    let mut name = target_root.as_os_str().to_os_string();
    name.push(STAGING_SUFFIX);
    PathBuf::from(name)
}

pub fn legacy_payload(bin: &Path) -> bool {
    nte::LEGACY_BIN_SIGNATURE
        .iter()
        .any(|name| bin.join(name).symlink_metadata().is_ok_and(|m| m.is_file()))
}

pub fn target_runtime_data(
    legacy_root: &Path,
    target_root: &Path,
    owned: &[SafeRelativePath],
) -> Result<TargetRuntimeData> {
    let mut report = MigrationReport::default();
    let mut found = false;

    for dir in RUNTIME_DIRS {
        for rel in collect_files(target_root, dir, &mut report)? {
            if is_owned(owned, &rel) || is_sidecar(rel.file_name()) {
                continue;
            }
            found = true;
            let legacy = rel.resolve_under(legacy_root);
            let copied = legacy.symlink_metadata().is_ok_and(|m| m.is_file())
                && same_contents(&legacy, &rel.resolve_under(target_root))?;
            if !copied {
                return Ok(TargetRuntimeData::Diverged);
            }
        }
    }

    Ok(if found {
        TargetRuntimeData::CopiesOfLegacy
    } else {
        TargetRuntimeData::Empty
    })
}
