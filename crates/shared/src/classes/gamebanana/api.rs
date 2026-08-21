use super::cache::CacheManager;
use super::types::{ApiRecord, NteMod, NteModFile, ProfilePage, SearchResponse, SubfeedResponse};
use crate::utils::{error_chain, get_local_version};
use anyhow::{anyhow, Context, Result};
use futures::{stream, StreamExt};
use reqwest::{Client, RequestBuilder, Response};
use tokio::sync::mpsc::UnboundedSender;

use std::time::Duration;

use log::*;

const BASE_URL: &str = "https://gamebanana.com";
const NTE_GAME_ID: u32 = 23012;
const FETCH_CONCURRENCY: usize = 6;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const POOL_MAX_IDLE_PER_HOST: usize = 8;
const TCP_KEEPALIVE: Duration = Duration::from_secs(30);
const MAX_ATTEMPTS: u32 = 3;
const RETRY_BACKOFF: Duration = Duration::from_millis(500);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(10);

enum Attempt {
    Done(Vec<u8>),
    Retry(anyhow::Error, Option<Duration>),
    Fatal(anyhow::Error),
}

pub struct GameBananaApi {
    client: Client,
    cache: CacheManager,
}

impl Default for GameBananaApi {
    fn default() -> Self {
        Self::new()
    }
}

impl GameBananaApi {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(format!("AuroraLauncher/{}", get_local_version()))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            .pool_max_idle_per_host(POOL_MAX_IDLE_PER_HOST)
            .tcp_keepalive(TCP_KEEPALIVE)
            .build()
            .unwrap_or_else(|e| {
                error!("Failed to build GameBanana client, falling back to default: {e}");
                Client::default()
            });

