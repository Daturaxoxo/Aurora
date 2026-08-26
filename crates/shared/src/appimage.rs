use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use log::*;

fn bundled_bin() -> Option<PathBuf> {
    ipc::appimage_mount().map(|dir| dir.join("usr/lib/Aurora/Bin"))
}

pub fn sync_bin() -> Result<()> {
    let Some(src_root) = bundled_bin() else {
        return Ok(());
    };
    if !src_root.is_dir() {
        return Err(anyhow!(
            "the AppImage has no bundled Bin at {}",
            src_root.display()
        ));
    }

    let dst_root = ipc::state_root().join("Bin");
    let mut copied = 0usize;

    for entry in crate::utils::read_dir_recursive(&src_root) {
        let src = entry.path();
        if !src.is_file() {
            continue;
        }
        let rel = src
            .strip_prefix(&src_root)
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
