use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use anyhow::{Context, Result, anyhow};
use ipc::manifest::{FileEntry, LocalManifest, Manifest, hash_bytes, safe_join};
use log::*;

#[derive(Debug, Default)]
pub struct RepairReport {
    pub restored: Vec<String>,
    pub failed: Vec<String>,
}

impl RepairReport {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.restored.is_empty() && self.failed.is_empty()
    }
}

/// Set once, when the manifest could not be fetched
static MANIFEST_UNREACHABLE: AtomicBool = AtomicBool::new(false);

/// Paths we have already tried to restore this session
static ATTEMPTED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

fn first_attempt(relative: &str) -> bool {
    ATTEMPTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(relative.to_owned())
}

fn install_looks_intact(root: &Path) -> bool {
    match LocalManifest::load(root) {
        Ok(Some(local)) => local
            .files
            .keys()
            .all(|relative| safe_join(root, relative).is_some_and(|path| path.exists())),
        Ok(None) => {
            debug!("Repair: no local manifest, falling back to the remote one");
            false
        }
        Err(e) => {
            warn!("Repair: local manifest unreadable ({e}), using the remote one");
            false
        }
    }
}

fn fetch_manifest() -> Result<Manifest> {
    let mut last_err = anyhow!("no manifest sources configured");

    for url in ipc::manifest_urls() {
        let fetched = crate::api::download_bytes(url)
            .and_then(|bytes| {
                serde_json::from_slice::<Manifest>(&bytes)
                    .with_context(|| format!("{url} returned a manifest that could not be parsed"))
            })
            .and_then(|manifest| {
                manifest
                    .validate()
                    .and_then(|()| manifest.validate_urls())
                    .map_err(|e| anyhow!("manifest from {url} rejected: {e}"))?;
                Ok(manifest)
            });

        match fetched {
            Ok(manifest) => return Ok(manifest),
            Err(e) => {
                warn!("Repair: manifest fetch failed from {url}: {e:#}");
                last_err = e;
            }
        }
    }

    Err(last_err.context("all manifest sources failed"))
}

fn restore(entry: &FileEntry, dest: &Path) -> Result<()> {
    let mut last_err = anyhow!("no download sources available");

    for url in entry.download_urls() {
        let downloaded = crate::api::download_bytes(&url).and_then(|bytes| {
            let actual = hash_bytes(&bytes);
            if actual == entry.sha256 {
                Ok(bytes)
            } else {
                Err(anyhow!(
                    "hash mismatch for {}: expected {}, got {actual}",
                    entry.path,
                    entry.sha256
                ))
            }
        });

        match downloaded {
            Ok(bytes) => return crate::api::write_atomic(dest, &bytes),
            Err(e) => {
                warn!("Repair: download failed from {url}: {e:#}");
                last_err = e;
            }
        }
    }

    Err(last_err.context("all download sources failed"))
}

pub fn restore_missing_files() -> RepairReport {
    restore_missing_files_in(&ipc::instance_root())
}

pub fn restore_missing_files_in(root: &Path) -> RepairReport {
    let mut report = RepairReport::default();

    if install_looks_intact(root) {
        trace!("Repair: install is intact");
        return report;
    }

    if MANIFEST_UNREACHABLE.load(Ordering::Relaxed) {
        debug!("Repair: the manifest was already unreachable this session, skipping");
        return report;
    }

    let manifest = match fetch_manifest() {
        Ok(manifest) => manifest,
        Err(e) => {
            warn!("Repair: could not fetch the manifest, skipping this session: {e:#}");
            MANIFEST_UNREACHABLE.store(true, Ordering::Relaxed);
            return report;
        }
    };

    let mut local = LocalManifest::load(root).ok().flatten();
    let mut local_dirty = false;

    if let Some(local) = &local
        && local.version != manifest.version
    {
        info!(
            "Repair: install is {} but the manifest is {}; restoring the manifest's copies",
            local.version, manifest.version
        );
    }

    for entry in &manifest.files {
        let Some(dest) = entry.resolve(root) else {
            warn!("Repair: skipping rejected manifest entry '{}'", entry.path);
            continue;
        };
        if dest.exists() || !first_attempt(&entry.path) {
            continue;
        }

        info!("Repair: '{}' is missing, restoring it", entry.path);
        match restore(entry, &dest) {
            Ok(()) => {
                info!("Repair: restored '{}'", entry.path);
                report.restored.push(entry.path.clone());
                if let Some(local) = local.as_mut() {
                    local.files.insert(entry.path.clone(), entry.sha256.clone());
                    local_dirty = true;
                }
            }
            Err(e) => {
                error!("Repair: could not restore '{}': {e:#}", entry.path);
                report.failed.push(entry.path.clone());
            }
        }
    }

    if local_dirty
        && let Some(local) = &local
        && let Err(e) = local.save(root)
    {
        warn!("Repair: could not update the local manifest: {e}");
    }

    report
}
