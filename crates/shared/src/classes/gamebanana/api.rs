use super::cache::CacheManager;
use super::types::{ApiRecord, NteMod, NteModFile, ProfilePage, SearchResponse, SubfeedResponse};
use crate::utils::get_local_version;
use anyhow::{Context, Result};
use futures::{stream, StreamExt};
use reqwest::Client;
use tokio::sync::mpsc::UnboundedSender;

use log::*;

const BASE_URL: &str = "https://gamebanana.com";
const NTE_GAME_ID: u32 = 23012;
const FETCH_CONCURRENCY: usize = 15;

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

    fn detect_nsfw(record: &ApiRecord) -> bool {
        let vis = record.initial_visibility.as_deref().unwrap_or("");
        if vis == "warn" || vis == "hide" {
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
        let resp = match client.get(url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Failed to fetch thumbnail '{url}': {e}");
                return Vec::new();
            }
        };

        let status = resp.status();
        if !status.is_success() {
            warn!("Thumbnail '{url}' returned HTTP {status}");
            return Vec::new();
        }

        match resp.bytes().await {
            Ok(bytes) => bytes.to_vec(),
            Err(e) => {
                warn!("Failed to read thumbnail '{url}': {e}");
                Vec::new()
            }
        }
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
        let resp = match self.client.get(&url).send().await {
            Ok(resp) => resp,
            Err(e) => {
                warn!("Failed to fetch profile for mod {}: {e}", record.id);
                return None;
            }
        };

        let status = resp.status();
        if !status.is_success() {
            warn!("Profile for mod {} returned HTTP {status}", record.id);
            return None;
        }

        match resp.json::<ProfilePage>().await {
            Ok(profile) => Some(Self::fetch_one(&self.client, profile.record).await),
            Err(e) => {
                warn!("Failed to parse profile for mod {}: {e}", record.id);
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
        let resp = self
            .client
            .get(&url)
            .query(&[("_nPage", page)])
            .send()
            .await
            .with_context(|| format!("could not reach the feed (page {page})"))?
            .error_for_status()
            .with_context(|| format!("the feed returned an error (page {page})"))?;
        let subfeed: SubfeedResponse = resp
            .json()
            .await
            .with_context(|| format!("could not parse the feed (page {page})"))?;

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
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("_sSearchString", query),
                ("_sModelName", "Mod"),
                ("_idGameRow", &NTE_GAME_ID.to_string()),
                ("_nPage", &page.to_string()),
                ("_nPerpage", "15"),
            ])
            .send()
            .await
            .with_context(|| format!("could not reach search for '{query}'"))?
            .error_for_status()
            .with_context(|| format!("search for '{query}' returned an error"))?;

        let search_response: SearchResponse = resp
            .json()
            .await
            .with_context(|| format!("could not parse search results for '{query}'"))?;

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
        let resp = self
            .client
            .get(&url)
            .query(&[
                (
                    "_aFilters[Generic_Category]",
                    category_id.to_string().as_str(),
                ),
                ("_nPage", &page.to_string()),
                ("_nPerpage", "15"),
            ])
            .send()
            .await
            .with_context(|| format!("could not reach category {category_id} (page {page})"))?
            .error_for_status()
            .with_context(|| format!("category {category_id} returned an error (page {page})"))?;
        let index: SearchResponse = resp
            .json()
            .await
            .with_context(|| format!("could not parse category {category_id} (page {page})"))?;

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
        let resp = match self.client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to fetch mod files for mod {mod_id}: {e}");
                return None;
            }
        };

        let status = resp.status();
        if !status.is_success() {
            error!("Mod files for mod {mod_id} returned HTTP {status}");
            return None;
        }

        let profile: ProfilePage = match resp.json().await {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to parse profile for mod {mod_id}: {e}");
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
