use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::LOCAL_MANIFEST_FILE;

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

impl Manifest {
    pub fn changed_files(&self, install_root: &Path, local: &LocalManifest) -> Vec<&FileEntry> {
        self.files
            .iter()
            .filter(|entry| {
                if entry.path == crate::UPDATER_EXE {
                    return false;
                }
                if !install_root.join(&entry.path).exists() {
                    return true;
                }
                local.files.get(&entry.path) != Some(&entry.sha256)
            })
            .collect()
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
        fs::write(install_root.join(LOCAL_MANIFEST_FILE), json)
    }

    pub fn build_manifest_from_disk(install_root: &Path, manifest: &Manifest) -> Self {
        let mut files = BTreeMap::new();
        for entry in &manifest.files {
            if let Ok(hash) = hash_file(&install_root.join(&entry.path)) {
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
