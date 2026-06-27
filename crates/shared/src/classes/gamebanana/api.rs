use super::cache::CacheManager;
use super::types::{ApiRecord, NteMod, NteModFile, ProfilePage, SearchResponse, SubfeedResponse};
use crate::utils::get_local_version;
use futures::{stream, StreamExt};
use reqwest::Client;
use tokio::sync::mpsc::UnboundedSender;

const BASE_URL: &str = "https://gamebanana.com";
const NTE_GAME_ID: u32 = 23012;

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
        Self {
            client: Client::builder()
                .user_agent(format!("AuroraLauncher/{}", get_local_version()))
                .build()
                .unwrap_or_default(),
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

    async fn fetch_one(client: &Client, record: ApiRecord) -> Option<NteMod> {
        let mut thumbnail_bytes = Vec::new();

        if let Some(media) = &record.preview_media {
            if let Some(images) = &media.images {
                if let Some(image) = images.first() {
                    let thumb_url = image.thumbnail_url();
                    if let Ok(resp) = client.get(&thumb_url).send().await {
                        if let Ok(bytes) = resp.bytes().await {
                            thumbnail_bytes = bytes.to_vec();
                        }
                    }
                }
            }
        }

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

        Some(NteMod {
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
        })
    }

    pub async fn get_nte_mods(
        &self,
        page: u32,
        force_refresh: bool,
        on_mod_ready: Option<UnboundedSender<NteMod>>,
    ) -> Option<Vec<NteMod>> {
        if !force_refresh {
            if let Some(cached) = self.cache.get_feed_cache(page).await {
                if let Some(tx) = on_mod_ready {
                    for m in &cached {
                        let _ = tx.send(m.clone());
                    }
                }
                return Some(cached);
            }
        }

        let url = format!("{BASE_URL}/apiv11/Game/{NTE_GAME_ID}/Subfeed");
        let resp = self
            .client
            .get(&url)
            .query(&[("_nPage", page)])
            .send()
            .await
            .ok()?;
        let subfeed: SubfeedResponse = resp.json().await.ok()?;

        let only_mods: Vec<ApiRecord> = subfeed
            .records
            .into_iter()
            .filter(|r| r.model_name == "Mod")
            .collect();

        if only_mods.is_empty() {
            return None;
        }

        let nte_mods: Vec<NteMod> = stream::iter(only_mods)
            .map(|record| {
                let value = on_mod_ready.clone();
                async move {
                    let m = Self::fetch_one(&self.client, record).await?;
                    if let Some(tx) = &value {
                        let _ = tx.send(m.clone());
                    }
                    Some(m)
                }
            })
            .buffer_unordered(15)
            .filter_map(|m| async { m })
            .collect()
            .await;

        if !nte_mods.is_empty() {
            self.cache.save_feed_cache(page, nte_mods.clone()).await;
        }

        Some(nte_mods)
    }

    pub async fn search_nte_mods(
        &self,
        query: &str,
        page: u32,
        force_refresh: bool,
        on_mod_ready: Option<UnboundedSender<NteMod>>,
    ) -> Option<Vec<NteMod>> {
        if query.len() < 3 {
            return None;
        }

        if !force_refresh {
            if let Some(cached) = self.cache.get_search_cache(query, page).await {
                if let Some(tx) = on_mod_ready {
                    for m in &cached {
                        let _ = tx.send(m.clone());
                    }
                }
                return Some(cached);
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
            .ok()?;

        let search_response: SearchResponse = resp.json().await.ok()?;
        let only_mods: Vec<ApiRecord> = search_response
            .records
            .into_iter()
            .filter(|r| r.model_name == "Mod")
            .collect();

        let nte_mods: Vec<NteMod> = stream::iter(only_mods)
            .map(|record| {
                let value = on_mod_ready.clone();
                async move {
                    let profile_url = format!("{}/apiv11/Mod/{}/ProfilePage", BASE_URL, record.id);
                    if let Ok(profile_resp) = self.client.get(&profile_url).send().await {
                        if let Ok(full_profile) = profile_resp.json::<ProfilePage>().await {
                            let m = Self::fetch_one(&self.client, full_profile.record).await?;
                            if let Some(tx) = &value {
                                let _ = tx.send(m.clone());
                            }
                            return Some(m);
                        }
                    }
                    None
                }
            })
            .buffer_unordered(15)
            .filter_map(|m| async { m })
            .collect()
            .await;

        if !nte_mods.is_empty() {
            self.cache
                .save_search_cache(query, page, nte_mods.clone())
                .await;
        }

        Some(nte_mods)
    }

    pub async fn get_mod_files(&self, mod_id: u32) -> Option<Vec<NteModFile>> {
        let url = format!("{BASE_URL}/apiv11/Mod/{mod_id}/ProfilePage");
        let resp = self.client.get(&url).send().await.ok()?;
        let profile: ProfilePage = resp.json().await.ok()?;

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
