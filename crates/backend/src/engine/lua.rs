use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::*;

use crate::classes::validate::ensure_dir;

/// Handles installing the UE4SS Lua scripting runtime — which Aurora's Lua
/// Scripts page (see the main crate's `lua.rs`) downloads into
/// `<bin_dir>\Lua` — into the game's actual `Win64` folder so UE4SS loads
/// when the game launches.
///
/// Disk layout (mirrors the main crate's `lua.rs`):
///   `<bin_dir>\Lua\dwmapi.dll`   <- UE4SS's proxy-DLL entry point
///   `<bin_dir>\Lua\ue4ss\...`    <- UE4SS runtime + Mods folder
///
/// Unreal loads `dwmapi.dll` via DLL search-order hijacking from the same
/// directory as the game's executable, so both `dwmapi.dll` and the
/// `ue4ss` folder need to actually live in `Win64` — not just in
/// `<bin_dir>\Lua` — for UE4SS to take effect.
pub struct LuaManager;

impl LuaManager {
    /// Mirrors the existence check in the main crate's `lua.rs`
    /// (`LuaScriptsHandler::setup`): if `<bin_dir>\Lua\ue4ss\UE4SS.dll`
    /// isn't there, UE4SS was never installed via the Lua Scripts page and
    /// there's nothing to inject.
    pub fn exists(bin_path: &Path) -> bool {
        bin_path
            .join("Lua")
            .join("ue4ss")
            .join("UE4SS.dll")
            .exists()
    }

    /// Copies `<bin_dir>\Lua`'s contents (`dwmapi.dll` and the `ue4ss`
    /// folder) into `win64_path`, overwriting anything already there.
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

/// Recursively copies `src` into `dst`, creating directories as needed and
/// overwriting any existing files. `std::fs` has no built-in directory
/// copy, so this walks the tree by hand.
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
        // Symlinks are skipped — not expected inside a UE4SS distribution.
    }

    Ok(())
}