use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DOWNLOAD_BASE_FALLBACK, DOWNLOAD_BASE_PRIMARY, LOCAL_MANIFEST_FILE};

pub fn fallback_url(relative: &str) -> Option<String> {
    if DOWNLOAD_BASE_FALLBACK.is_empty() {
        return None;
    }
    Some(format!(
        "{DOWNLOAD_BASE_FALLBACK}{}",
        relative.replace(['/', '\\'], "__")
    ))
}

fn download_urls(primary: &str, relative: &str) -> Vec<String> {
    let mut urls = Vec::with_capacity(2);
    if is_trusted_url(primary) {
        urls.push(primary.to_owned());
    }
    if let Some(url) = fallback_url(relative)
        && is_trusted_url(&url)
        && !urls.contains(&url)
    {
        urls.push(url);
    }
    urls
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Manifest {
    pub version: String,
    pub updater_hash: String,
    pub files: Vec<FileEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub sha256: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalCandidate {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug)]
pub struct UpdateDelta<'a> {
    pub downloads: Vec<&'a FileEntry>,
    pub removals: Vec<RemovalCandidate>,
}

impl UpdateDelta<'_> {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.downloads.is_empty() && self.removals.is_empty()
    }
}

#[cfg(target_os = "linux")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinuxManifest {
    pub version: String,
    pub appimage: AppImageEntry,
}

#[cfg(target_os = "linux")]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppImageEntry {
    pub sha256: String,
    pub url: String,
}

#[cfg(target_os = "linux")]
impl AppImageEntry {
    pub fn validate_url(&self) -> Result<(), String> {
        check_url("appimage entry", &self.url)
    }

    pub fn download_urls(&self) -> Vec<String> {
        download_urls(&self.url, crate::APPIMAGE_NAME)
    }
}

const WINDOWS_RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_reserved_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    let stem = stem.trim_end_matches([' ', '.']);
    WINDOWS_RESERVED_NAMES
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(name))
}

fn check_component(component: &str) -> Result<(), String> {
    if component.is_empty() {
        return Err("empty path component".to_owned());
    }
    if component == "." || component == ".." {
        return Err(format!("path component `{component}` is not allowed"));
    }
    if let Some(bad) = component
        .chars()
        .find(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || (*c as u32) < 0x20)
    {
        return Err(format!(
            "path component `{component}` contains invalid character {bad:?}"
        ));
    }
    if component.ends_with(' ') || component.ends_with('.') {
        return Err(format!(
            "path component `{component}` ends with a space or a dot"
        ));
    }
    if is_reserved_name(component) {
        return Err(format!(
            "path component `{component}` is a reserved device name"
        ));
    }
    Ok(())
}

fn check_relative_path(relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty() {
        return Err("empty path".to_owned());
    }
    if relative.contains('\0') {
        return Err("path contains a NUL byte".to_owned());
    }
    let mut out = PathBuf::new();
    for component in relative.split(['/', '\\']) {
        check_component(component)?;
        out.push(component);
    }
    Ok(out)
}

fn path_key(relative: &str) -> String {
    relative.replace('\\', "/").to_lowercase()
}

fn is_protected_owned_path(relative: &str) -> bool {
    let key = path_key(relative);
    [
        crate::AURORA_EXE,
        crate::UPDATER_EXE,
        crate::LOCAL_MANIFEST_FILE,
    ]
    .into_iter()
    .any(|protected| key == path_key(protected))
}

fn is_externally_managed_download(relative: &str) -> bool {
    let key = path_key(relative);
    [crate::UPDATER_EXE, crate::LOCAL_MANIFEST_FILE]
        .into_iter()
        .any(|managed| key == path_key(managed))
}

pub fn safe_join(install_root: &Path, relative: &str) -> Option<PathBuf> {
    check_relative_path(relative)
        .ok()
        .map(|rel| install_root.join(rel))
}

pub fn is_trusted_url(url: &str) -> bool {
    [DOWNLOAD_BASE_PRIMARY, DOWNLOAD_BASE_FALLBACK]
        .into_iter()
        .filter(|base| !base.is_empty())
        .any(|base| {
            let Some(name) = url.strip_prefix(base) else {
                return false;
            };
            !name.is_empty() && !name.contains(['/', '\\', '?', '#'])
        })
}

