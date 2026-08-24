use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use ipc::manifest::{LocalManifest, Manifest, is_trusted_url};
pub const UNINSTALLER_EXE: &str = "AuroraUninstaller.exe";
const RUNTIME_FILES: [&str; 6] = [
    ipc::LOCAL_MANIFEST_FILE,
    ipc::AURORA_LOCK_FILE,
    ipc::UPDATER_LOCK_FILE,
    "updater.log",
    "AuroraInstaller.log",
    "AuroraUninstaller.log",
];
const FALLBACK_FILES: [&str; 3] = [ipc::AURORA_EXE, ipc::UPDATER_EXE, UNINSTALLER_EXE];
type PackagedCheck = fn(&str) -> bool;
const PACKAGED_FILES: [(&str, PackagedCheck); 2] = [
    ("manifest.json", is_aurora_manifest),
    ("steam_appid.txt", is_aurora_appid),
];
const STEAM_APP_ID: &str = "4508340";
const SIDECAR_SUFFIXES: [&str; 2] = ["tmp", "bak"];
const BIN_DIR: &str = "Bin";
const LOGS_DIR: &str = "Logs";
const BIN_SIGNATURE: [&str; 3] = ["AuroraEngine.dll", "cutils.dll", "Everlight.asi"];

pub struct Owned {
    pub trees: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    pub prune: Vec<PathBuf>,
    pub foreign: Vec<PathBuf>,
    pub from_manifest: bool,
}

pub fn resolve(app_dir: &Path) -> Owned {
    let manifest = LocalManifest::load(app_dir).ok().flatten();
    let listed: Vec<String> = manifest
        .map(|m| m.files.into_keys().collect())
        .unwrap_or_default();
    let from_manifest = !listed.is_empty();

    let mut relative: BTreeSet<PathBuf> = BTreeSet::new();
    if from_manifest {
        relative.extend(listed.iter().filter_map(|path| relative_path(path)));
    } else {
        relative.extend(FALLBACK_FILES.iter().filter_map(|name| relative_path(name)));
    }
    relative.extend(RUNTIME_FILES.iter().filter_map(|name| relative_path(name)));

    for path in relative.clone() {
        for suffix in SIDECAR_SUFFIXES {
            relative.insert(with_suffix(&path, suffix));
        }
    }

    let bin = app_dir.join(BIN_DIR);
    let bin_owned = is_aurora_bin(&bin);
    let logs = app_dir.join(LOGS_DIR);
    let log_files = log_files(&logs);

    let mut trees = Vec::new();
    if bin_owned {
        trees.push(bin.clone());
    }

    let mut files = Vec::new();
    let mut prune: BTreeSet<PathBuf> = BTreeSet::new();
    for rel in &relative {
        let path = app_dir.join(rel);
        if bin_owned && path.starts_with(&bin) {continue}
        if !path.is_file() {continue}
        prune.extend(parents_below(app_dir, &path));
        files.push(path);
    }

    if !log_files.is_empty() {
        files.extend(log_files);
        prune.insert(logs);
    }

    let packaged = packaged_files(app_dir);
    files.extend(packaged.iter().cloned());

    let mut owned_names: BTreeSet<String> = relative
        .iter()
        .filter_map(|rel| rel.components().next().map(|c| key(c.as_os_str())))
        .collect();
    if bin_owned {
        owned_names.insert(key(BIN_DIR.as_ref()));
    }
    if prune.contains(&app_dir.join(LOGS_DIR)) {
        owned_names.insert(key(LOGS_DIR.as_ref()));
    }
    owned_names.extend(packaged.iter().filter_map(|path| path.file_name().map(key)));

    Owned {
        trees,
        files,
        prune: deepest_first(prune),
        foreign: foreign_in(app_dir, &owned_names),
        from_manifest,
    }
}

pub fn foreign_entries(app_dir: &Path) -> Vec<PathBuf> {
    resolve(app_dir).foreign
}

fn foreign_in(app_dir: &Path, owned_names: &BTreeSet<String>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(app_dir) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|entry| !owned_names.contains(&key(&entry.file_name())))
        .map(|entry| entry.path())
        .collect()
}

fn log_files(logs: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(logs) else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|entry| {
            let name = key(&entry.file_name());
            (name.starts_with("aurora-") && name.ends_with(".log")) || name == "updater.log"
        })
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect()
}

fn packaged_files(app_dir: &Path) -> Vec<PathBuf> {
    PACKAGED_FILES
        .iter()
        .map(|(name, is_aurora)| (app_dir.join(name), is_aurora))
        .filter(|(path, is_aurora)| {
            std::fs::read_to_string(path).is_ok_and(|contents| is_aurora(&contents))
        })
        .map(|(path, _)| path)
        .collect()
}

fn is_aurora_manifest(contents: &str) -> bool {
    serde_json::from_str::<Manifest>(contents)
        .is_ok_and(|manifest| manifest.files.iter().any(|entry| is_trusted_url(&entry.url)))
}

fn is_aurora_appid(contents: &str) -> bool {
    contents.trim() == STEAM_APP_ID
}

fn is_aurora_bin(bin: &Path) -> bool {
    bin.is_dir() && BIN_SIGNATURE.iter().any(|name| bin.join(name).is_file())
}

fn parents_below(app_dir: &Path, path: &Path) -> Vec<PathBuf> {
    path.ancestors()
        .skip(1)
        .take_while(|dir| *dir != app_dir)
        .map(Path::to_path_buf)
        .collect()
}

fn deepest_first(dirs: BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = dirs.into_iter().collect();
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    dirs
}

fn relative_path(value: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in value.split(['/', '\\']) {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.contains(':')
        {return None}
        out.push(component);
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.push('.');
    name.push_str(suffix);
    path.with_file_name(name)
}

fn key(name: &std::ffi::OsStr) -> String {
    name.to_string_lossy().to_lowercase()
}