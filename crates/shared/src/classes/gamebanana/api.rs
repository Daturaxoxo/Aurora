use super::cache::CacheManager;
use super::types::{
    ApiFile, ApiRecord, GbMod, GbModFile, ModProfile, ProfilePage, SearchResponse, SubfeedResponse,
};
use crate::classes::games::{
    Game, capabilities::GameBananaSupport, find_game, identity::GameId, registry,
};
use crate::utils::{error_chain, get_local_version};
use anyhow::{Context, Result, anyhow};
use futures::{StreamExt, stream};
use reqwest::{Client, RequestBuilder, Response};
use tokio::sync::mpsc::UnboundedSender;

use std::sync::Arc;
use std::time::Duration;

use log::*;

const BASE_URL: &str = "https://gamebanana.com";
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

/// a registered game's `GameBanana` id plus the cache namespace its requests write to (dont delete this comment or else)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteGame {
    pub id: u32,
    pub namespace: GameId,
}

impl RemoteGame {
    pub fn of(game: &dyn Game) -> Option<Self> {
        Some(Self {
            id: game.gamebanana()?.remote_game_id(),
            namespace: game.descriptor().id.clone(),
        })
    }
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
            Err(e) => Attempt::Retry(anyhow!("could not read {what}: {}", error_chain(&e)), None),
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
            .map_or_default(|c| c.name.to_lowercase());
        let sub = record
            .sub_category
            .as_ref()
            .map_or_default(|c| c.name.to_lowercase());