fn check_url(kind: &str, url: &str) -> Result<(), String> {
    if is_trusted_url(url) {
        Ok(())
    } else {
        Err(format!(
            "rejected {kind}: url `{url}` is not an asset under a known download base"
        ))
    }
}

impl FileEntry {
    pub fn resolve(&self, install_root: &Path) -> Option<PathBuf> {
        safe_join(install_root, &self.path)
    }

    pub fn validate_url(&self) -> Result<(), String> {
        check_url(&format!("manifest entry `{}`", self.path), &self.url)
    }

    pub fn download_urls(&self) -> Vec<String> {
        download_urls(&self.url, &self.path)
    }
}

pub fn hash_eq(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for entry in &self.files {
            check_relative_path(&entry.path)
                .map_err(|e| format!("rejected manifest entry `{}`: {e}", entry.path))?;

            let key = path_key(&entry.path);
            if !seen.insert(key) {
                return Err(format!("manifest lists `{}` more than once", entry.path));
            }
        }
        Ok(())
    }

    pub fn validate_urls(&self) -> Result<(), String> {
        for entry in &self.files {
            entry.validate_url()?;
        }
        Ok(())
    }

    pub fn update_delta<'a>(
        &'a self,
        install_root: &Path,
        local: &LocalManifest,
    ) -> UpdateDelta<'a> {
        let downloads = self
            .files
            .iter()
            .filter(|entry| {
                if is_externally_managed_download(&entry.path) {
                    return false;
                }

                let Some(target) = entry.resolve(install_root) else {
                    return false;
                };

                if !target.exists() {
                    return true;
                }

                let entry_key = path_key(&entry.path);
                local
                    .files
                    .iter()
                    .filter(|(path, _)| path_key(path) == entry_key)
                    .all(|(_, known)| !hash_eq(known, &entry.sha256))
            })
            .collect();

        let shipped: BTreeSet<String> = self
            .files
            .iter()
            .map(|entry| path_key(&entry.path))
            .collect();
        let removals = local
            .files
            .iter()
            .filter(|(path, _)| {
                !is_protected_owned_path(path) && !shipped.contains(&path_key(path))
            })
            .map(|(path, sha256)| RemovalCandidate {
                path: path.clone(),
                sha256: sha256.clone(),
            })
            .collect();

        UpdateDelta {
            downloads,
            removals,
        }
    }

    pub fn changed_files<'a>(
        &'a self,
        install_root: &Path,
        local: &LocalManifest,
    ) -> Vec<&'a FileEntry> {
        self.update_delta(install_root, local).downloads
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct LocalManifest {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
}

