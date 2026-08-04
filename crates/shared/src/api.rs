pub mod ccu;
use std::error::Error;
use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;
use anyhow::{Context, Result};
use log::*;
use reqwest::blocking::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;
const DEFAULT_BASE_URL: &str = "https://api.getaurora.moe/v2";
const BASE_URL_ENV: &str = "AURORA_API_BASE";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
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