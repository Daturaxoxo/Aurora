use crate::utils::get_cache_dir;

use super::types::NteMod;
use bincode::config::standard;
use bincode::serde::{decode_from_slice, encode_to_vec};
use log::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tokio::fs;

const CACHE_TTL_SECONDS: u64 = 3600;
// 128 MB
const CACHE_MAX_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Serialize)]
struct CacheWrapper<'a> {
    cached_at: u64,
    page: Option<u32>,
    query: Option<String>,
    mods: Vec<&'a NteMod>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct DecodedCache {
    cached_at: u64,
    page: Option<u32>,
    query: Option<String>,
    mods: Vec<NteMod>,
}

pub struct CacheManager {
    base_dir: PathBuf,
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheManager {
    pub fn new() -> Self {
        let base_dir = get_cache_dir().join("GameBanana");
        std::fs::create_dir_all(&base_dir).ok();
        Self::prune(&base_dir);
        Self { base_dir }
    }

    fn current_timestamp() -> u64 {
        chrono::Utc::now().timestamp().cast_unsigned()
    }

    fn file_age_seconds(path: &Path) -> Option<u64> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        let stamp = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
        Some(Self::current_timestamp().abs_diff(stamp))
    }

    fn prune(base_dir: &Path) {
        let Ok(entries) = std::fs::read_dir(base_dir) else {
            return;
        };

        let mut kept: Vec<(u64, u64, PathBuf)> = Vec::new();
        let mut total: u64 = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file()
                || path
                    .extension()
                    .is_none_or(|ext| ext != "json" && ext != "bin")
            {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or_default();
            let Some(age) = Self::file_age_seconds(&path) else {
                continue;
            };
            if age >= CACHE_TTL_SECONDS {
                if let Err(e) = std::fs::remove_file(&path) {
                    warn!(
                        "Could not remove expired GameBanana cache '{}': {e}",
                        path.display()
                    );
                }
                continue;
            }
            total = total.saturating_add(size);
            kept.push((age, size, path));
        }

        if total <= CACHE_MAX_BYTES {
            return;
        }

        kept.sort_by_key(|entry| std::cmp::Reverse(entry.0));
        for (_, size, path) in kept {
            if total <= CACHE_MAX_BYTES {
                break;
            }
            if let Err(e) = std::fs::remove_file(&path) {
                warn!(
                    "Could not remove oversized GameBanana cache '{}': {e}",
                    path.display()
                );
                continue;
            }
            total = total.saturating_sub(size);
        }
    }

    fn cache_path(&self, prefix: &str, key: &str) -> PathBuf {
        self.base_dir
            .join(format!("{prefix}_{:x}.bin", md5::compute(key.as_bytes())))
    }

    fn feed_path(&self, page: u32) -> PathBuf {
        self.cache_path("page", &page.to_string())
    }

    fn search_path(&self, query: &str, page: u32) -> PathBuf {
        self.cache_path("search", &format!("{query}\u{1f}{page}"))
    }

    fn category_path(&self, category_id: u32, page: u32) -> PathBuf {
        self.cache_path("cat", &format!("{category_id}\u{1f}{page}"))
    }

    pub async fn get_feed_cache(&self, page: u32) -> Option<Vec<NteMod>> {
        let path = self.feed_path(page);
        self.load_cache(&path).await
    }

    pub async fn save_feed_cache(&self, page: u32, mods: &[Arc<NteMod>]) {
        let path = self.feed_path(page);
        self.save_cache(
            &path,
            CacheWrapper {
                cached_at: Self::current_timestamp(),
                page: Some(page),
                query: None,
                mods: mods.iter().map(std::convert::AsRef::as_ref).collect(),
            },
        )
        .await;
    }

    pub async fn get_search_cache(&self, query: &str, page: u32) -> Option<Vec<NteMod>> {
        let path = self.search_path(query, page);
        self.load_cache(&path).await
    }

    pub async fn save_search_cache(&self, query: &str, page: u32, mods: &[Arc<NteMod>]) {
        let path = self.search_path(query, page);
        self.save_cache(
            &path,
            CacheWrapper {
                cached_at: Self::current_timestamp(),
                page: Some(page),
                query: Some(query.to_string()),
                mods: mods.iter().map(std::convert::AsRef::as_ref).collect(),
            },
        )
        .await;
    }

    pub async fn get_category_cache(&self, category_id: u32, page: u32) -> Option<Vec<NteMod>> {
        let path = self.category_path(category_id, page);
        self.load_cache(&path).await
    }

    pub async fn save_category_cache(&self, category_id: u32, page: u32, mods: &[Arc<NteMod>]) {
        let path = self.category_path(category_id, page);
        self.save_cache(
            &path,
            CacheWrapper {
                cached_at: Self::current_timestamp(),
                page: Some(page),
                query: None,
                mods: mods.iter().map(std::convert::AsRef::as_ref).collect(),
            },
        )
        .await;
    }

    pub async fn clear(&self) -> std::io::Result<()> {
        if !self.base_dir.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&self.base_dir).await?;
        fs::create_dir_all(&self.base_dir).await
    }

    async fn load_cache(&self, path: &Path) -> Option<Vec<NteMod>> {
        let data = match fs::read(path).await {
            Ok(data) => data,
            // Nothing has been cached yet
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
            Err(e) => {
                warn!("Could not read the GameBanana cache '{}': {e}", path.display());
                return None;
            }
        };
        
        let (wrapper, _) =
            decode_from_slice::<DecodedCache, _>(&data, standard()).ok()?;
        (Self::current_timestamp().abs_diff(wrapper.cached_at) < CACHE_TTL_SECONDS)
            .then_some(wrapper.mods)
    }

    async fn save_cache(&self, path: &Path, wrapper: CacheWrapper<'_>) {
        if let Ok(bytes) = encode_to_vec(&wrapper, standard()) {
            let _ = fs::write(path, bytes).await;
        }
    }
}
