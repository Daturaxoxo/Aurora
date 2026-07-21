use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

use ipc::manifest::Manifest;
use ipc::{MANIFEST_URL_FALLBACK, MANIFEST_URL_PRIMARY};

use crate::logfile::log;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .build()
        .into()
}

pub fn fetch_manifest() -> Result<Manifest, String> {
    let mut last_err = String::new();
    for url in [MANIFEST_URL_PRIMARY, MANIFEST_URL_FALLBACK] {
        match fetch_manifest_from(url) {
            Ok(manifest) => return Ok(manifest),
            Err(e) => {
                log(&format!("manifest fetch failed from {url}: {e}"));
                last_err = e;
            }
        }
    }
    Err(format!(
        "all manifest sources failed (last error: {last_err})"
    ))
}

fn fetch_manifest_from(url: &str) -> Result<Manifest, String> {
    let mut response = agent()
        .get(url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("failed to read body: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("invalid manifest JSON: {e}"))
}

pub fn download(url: &str, dest: &Path) -> Result<(), String> {
    let response = agent()
        .get(url)
        .call()
        .map_err(|e| format!("download request failed for {url}: {e}"))?;
    let mut reader = response.into_body().into_reader();
    let mut file =
        File::create(dest).map_err(|e| format!("failed to create {}: {e}", dest.display()))?;
    io::copy(&mut reader, &mut file)
        .map_err(|e| format!("failed to write {}: {e}", dest.display()))?;
    Ok(())
}
