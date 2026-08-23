use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use ipc::manifest::Manifest;
use ipc::manifest_urls;

use shared::utils;

use crate::logfile::log;

const CHUNK: usize = 64 * 1024;

fn agent(global_timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(ipc::user_agent(&utils::get_local_version()))
        .timeout_connect(Some(ipc::HTTP_CONNECT_TIMEOUT))
        .timeout_recv_response(Some(ipc::HTTP_MANIFEST_TIMEOUT))
        .timeout_global(Some(global_timeout))
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
    let mut response = agent(ipc::HTTP_MANIFEST_TIMEOUT)
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
        let attempt = attempt_path(dest, i);
        let _ = std::fs::remove_file(&attempt);
        match download(url, &attempt, &mut *progress) {
            Ok(()) => {
                if let Err(e) = std::fs::rename(&attempt, dest) {
                    let _ = std::fs::remove_file(&attempt);
                    last_err = format!("failed to move the download into place: {e}");
                    log(&last_err);
                    continue;
                }
                if i > 0 {
                    log(&format!("fell back to {url}"));
                }
                return Ok(());
            }
            Err(e) => {
                log(&format!("download failed from {url}: {e}"));
                let _ = std::fs::remove_file(&attempt);
                last_err = e;
            }
        }
    }
    Err(format!(
        "all download sources failed (last error: {last_err})"
    ))
}

fn attempt_path(dest: &Path, attempt: usize) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    dest.with_file_name(format!("{name}.{attempt}.part"))
}

pub fn cleanup_attempts(dest: &Path) {
    for i in 0..8 {
        let path = attempt_path(dest, i);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

enum Tick {
    Progress { done: u64, total: u64 },
    Done(Result<u64, String>),
}

pub fn download(url: &str, dest: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let url_owned = url.to_owned();
    let dest_owned = dest.to_path_buf();
    std::thread::spawn(move || {
        let result = stream_to_file(&url_owned, &dest_owned, &tx);
        let _ = tx.send(Tick::Done(result));
    });

    let deadline = Instant::now() + ipc::HTTP_DOWNLOAD_TIMEOUT;
    let mut done = 0u64;
    loop {
        let budget =
            ipc::HTTP_STALL_TIMEOUT.min(deadline.saturating_duration_since(Instant::now()));
        if budget.is_zero() {
            return Err(format!(
                "download of {url} exceeded {}s and was abandoned",
                ipc::HTTP_DOWNLOAD_TIMEOUT.as_secs()
            ));
        }
        match rx.recv_timeout(budget) {
            Ok(Tick::Progress { done: d, total }) => {
                done = d;
                progress(done, total);
            }
            Ok(Tick::Done(result)) => {
                let written = result?;
                return verify_size(dest, written);
            }
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "download of {url} made no progress for {}s ({done} bytes received); \
                     the connection appears to be stalled",
                    ipc::HTTP_STALL_TIMEOUT.as_secs()
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("download of {url} ended unexpectedly"));
            }
        }
    }
}

fn stream_to_file(url: &str, dest: &Path, tx: &mpsc::Sender<Tick>) -> Result<u64, String> {
    let response = agent(ipc::HTTP_DOWNLOAD_TIMEOUT)
        .get(url)
        .call()
        .map_err(|e| format!("download request failed for {url}: {e}"))?;
    let total = response.body().content_length().unwrap_or(0);
    let mut reader = response.into_body().into_reader();
    let mut file =
        File::create(dest).map_err(|e| format!("failed to create {}: {e}", dest.display()))?;

    let mut done: u64 = 0;
    let mut buf = vec![0u8; CHUNK];
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
        if tx.send(Tick::Progress { done, total }).is_err() {
            return Err("download abandoned".to_owned());
        }
    }

    file.sync_all()
        .map_err(|e| format!("failed to flush {}: {e}", dest.display()))?;
    drop(file);
    Ok(done)
}

fn verify_size(dest: &Path, written: u64) -> Result<(), String> {
    let size = std::fs::metadata(dest)
        .map_err(|e| {
            format!(
                "{} vanished right after download (likely antivirus): {e}",
                dest.display()
            )
        })?
        .len();
    if size != written {
        return Err(format!(
            "{} is {size} bytes on disk but {written} bytes were written (likely antivirus)",
            dest.display()
        ));
    }
    Ok(())
}
