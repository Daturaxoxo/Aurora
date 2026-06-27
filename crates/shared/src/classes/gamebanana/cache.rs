use super::types::NteMod;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use tokio::fs;

const CACHE_TTL_SECONDS: u64 = 3600;

#[derive(Serialize, Deserialize)]
struct CacheWrapper {
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
        let base_dir = env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_else(|| env::current_dir().unwrap_or_default());
        std::fs::create_dir_all(base_dir.join("Cache/GameBanana")).ok();
        Self { base_dir }
    }

    fn current_timestamp() -> u64 {
        chrono::Utc::now().timestamp().cast_unsigned()
    }

    fn is_valid(&self, cache_file: &PathBuf) -> bool {
        if !cache_file.exists() {
            return false;
        }
        if let Ok(data) = std::fs::read_to_string(cache_file) {
            if let Ok(wrapper) = serde_json::from_str::<CacheWrapper>(&data) {
                return Self::current_timestamp() - wrapper.cached_at < CACHE_TTL_SECONDS;
            }
        }
        false
    }

    pub async fn get_feed_cache(&self, page: u32) -> Option<Vec<NteMod>> {
        let path = self.base_dir.join(format!("page_{page}.json"));
        self.load_cache(&path).await
    }

    pub async fn save_feed_cache(&self, page: u32, mods: Vec<NteMod>) {
        let path = self.base_dir.join(format!("page_{page}.json"));
        let wrapper = CacheWrapper {
            cached_at: Self::current_timestamp(),
            page: Some(page),
            query: None,
            mods,
        };
        self.save_cache(&path, wrapper).await;
    }

    pub async fn get_search_cache(&self, query: &str, page: u32) -> Option<Vec<NteMod>> {
        let safe_query = query.replace(|c: char| !c.is_alphanumeric(), "_");
        let path = self
            .base_dir
            .join(format!("search_{safe_query}_p{page}.json"));
        self.load_cache(&path).await
    }

    pub async fn save_search_cache(&self, query: &str, page: u32, mods: Vec<NteMod>) {
        let safe_query = query.replace(|c: char| !c.is_alphanumeric(), "_");
        let path = self
            .base_dir
            .join(format!("search_{safe_query}_p{page}.json"));
        let wrapper = CacheWrapper {
            cached_at: Self::current_timestamp(),
            page: Some(page),
            query: Some(query.to_string()),
            mods,
        };
        self.save_cache(&path, wrapper).await;
    }

    async fn load_cache(&self, path: &PathBuf) -> Option<Vec<NteMod>> {
        if self.is_valid(path) {
            let data = fs::read_to_string(path).await.ok()?;
            let wrapper: CacheWrapper = serde_json::from_str(&data).ok()?;
            return Some(wrapper.mods);
        }
        None
    }

    async fn save_cache(&self, path: &PathBuf, wrapper: CacheWrapper) {
        if let Ok(json) = serde_json::to_string(&wrapper) {
            let _ = fs::write(path, json).await;
        }
    }
}