impl LocalManifest {
    pub fn load(install_root: &Path) -> io::Result<Option<Self>> {
        let path = install_root.join(LOCAL_MANIFEST_FILE);
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn save(&self, install_root: &Path) -> io::Result<()> {
        let json = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let target = install_root.join(LOCAL_MANIFEST_FILE);
        let temp = install_root.join(format!("{LOCAL_MANIFEST_FILE}.{}.tmp", std::process::id()));

        let write_temp = || -> io::Result<()> {
            let mut file = fs::File::create(&temp)?;
            file.write_all(&json)?;
            file.sync_all()
        };
        if let Err(e) = write_temp() {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        if let Err(e) = fs::rename(&temp, &target) {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        Ok(())
    }

    pub fn build_manifest_from_disk(install_root: &Path, manifest: &Manifest) -> Self {
        let mut files = BTreeMap::new();
        for entry in &manifest.files {
            let Some(target) = entry.resolve(install_root) else {
                continue;
            };
            if let Ok(hash) = hash_file(&target) {
                files.insert(entry.path.clone(), hash);
            }
        }
        Self {
            version: manifest.version.clone(),
            files,
        }
    }
}

pub fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    hex_encode(&Sha256::digest(bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("aurora-manifest-test-{}-{id}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn entry(path: &str, contents: &[u8]) -> FileEntry {
        FileEntry {
            path: path.to_owned(),
            sha256: hash_bytes(contents),
            url: String::new(),
        }
    }

    fn manifest(files: Vec<FileEntry>) -> Manifest {
        Manifest {
            version: "2.1.0".to_owned(),
            updater_hash: String::new(),
            files,
        }
    }

    #[test]
    fn update_delta_detects_removal_only_release() {
        let root = TestDir::new();
        fs::write(root.0.join("current.dll"), b"current").unwrap();
        fs::write(root.0.join("obsolete.dll"), b"obsolete").unwrap();

        let remote = manifest(vec![entry("current.dll", b"current")]);
        let local = LocalManifest {
            version: "2.0.0".to_owned(),
            files: BTreeMap::from([
                ("current.dll".to_owned(), hash_bytes(b"current")),
                ("obsolete.dll".to_owned(), hash_bytes(b"obsolete")),
            ]),
        };

        let delta = remote.update_delta(&root.0, &local);
        assert_eq!(delta.downloads.len(), 0);
        assert_eq!(
            delta.removals,
            [RemovalCandidate {
                path: "obsolete.dll".to_owned(),
                sha256: hash_bytes(b"obsolete"),
            }]
        );
    }

    #[test]
    fn update_delta_detects_mixed_downloads_and_removals() {
        let root = TestDir::new();
        fs::write(root.0.join("changed.dll"), b"old").unwrap();
        fs::write(root.0.join("obsolete.dll"), b"obsolete").unwrap();

        let remote = manifest(vec![entry("changed.dll", b"new")]);
        let local = LocalManifest {
            version: "2.0.0".to_owned(),
            files: BTreeMap::from([
                ("changed.dll".to_owned(), hash_bytes(b"old")),
                ("obsolete.dll".to_owned(), hash_bytes(b"obsolete")),
            ]),
        };

        let delta = remote.update_delta(&root.0, &local);
        assert_eq!(
            delta
                .downloads
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            ["changed.dll"]
        );
        assert_eq!(delta.removals[0].path, "obsolete.dll");
    }

    #[test]
    fn missing_local_manifest_does_not_infer_removals_from_disk() {
        let root = TestDir::new();
        fs::write(root.0.join("current.dll"), b"current").unwrap();
        fs::write(root.0.join("unowned.dll"), b"unowned").unwrap();
        let remote = manifest(vec![entry("current.dll", b"current")]);

        assert!(LocalManifest::load(&root.0).unwrap().is_none());
        let local = LocalManifest::build_manifest_from_disk(&root.0, &remote);
        let delta = remote.update_delta(&root.0, &local);

        assert!(delta.is_empty());
        assert!(!local.files.contains_key("unowned.dll"));
    }

    #[test]
    fn remote_path_identity_uses_windows_case_and_separator_rules() {
        let root = TestDir::new();
        fs::create_dir(root.0.join("Bin")).unwrap();
        fs::write(root.0.join("Bin").join("owned.dll"), b"owned").unwrap();
        let remote = manifest(vec![entry("Bin/owned.dll", b"owned")]);
        let local = LocalManifest {
            version: String::new(),
            files: BTreeMap::from([("BIN\\OWNED.DLL".to_owned(), hash_bytes(b"owned"))]),
        };

        let delta = remote.update_delta(&root.0, &local);

        assert_eq!(delta.removals.len(), 0);
        assert_eq!(delta.downloads.len(), 0);
    }

    #[test]
    fn core_runtime_files_are_never_removal_candidates() {
        let root = TestDir::new();
        let local = LocalManifest {
            version: String::new(),
            files: BTreeMap::from([
                (crate::AURORA_EXE.to_uppercase(), "aurora".to_owned()),
                (crate::UPDATER_EXE.to_uppercase(), "updater".to_owned()),
                (
                    crate::LOCAL_MANIFEST_FILE.to_uppercase(),
                    "manifest".to_owned(),
                ),
                ("obsolete.dll".to_owned(), "obsolete".to_owned()),
            ]),
        };

        let remote = manifest(Vec::new());
        let delta = remote.update_delta(&root.0, &local);

        assert_eq!(delta.removals.len(), 1);
        assert_eq!(delta.removals[0].path, "obsolete.dll");
    }

    #[test]
    fn externally_managed_files_are_not_downloaded_through_the_transaction() {
        let root = TestDir::new();
        let remote = manifest(vec![
            entry(crate::UPDATER_EXE, b"updater"),
            entry(crate::LOCAL_MANIFEST_FILE, b"manifest"),
        ]);

        let delta = remote.update_delta(&root.0, &LocalManifest::default());

        assert_eq!(delta.downloads.len(), 0);
    }
}