        Self {
            client,
            cache: CacheManager::new(),
        }
    }

    pub async fn clear_cache(&self) -> std::io::Result<()> {
        self.cache.clear().await
    }

    fn is_transient(error: &reqwest::Error) -> bool {
        error.is_timeout() || error.is_connect() || error.is_request() || error.is_body()
    }

    fn retry_after(resp: &Response) -> Option<Duration> {
        let seconds: u64 = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)?
            .to_str()
            .ok()?
            .trim()
            .parse()
            .ok()?;

        Some(Duration::from_secs(seconds).min(MAX_RETRY_AFTER))
    }

    fn backoff(attempt: u32, suggested: Option<Duration>) -> Duration {
        if let Some(after) = suggested {
            return after;
        }

        let base = RETRY_BACKOFF.saturating_mul(1 << (attempt - 1).min(4));
        let spread = u64::try_from(base.as_millis()).unwrap_or(u64::MAX) / 2;

        base + Duration::from_millis(Self::jitter(spread))
    }

    fn jitter(max_ms: u64) -> u64 {
        use std::collections::hash_map::RandomState;
        use std::hash::{BuildHasher, Hasher};

        if max_ms == 0 {
            return 0;
        }
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(max_ms);
        hasher.finish() % max_ms
    }

    async fn run_once(request: RequestBuilder, what: &str) -> Attempt {
        let resp = match request.send().await {
            Ok(resp) => resp,
            Err(e) => {
                let msg = format!("could not reach {what}: {}", error_chain(&e));
                return if Self::is_transient(&e) {
                    Attempt::Retry(anyhow!(msg), None)
                } else {
                    Attempt::Fatal(anyhow!(msg))
                };
            }
        };

        let status = resp.status();

        if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let after = Self::retry_after(&resp);
            return Attempt::Retry(anyhow!("{what} returned HTTP {status}"), after);
        }

        if !status.is_success() {
            return Attempt::Fatal(anyhow!("{what} returned HTTP {status}"));
        }

        match resp.bytes().await {
            Err(e) => Attempt::Retry(
                anyhow!("could not read {what}: {}", error_chain(&e)),
                None,
            ),
            Ok(bytes) if bytes.is_empty() => {
                Attempt::Retry(anyhow!("{what} returned an empty body"), None)
            }
            Ok(bytes) => Attempt::Done(bytes.to_vec()),
        }
    }

    async fn fetch_bytes(request: RequestBuilder, what: &str) -> Result<Vec<u8>> {
        let mut attempt = 1u32;
        loop {
            let Some(replay) = request.try_clone() else {
                return match Self::run_once(request, what).await {
                    Attempt::Done(bytes) => Ok(bytes),
                    Attempt::Retry(e, _) | Attempt::Fatal(e) => Err(e),
                };
            };

            match Self::run_once(replay, what).await {
                Attempt::Done(bytes) => return Ok(bytes),
                Attempt::Fatal(e) => return Err(e),
                Attempt::Retry(e, after) => {
                    if attempt >= MAX_ATTEMPTS {
                        return Err(e.context(format!(
                            "{what} still failing after {MAX_ATTEMPTS} attempts"
                        )));
                    }

                    let wait = Self::backoff(attempt, after);
                    warn!(
                        "{what} failed, retrying in {}ms ({attempt}/{MAX_ATTEMPTS}): {e:#}",
                        wait.as_millis()
                    );
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
            }
        }
    }

    async fn fetch_json<T: serde::de::DeserializeOwned>(
        request: RequestBuilder,
        what: &str,
    ) -> Result<T> {
        let bytes = Self::fetch_bytes(request, what).await?;

        serde_json::from_slice(&bytes).with_context(|| {
            format!(
                "could not parse {what} ({} bytes of {})",
                bytes.len(),
                String::from_utf8_lossy(&bytes[..bytes.len().min(120)])
            )
        })
    }

    fn detect_nsfw(record: &ApiRecord) -> bool {
        let vis = record.initial_visibility.as_deref().unwrap_or("");
        if vis == "hide" {
            return true;
        }
        if vis == "show" {
            return false;
        }

        if record.has_nsfw_content.unwrap_or(false) || record.is_nsfw.unwrap_or(false) {
            return true;
        }

        let root = record
            .root_category
            .as_ref()
            .map(|c| c.name.to_lowercase())
            .unwrap_or_default();
        let sub = record
            .sub_category
            .as_ref()
            .map(|c| c.name.to_lowercase())
            .unwrap_or_default();

        root.contains("nsfw") || sub.contains("nsfw")
    }

    async fn fetch_thumbnail(client: &Client, url: &str) -> Vec<u8> {
        let request = client.get(url);

        Self::fetch_bytes(request, &format!("thumbnail '{url}'"))
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to fetch thumbnail '{url}': {e:#}");
                Vec::new()
            })
    }

    async fn fetch_one(client: &Client, record: ApiRecord) -> NteMod {
        let thumb_url = record
            .preview_media
            .as_ref()
            .and_then(|media| media.images.as_ref())
            .and_then(|images| images.first())
            .map(super::types::Image::thumbnail_url);

        let thumbnail_bytes = match thumb_url {
            Some(url) => Self::fetch_thumbnail(client, &url).await,
            None => Vec::new(),
        };

        let author = record
            .submitter
            .as_ref()
            .map_or_else(|| "Unknown".to_string(), |s| s.name.clone());
        let root_cat = record
            .root_category
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let sub_cat = record
            .sub_category
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let is_nsfw = Self::detect_nsfw(&record);
        let mod_url = record
            .profile_url
            .clone()
            .unwrap_or_else(|| format!("{}/mods/{}", BASE_URL, record.id));
        let preview_urls = record
            .preview_media
            .as_ref()
            .and_then(|media| media.images.as_ref())
            .map(|images| {
                images
                    .iter()
                    .map(|img| format!("{}/{}", img.base_url, img.file))
                    .collect()
            })
            .unwrap_or_default();

        NteMod {
            id: record.id,
            name: record.name,
            thumbnail: thumbnail_bytes,
            author,
            view_count: record.view_count,
            download_count: record.download_count,
            like_count: record.like_count,
            is_nsfw,
            root_category: root_cat,
            sub_category: sub_cat,
            mod_url,
            preview_urls,
        }
    }

    async fn collect_mods(
        &self,
        records: Vec<ApiRecord>,
        on_mod_ready: Option<&UnboundedSender<NteMod>>,
    ) -> Vec<NteMod> {
        let mut stream = stream::iter(records)
            .map(|record| Self::fetch_one(&self.client, record))
            .buffered(FETCH_CONCURRENCY);

        let mut mods = Vec::new();
        while let Some(m) = stream.next().await {
            if let Some(tx) = on_mod_ready {
                let _ = tx.send(m.clone());
            }
            mods.push(m);
        }
        mods
    }

    async fn fetch_profile_mod(&self, record: &ApiRecord) -> Option<NteMod> {
        let url = format!("{}/apiv11/Mod/{}/ProfilePage", BASE_URL, record.id);
        let request = self.client.get(&url);
        let what = format!("the profile of mod {}", record.id);

        match Self::fetch_json::<ProfilePage>(request, &what).await {
            Ok(profile) => Some(Self::fetch_one(&self.client, profile.record).await),
            Err(e) => {
                warn!("Failed to fetch profile for mod {}: {e:#}", record.id);
                None
            }
        }
    }

    fn only_mod_records(records: Vec<ApiRecord>) -> Vec<ApiRecord> {
        records
            .into_iter()
            .filter(|r| r.model_name.as_deref() == Some("Mod"))
            .collect()
    }

    pub async fn get_nte_mods(
        &self,
        page: u32,
        force_refresh: bool,
        on_mod_ready: Option<UnboundedSender<NteMod>>,
    ) -> Result<Vec<NteMod>> {
        if !force_refresh {
            if let Some(cached) = self.cache.get_feed_cache(page).await {
                if let Some(tx) = on_mod_ready {
                    for m in &cached {
                        let _ = tx.send(m.clone());
                    }
                }
                return Ok(cached);
            }
        }

        let url = format!("{BASE_URL}/apiv11/Game/{NTE_GAME_ID}/Subfeed");
        let request = self.client.get(&url).query(&[("_nPage", page)]);
        let subfeed: SubfeedResponse =
            Self::fetch_json(request, &format!("the feed (page {page})")).await?;

        let only_mods = Self::only_mod_records(subfeed.records);
        if only_mods.is_empty() {
            return Ok(Vec::new());
        }

        let expected = only_mods.len();
        let nte_mods = self.collect_mods(only_mods, on_mod_ready.as_ref()).await;

        if nte_mods.len() == expected {
            self.cache.save_feed_cache(page, nte_mods.clone()).await;
        }

        Ok(nte_mods)
    }

    pub async fn search_nte_mods(
        &self,
        query: &str,
        page: u32,
        force_refresh: bool,
        on_mod_ready: Option<UnboundedSender<NteMod>>,
    ) -> Result<Vec<NteMod>> {
        if query.len() < 3 {
            return Ok(Vec::new());
        }

        if !force_refresh {
            if let Some(cached) = self.cache.get_search_cache(query, page).await {
                if let Some(tx) = on_mod_ready {
                    for m in &cached {
                        let _ = tx.send(m.clone());
                    }
                }
                return Ok(cached);
            }
        }

        let url = format!("{BASE_URL}/apiv11/Util/Search/Results");
        let request = self.client.get(&url).query(&[
            ("_sSearchString", query),
            ("_sModelName", "Mod"),
            ("_idGameRow", &NTE_GAME_ID.to_string()),
            ("_nPage", &page.to_string()),
            ("_nPerpage", "15"),
        ]);
        let search_response: SearchResponse =
            Self::fetch_json(request, &format!("search for '{query}'")).await?;

        let only_mods = Self::only_mod_records(search_response.records);
        if only_mods.is_empty() {
            return Ok(Vec::new());
        }

        let expected = only_mods.len();
        let mut stream = stream::iter(only_mods)
            .map(|record| async move { self.fetch_profile_mod(&record).await })
            .buffered(FETCH_CONCURRENCY);

        let mut nte_mods = Vec::new();
        while let Some(m) = stream.next().await {
            if let Some(m) = m {
                if let Some(tx) = &on_mod_ready {
                    let _ = tx.send(m.clone());
                }
                nte_mods.push(m);
            }
        }

        if nte_mods.len() == expected {
            self.cache
                .save_search_cache(query, page, nte_mods.clone())
                .await;
        }

        Ok(nte_mods)
    }

    pub async fn get_category_mods(
        &self,
        category_id: u32,
        page: u32,
        force_refresh: bool,
        on_mod_ready: Option<UnboundedSender<NteMod>>,
    ) -> Result<Vec<NteMod>> {
        if !force_refresh {
            if let Some(cached) = self.cache.get_category_cache(category_id, page).await {
                if let Some(tx) = on_mod_ready {
                    for m in &cached {
                        let _ = tx.send(m.clone());
                    }
                }
                return Ok(cached);
            }
        }

        let url = format!("{BASE_URL}/apiv11/Mod/Index");
        let request = self.client.get(&url).query(&[
            (
                "_aFilters[Generic_Category]",
                category_id.to_string().as_str(),
            ),
            ("_nPage", &page.to_string()),
            ("_nPerpage", "15"),
        ]);
        let index: SearchResponse = Self::fetch_json(
            request,
            &format!("category {category_id} (page {page})"),
        )
        .await?;

        let only_mods = Self::only_mod_records(index.records);
        if only_mods.is_empty() {
            return Ok(Vec::new());
        }

        let expected = only_mods.len();
        let nte_mods = self.collect_mods(only_mods, on_mod_ready.as_ref()).await;

        if nte_mods.len() == expected {
            self.cache
                .save_category_cache(category_id, page, nte_mods.clone())
                .await;
        }

        Ok(nte_mods)
    }

    pub async fn get_mod_files(&self, mod_id: u32) -> Option<Vec<NteModFile>> {
        let url = format!("{BASE_URL}/apiv11/Mod/{mod_id}/ProfilePage");
        let request = self.client.get(&url);
        let what = format!("the files of mod {mod_id}");

        let profile: ProfilePage = match Self::fetch_json(request, &what).await {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to fetch mod files for mod {mod_id}: {e:#}");
                return None;
            }
        };

        let mut output = Vec::new();
        if let Some(files) = profile.files {
            for f in files {
                output.push(NteModFile {
                    id: f.id,
                    name: f.file_name,
                    size: f.file_size,
                    download_count: f.download_count,
                    url: f.download_url,
                    md5: f.md5_checksum,
                    is_archived: f.is_archived,
                    has_contents: f.has_contents,
                });
            }
        }
        Some(output)
    }
}
