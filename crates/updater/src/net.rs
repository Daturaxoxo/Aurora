use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use ipc::manifest::Manifest;
use ipc::manifest_urls;

use crate::logfile::log;

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .build()
        .into()
}

pub fn fetch_manifest() -> Result<Manifest, String> {
    let mut last_err = String::from("no manifest sources configured");
    for url in manifest_urls() {
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
    let manifest: Manifest =
        serde_json::from_str(&body).map_err(|e| format!("invalid manifest JSON: {e}"))?;
    manifest.validate_urls()?;
    Ok(manifest)
}

pub fn download_from_any(
    urls: &[String],
    dest: &Path,
    progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    let mut last_err = String::from("no download sources available");
    for (i, url) in urls.iter().enumerate() {
        match download(url, dest, &mut *progress) {
            Ok(()) => {
                if i > 0 {
                    log(&format!("fell back to {url}"));
                }
                return Ok(());
            }
            Err(e) => {
                log(&format!("download failed from {url}: {e}"));
                let _ = std::fs::remove_file(dest);
                last_err = e;
            }
        }
    }
    Err(format!(
        "all download sources failed (last error: {last_err})"
    ))
}

pub fn download(url: &str, dest: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    let response = agent()
        .get(url)
        .call()
        .map_err(|e| format!("download request failed for {url}: {e}"))?;
    let total = response.body().content_length().unwrap_or(0);
    let mut reader = response.into_body().into_reader();
    let mut file =
        File::create(dest).map_err(|e| format!("failed to create {}: {e}", dest.display()))?;

    let mut done: u64 = 0;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("failed to read download for {url}: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("failed to write {}: {e}", dest.display()))?;
        done += n as u64;
        progress(done, total);
    }
    
    file.sync_all()
        .map_err(|e| format!("failed to flush {}: {e}", dest.display()))?;
    drop(file);

    let size = std::fs::metadata(dest)
        .map_err(|e| {
            format!(
                "{} vanished right after download (likely antivirus): {e}",
                dest.display()
            )
        })?
        .len();
    if size != done {
        return Err(format!(
            "{} is {size} bytes on disk but {done} bytes were written (likely antivirus)",
            dest.display()
        ));
    }
    Ok(())
}
