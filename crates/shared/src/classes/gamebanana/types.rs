use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NteMod {
    pub id: u32,
    pub name: String,
    pub thumbnail: Vec<u8>,
    pub author: String,
    pub view_count: u32,
    pub download_count: u32,
    pub like_count: u32,
    pub is_nsfw: bool,
    pub root_category: String,
    pub sub_category: String,
    pub mod_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NteModFile {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub download_count: u32,
    pub url: String,
    pub md5: String,
    pub is_archived: bool,
    pub has_contents: bool,
}

#[derive(Debug, Deserialize)]
pub struct SubfeedResponse {
    #[serde(rename = "_aRecords")]
    pub records: Vec<ApiRecord>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    #[serde(rename = "_aRecords", default)]
    pub records: Vec<ApiRecord>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiRecord {
    #[serde(rename = "_idRow")]
    pub id: u32,
    #[serde(rename = "_sModelName")]
    pub model_name: Option<String>,
    #[serde(rename = "_sName")]
    pub name: String,
    #[serde(rename = "_sProfileUrl", default)]
    pub profile_url: Option<String>,
    #[serde(rename = "_aSubmitter", default)]
    pub submitter: Option<Submitter>,
    #[serde(rename = "_aPreviewMedia", default)]
    pub preview_media: Option<PreviewMedia>,
    #[serde(rename = "_aRootCategory", default)]
    pub root_category: Option<Category>,
    #[serde(rename = "_aSubCategory", default)]
    pub sub_category: Option<Category>,
    #[serde(rename = "_nViewCount", default)]
    pub view_count: u32,
    #[serde(rename = "_nDownloadCount", default)]
    pub download_count: u32,
    #[serde(rename = "_nLikeCount", default)]
    pub like_count: u32,

    #[serde(rename = "_sInitialVisibility", default)]
    pub initial_visibility: Option<String>,
    #[serde(rename = "_bHasNsfwContent", default)]
    pub has_nsfw_content: Option<bool>,
    #[serde(rename = "_bIsNsfw", default)]
    pub is_nsfw: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProfilePage {
    #[serde(flatten)]
    pub record: ApiRecord,
    #[serde(rename = "_aFiles", default)]
    pub files: Option<Vec<ApiFile>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiFile {
    #[serde(rename = "_idRow")]
    pub id: u32,
    #[serde(rename = "_sFile")]
    pub file_name: String,
    #[serde(rename = "_nFilesize")]
    pub file_size: u64,
    #[serde(rename = "_sDownloadUrl")]
    pub download_url: String,
    #[serde(rename = "_nDownloadCount", default)]
    pub download_count: u32,
    #[serde(rename = "_sMd5Checksum", default)]
    pub md5_checksum: String,
    #[serde(rename = "_bIsArchived", default)]
    pub is_archived: bool,
    #[serde(rename = "_bHasContents", default)]
    pub has_contents: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Submitter {
    #[serde(rename = "_sName")]
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Category {
    #[serde(rename = "_sName")]
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PreviewMedia {
    #[serde(rename = "_aImages", default)]
    pub images: Option<Vec<Image>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Image {
    #[serde(rename = "_sBaseUrl")]
    pub base_url: String,
    #[serde(rename = "_sFile")]
    pub file: String,
    #[serde(rename = "_sFile800")]
    pub file_800: Option<String>,
    #[serde(rename = "_sFile530")]
    pub file_530: Option<String>,
    #[serde(rename = "_sFile220")]
    pub file_220: Option<String>,
}

impl Image {
    pub fn thumbnail_url(&self) -> String {
        let filename = self
            .file_530
            .as_ref()
            .or(self.file_800.as_ref())
            .or(self.file_220.as_ref())
            .unwrap_or(&self.file);

        format!("{}/{}", self.base_url, filename)
    }
}