        root.contains("nsfw") || sub.contains("nsfw")
    }

    fn map_files(files: Option<Vec<ApiFile>>) -> Vec<GbModFile> {
        files
            .unwrap_or_default()
            .into_iter()
            .map(|file| GbModFile {
                id: file.id,
                name: file.file_name,
                size: file.file_size,
                download_count: file.download_count,
                url: file.download_url,
                md5: file.md5_checksum,
                is_archived: file.is_archived,
                has_contents: file.has_contents,
            })
            .collect()
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

    async fn fetch_one(client: &Client, record: ApiRecord) -> GbMod {
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
            .map_or_default(|c| c.name.clone());
        let sub_cat = record
            .sub_category
            .as_ref()
            .map_or_default(|c| c.name.clone());
        let is_nsfw = Self::detect_nsfw(&record);
        let mod_url = record
            .profile_url
            .clone()
            .unwrap_or_else(|| format!("{}/mods/{}", BASE_URL, record.id));
        let preview_urls = record
            .preview_media
            .as_ref()
            .and_then(|media| media.images.as_ref())
            .map_or_default(|images| {
                images
                    .iter()
                    .map(|img| {
                        let filename = img.file_800.as_ref().unwrap_or(&img.file);
                        format!("{}/{filename}", img.base_url)
                    })
                    .collect()
            });

        GbMod {
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
        on_mod_ready: Option<&UnboundedSender<Arc<GbMod>>>,
    ) -> Vec<Arc<GbMod>> {
        let mut stream = stream::iter(records)
            .map(|record| Self::fetch_one(&self.client, record))
            .buffered(FETCH_CONCURRENCY);

        let mut mods = Vec::new();
        while let Some(m) = stream.next().await {
            let m = Arc::new(m);
            if let Some(tx) = on_mod_ready {
                let _ = tx.send(Arc::clone(&m));
            }
            mods.push(m);
        }
        mods
    }

    async fn fetch_profile_mod(&self, record: &ApiRecord) -> Option<GbMod> {
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

    pub async fn get_mods(
        &self,
        game: &RemoteGame,
        page: u32,
        force_refresh: bool,
        on_mod_ready: Option<UnboundedSender<Arc<GbMod>>>,
    ) -> Result<Vec<Arc<GbMod>>> {
        if !force_refresh
            && let Some(cached) = self.cache.get_feed_cache(&game.namespace, page).await
        {
            let cached: Vec<_> = cached.into_iter().map(Arc::new).collect();
            if let Some(tx) = on_mod_ready {
                for m in &cached {
                    let _ = tx.send(Arc::clone(m));
                }
            }
            return Ok(cached);
        }

        let url = format!("{BASE_URL}/apiv11/Game/{}/Subfeed", game.id);
        let request = self.client.get(&url).query(&[("_nPage", page)]);
        let subfeed: SubfeedResponse =
            Self::fetch_json(request, &format!("the feed (page {page})")).await?;

        let only_mods = Self::only_mod_records(subfeed.records);
        if only_mods.is_empty() {
            return Ok(Vec::new());
        }

        let expected = only_mods.len();
        let mods = self.collect_mods(only_mods, on_mod_ready.as_ref()).await;

        if mods.len() == expected {
            self.cache
                .save_feed_cache(&game.namespace, page, &mods)
                .await;
        }

        Ok(mods)
    }

    pub async fn search_mods(
        &self,
        game: &RemoteGame,
        query: &str,
        page: u32,
        force_refresh: bool,
        on_mod_ready: Option<UnboundedSender<Arc<GbMod>>>,
    ) -> Result<Vec<Arc<GbMod>>> {
        if query.len() < 3 {
            return Ok(Vec::new());
        }

        if !force_refresh
            && let Some(cached) = self
                .cache
                .get_search_cache(&game.namespace, query, page)
                .await
        {
            let cached: Vec<_> = cached.into_iter().map(Arc::new).collect();
            if let Some(tx) = on_mod_ready {
                for m in &cached {
                    let _ = tx.send(Arc::clone(m));
                }
            }
            return Ok(cached);
        }

        let url = format!("{BASE_URL}/apiv11/Util/Search/Results");
        let request = self.client.get(&url).query(&[
            ("_sSearchString", query),
            ("_sModelName", "Mod"),
            ("_idGameRow", &game.id.to_string()),
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

        let mut mods = Vec::new();
        while let Some(m) = stream.next().await {
            if let Some(m) = m {
                let m = Arc::new(m);
                if let Some(tx) = &on_mod_ready {
                    let _ = tx.send(Arc::clone(&m));
                }
                mods.push(m);
            }
        }

        if mods.len() == expected {
            self.cache
                .save_search_cache(&game.namespace, query, page, &mods)
                .await;
        }

        Ok(mods)
    }

    pub async fn get_category_mods(
        &self,
        game: &RemoteGame,
        category_id: u32,
        page: u32,
        force_refresh: bool,
        on_mod_ready: Option<UnboundedSender<Arc<GbMod>>>,
    ) -> Result<Vec<Arc<GbMod>>> {
        if !force_refresh
            && let Some(cached) = self
                .cache
                .get_category_cache(&game.namespace, category_id, page)
                .await
        {
            let cached: Vec<_> = cached.into_iter().map(Arc::new).collect();
            if let Some(tx) = on_mod_ready {
                for m in &cached {
                    let _ = tx.send(Arc::clone(m));
                }
            }
            return Ok(cached);
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
        let index: SearchResponse =
            Self::fetch_json(request, &format!("category {category_id} (page {page})")).await?;

        let only_mods = Self::only_mod_records(index.records);
        if only_mods.is_empty() {
            return Ok(Vec::new());
        }

        let expected = only_mods.len();
        let mods = self.collect_mods(only_mods, on_mod_ready.as_ref()).await;

        if mods.len() == expected {
            self.cache
                .save_category_cache(&game.namespace, category_id, page, &mods)
                .await;
        }

        Ok(mods)
    }

    pub async fn get_mod_files(&self, mod_id: u32) -> Option<Vec<GbModFile>> {
        self.get_mod_files_and_version(mod_id)
            .await
            .map(|(files, _)| files)
    }

    pub async fn get_mod_files_and_version(
        &self,
        mod_id: u32,
    ) -> Option<(Vec<GbModFile>, Option<String>)> {
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

        let version = profile
            .version
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());

        Some((Self::map_files(profile.files), version))
    }

    pub async fn get_mod_profile(&self, mod_id: u32) -> Result<ModProfile> {
        let url = format!("{BASE_URL}/apiv11/Mod/{mod_id}/ProfilePage");
        let request = self.client.get(&url);
        let what = format!("the profile of mod {mod_id}");
        let profile: ProfilePage = Self::fetch_json(request, &what).await?;
        let game_id = profile
            .game
            .as_ref()
            .map(|game| game.id)
            .ok_or_else(|| anyhow!("{what} did not identify its game"))?;
        let author = profile
            .record
            .submitter
            .as_ref()
            .map_or_else(|| "Unknown".to_owned(), |submitter| submitter.name.clone());
        let is_nsfw = Self::detect_nsfw(&profile.record);

        let thumbnail_url = profile
            .record
            .preview_media
            .as_ref()
            .and_then(|media| media.images.as_ref())
            .and_then(|images| images.first())
            .map(super::types::Image::thumbnail_url);
        let thumbnail = match thumbnail_url {
            Some(url) => Self::fetch_thumbnail(&self.client, &url).await,
            None => Vec::new(),
        };

        Ok(ModProfile {
            id: profile.record.id,
            game_id,
            name: profile.record.name,
            author,
            thumbnail,
            is_nsfw,
            files: Self::map_files(profile.files),
        })
    }
}

pub fn gamebanana_id(game: &GameId) -> Option<u32> {
    find_game(game.as_str())
        .filter(|found| found.descriptor().id == *game)?
        .gamebanana()
        .map(GameBananaSupport::remote_game_id)
}

pub fn registry_game(gamebanana_id: u32) -> Option<&'static dyn Game> {
    registry().iter().copied().find(|game| {
        game.gamebanana()
            .is_some_and(|support| support.remote_game_id() == gamebanana_id)
    })
}

pub fn game_page(game: &GameId) -> Option<String> {
    gamebanana_id(game).map(|id| format!("{BASE_URL}/games/{id}"))
}
