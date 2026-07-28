use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::*;

use crate::classes::validate::ensure_dir;

pub struct LuaManager;

impl LuaManager {
    pub fn exists(bin_path: &Path) -> bool {
        let marker = bin_path
            .join("Lua")
            .join("ue4ss")
            .join("UE4SS.dll");
        let found = marker.exists();
        info!("LuaManager::exists checking {} -> {found}", marker.display());
        found
    }

    pub fn setup(bin_path: &Path, win64_path: PathBuf) -> Result<()> {
        let lua_dir = bin_path.join("Lua");

        ensure_dir(&win64_path)?;

        let dwmapi_src = lua_dir.join("dwmapi.dll");
        let dwmapi_dst = win64_path.join("dwmapi.dll");
        if dwmapi_src.exists() {
            fs::copy(&dwmapi_src, &dwmapi_dst).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    dwmapi_src.display(),
                    dwmapi_dst.display()
                )
            })?;
            trace!(
                "Copied {} to {}",
                dwmapi_src.display(),
                dwmapi_dst.display()
            );
        } else {
            warn!(
                "Expected {} to exist but it's missing; skipping",
                dwmapi_src.display()
            );
        }

        let ue4ss_src = lua_dir.join("ue4ss");
        let ue4ss_dst = win64_path.join("ue4ss");
        if ue4ss_src.exists() {
            copy_dir_all(&ue4ss_src, &ue4ss_dst).with_context(|| {
                format!(
                    "Failed to copy {} to {}",
                    ue4ss_src.display(),
                    ue4ss_dst.display()
                )
            })?;
            info!(
                "Copied {} to {}",
                ue4ss_src.display(),
                ue4ss_dst.display()
            );
        } else {
            warn!(
                "Expected {} to exist but it's missing; skipping",
                ue4ss_src.display()
            );
        }

        Ok(())
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
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