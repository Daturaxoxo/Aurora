use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use log::*;
use crate::classes::validate::ensure_dir;
use shared::classes::games::capabilities::LuaLayout;
pub struct LuaManager;

impl LuaManager {
    pub fn setup(layout: &LuaLayout) -> Result<()> {
        for deploy in &layout.deploy {
            let (source, destination) = (&deploy.source, &deploy.destination);
            if source.is_dir() {
                copy_dir_all(source, destination).with_context(|| {
                    format!(
                        "Failed to copy {} to {}",
                        source.display(),
                        destination.display()
                    )
                })?;
                info!("Copied {} to {}", source.display(), destination.display());
            } else if source.is_file() {
                if let Some(parent) = destination.parent() {
                    ensure_dir(&parent.to_path_buf())?;
                }
                fs::copy(source, destination).with_context(|| {
                    format!(
                        "Failed to copy {} to {}",
                        source.display(),
                        destination.display()
                    )
                })?;
                trace!("Copied {} to {}", source.display(), destination.display());
            } else {
                warn!(
                    "Expected {} to exist but it's missing; skipping",
                    source.display()
                );
            }
        }

        Ok(())
    }
}

pub(super) fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    ensure_dir(&dst.to_path_buf())?;

    for entry in fs::read_dir(src).with_context(|| format!("Failed to read {}", src.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }

    Ok(())
}
