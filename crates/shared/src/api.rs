pub mod ccu;

use anyhow::{Context, Result, anyhow};
use log::*;
use reqwest::blocking::Client;
use serde::Serialize;
use serde::de::DeserializeOwned;

use std::error::Error;
use std::fmt;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://api.getaurora.moe/v2";
const BASE_URL_ENV: &str = "AURORA_API_BASE";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy)]
pub struct StatusError(pub u16);

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "server returned HTTP {}", self.0)
    }
}

impl Error for StatusError {}

pub fn base_url() -> &'static str {
    static BASE: OnceLock<String> = OnceLock::new();

    BASE.get_or_init(|| {
        std::env::var(BASE_URL_ENV).map_or_else(
            |_| DEFAULT_BASE_URL.to_string(),
            |value| value.trim_end_matches('/').to_string(),
        )
    })
}

pub fn client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();

    CLIENT.get_or_init(|| {
        let version = crate::utils::get_local_version().trim().to_string();

        Client::builder()
            .user_agent(format!("AuroraLauncher/{version}"))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .unwrap_or_default()
    })
}

fn download_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();

    CLIENT.get_or_init(|| {
        let version = crate::utils::get_local_version().trim().to_string();

        Client::builder()
            .user_agent(format!("AuroraLauncher/{version}"))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .unwrap_or_default()
    })
}

/// Downloads an absolute `url` into memory.
pub fn download_bytes(url: &str) -> Result<Vec<u8>> {
    let response = download_client()
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(StatusError(status.as_u16())).with_context(|| format!("GET {url}"));
    }

    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.trim_start_matches("W/")
                .trim_matches('"')
                .to_ascii_lowercase()
        })
        .filter(|e| e.len() == 32 && e.chars().all(|c| c.is_ascii_hexdigit()));

    let bytes = response
        .bytes()
        .with_context(|| format!("reading the body of {url}"))?;

    if let Some(etag) = etag {
        let digest = format!("{:x}", md5::compute(&bytes));
        if digest != etag {
            return Err(anyhow!(
                "{url} did not match its ETag (expected {etag}, got {digest})"
            ));
        }
    }

    trace!("GET {url} -> {} bytes", bytes.len());
    Ok(bytes.to_vec())
}

pub fn write_atomic(dest: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }

    let file_name = dest
        .file_name()
        .ok_or_else(|| anyhow!("'{}' is not a file path", dest.display()))?
        .to_string_lossy()
        .into_owned();
    let temp = dest.with_file_name(format!("{file_name}.part"));

    let written = fs::write(&temp, bytes)
        .with_context(|| format!("writing '{}' to disk", temp.display()))
        .and_then(|()| {
            fs::rename(&temp, dest)
                .with_context(|| format!("moving '{}' to '{}'", temp.display(), dest.display()))
        });

    if written.is_err() && temp.exists() {
        if let Err(e) = fs::remove_file(&temp) {
            warn!("Download: could not remove '{}': {e}", temp.display());
        }
    }

    written
}

pub fn download_to(url: &str, dest: &Path) -> Result<()> {
    let bytes = download_bytes(url)?;
    write_atomic(dest, &bytes)
}

pub fn url(path: &str) -> String {
    format!("{}{path}", base_url())
}

pub fn is_retryable(error: &anyhow::Error) -> bool {
    if let Some(status) = error.downcast_ref::<StatusError>() {
        return status.0 >= 500 || status.0 == 429;
    }

    error
        .downcast_ref::<reqwest::Error>()
        .is_some_and(|e| e.is_timeout() || e.is_connect() || e.is_request())
}

pub fn post_json<T>(path: &str, body: &T) -> Result<()>
where
    T: Serialize + ?Sized,
{
    let url = url(path);

    let response = client()
        .post(&url)
        .json(body)
        .send()
        .with_context(|| format!("POST {url}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(StatusError(status.as_u16())).with_context(|| format!("POST {url}"));
    }

    trace!("POST {url} -> {status}");
    Ok(())
}

pub fn get_json<T: DeserializeOwned>(path: &str) -> Result<T> {
    let url = url(path);

    let response = client()
        .get(&url)
        .send()
        .with_context(|| format!("GET {url}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(StatusError(status.as_u16())).with_context(|| format!("GET {url}"));
    }

    trace!("GET {url} -> {status}");
    response
        .json::<T>()
        .with_context(|| format!("GET {url} returned a body that could not be parsed"))
}
