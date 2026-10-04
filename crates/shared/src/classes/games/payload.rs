use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, anyhow};
use super::{Game, identity::SafeRelativePath};
use crate::config::{CopyOutcome, copy_verified};
const WRAPPERS: &str = "Wrappers";
const PLUGINS: &str = "Plugins";
const ADDONS: &str = "Addons";
const LUA: &str = "Lua";
const LUA_MARKER: [&str; 2] = ["ue4ss", "UE4SS.dll"];
const RUNTIME_DIRS: &[&str] = &[ADDONS, LUA];
const SIDECAR_SUFFIXES: &[&str] = &[".bak", ".bak.old", ".part"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadLayoutKind {
    Target,
    LegacyFlat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPayloadLayout {
    root: PathBuf,
    kind: PayloadLayoutKind,
    files: Vec<PathBuf>,
    wrappers: PathBuf,
    plugins: PathBuf,
    addons: PathBuf,
    lua: Option<PathBuf>,
}

impl ResolvedPayloadLayout {
    pub fn inspect(game: &dyn Game, root: &Path, kind: PayloadLayoutKind) -> Result<Self> {
        if !root.is_dir() {
            return Err(anyhow!(
                "{kind:?} payload root {} is missing",
                root.display()
            ));
        }

        let files: Vec<PathBuf> = game
            .descriptor()
            .payload_files
            .iter()
            .map(|file| file.resolve_under(root))
            .collect();
        let dirs = [WRAPPERS, PLUGINS, ADDONS].map(|name| root.join(name));

        let missing: Vec<PathBuf> = files
            .iter()
            .filter(|path| !path.is_file())
            .chain(dirs.iter().filter(|path| !path.is_dir()))
            .cloned()
            .collect();
        if !missing.is_empty() {
            let missing: Vec<String> = missing.iter().map(|p| p.display().to_string()).collect();
            return Err(anyhow!(
                "{kind:?} payload at {} is incomplete; missing {}",
                root.display(),
                missing.join(", ")
            ));
        }

        let lua = root.join(LUA);
        let has_lua = LUA_MARKER
            .iter()
            .fold(lua.clone(), |path, part| path.join(part))
            .is_file();
        let [wrappers, plugins, addons] = dirs;

        Ok(Self {
            root: root.to_path_buf(),
            kind,
            files,
            wrappers,
            plugins,
            addons,
            lua: has_lua.then_some(lua),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub const fn kind(&self) -> PayloadLayoutKind {
        self.kind
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn wrappers(&self) -> &Path {
        &self.wrappers
    }

    pub fn plugins(&self) -> &Path {
        &self.plugins
    }

    pub fn addons(&self) -> &Path {
        &self.addons
    }

    pub fn lua(&self) -> Option<&Path> {
        self.lua.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadRoots {
    bin: PathBuf,
}

impl PayloadRoots {
    pub const fn new(bin: PathBuf) -> Self {
        Self { bin }
    }

    pub fn target(&self, game: &dyn Game) -> PathBuf {
        game.descriptor().payload_dir.resolve_under(&self.bin)
    }

    pub fn legacy(&self) -> &Path {
        &self.bin
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadResolutionMode {
    TargetOnly,
    TargetOrLegacyFlat,
    LegacyFlatOnly,
}

pub fn resolve_payload(
    game: &dyn Game,
    roots: &PayloadRoots,
    mode: PayloadResolutionMode,
) -> Result<ResolvedPayloadLayout> {
    let target =
        || ResolvedPayloadLayout::inspect(game, &roots.target(game), PayloadLayoutKind::Target);
    let legacy =
        || ResolvedPayloadLayout::inspect(game, roots.legacy(), PayloadLayoutKind::LegacyFlat);

    match mode {
        PayloadResolutionMode::TargetOnly => target(),
        PayloadResolutionMode::LegacyFlatOnly => legacy(),
        PayloadResolutionMode::TargetOrLegacyFlat => match target() {
            Ok(layout) => Ok(layout),
            Err(target) => {
                log::warn!("Falling back to the flat payload: {target}");
                legacy().map_err(|legacy| anyhow!("no complete payload: {target:#}; {legacy:#}"))
            }
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpgradeEligibility {
    FirstUpgrade,
    InterruptedRetry,
}

#[derive(Debug, Clone)]
pub struct LegacyRuntimeMigrationRequest {
    source: PathBuf,
    target: ResolvedPayloadLayout,
    owned: Vec<SafeRelativePath>,
    eligibility: UpgradeEligibility,
}

impl LegacyRuntimeMigrationRequest {
    pub fn new(
        source: PathBuf,
        target: ResolvedPayloadLayout,
        owned: Vec<SafeRelativePath>,
        eligibility: UpgradeEligibility,
    ) -> Result<Self> {
        if target.kind() != PayloadLayoutKind::Target {
            return Err(anyhow!(
                "{} is not a target payload; legacy data is only prepared into one",
                target.root().display()
            ));
        }
        Ok(Self {
            source,
            target,
            owned,
            eligibility,
        })
    }

    pub const fn eligibility(&self) -> UpgradeEligibility {
        self.eligibility
    }

    fn is_owned(&self, path: &SafeRelativePath) -> bool {
        self.owned.iter().any(|owned| {
            owned.components().len() == path.components().len()
                && owned
                    .components()
                    .iter()
                    .zip(path.components())
                    .all(|(a, b)| a.eq_ignore_ascii_case(b))
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MigrationReport {
    pub copied: Vec<SafeRelativePath>,
    pub identical: Vec<SafeRelativePath>,
    pub skipped_owned: Vec<SafeRelativePath>,
    pub skipped_sidecars: Vec<SafeRelativePath>,
    pub skipped_other: Vec<PathBuf>,
}

enum Planned {
    Copy,
    Identical,
    Conflict,
}

fn is_sidecar(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    SIDECAR_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

fn collect_files(
    source: &Path,
    dir: &str,
    report: &mut MigrationReport,
) -> Result<Vec<SafeRelativePath>> {
    let mut files = Vec::new();
    let mut pending = vec![(source.join(dir), dir.to_string())];

    while let Some((path, rel)) = pending.pop() {
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(e).with_context(|| format!("could not read {}", path.display()));
            }
        };

        for entry in entries {
            let entry = entry.with_context(|| format!("could not read {}", path.display()))?;
            let child = entry.path();
            let kind = entry
                .file_type()
                .with_context(|| format!("could not inspect {}", child.display()))?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                report.skipped_other.push(child);
                continue;
            };
            let child_rel = format!("{rel}/{name}");

            if kind.is_dir() {
                pending.push((child, child_rel));
            } else if !kind.is_file() {
                report.skipped_other.push(child);
            } else {
                match SafeRelativePath::parse(&child_rel) {
                    Ok(safe) => files.push(safe),
                    Err(_) => report.skipped_other.push(child),
                }
            }
        }
    }

    Ok(files)
}

fn same_contents(a: &Path, b: &Path) -> Result<bool> {
    let a = fs::read(a).with_context(|| format!("could not read {}", a.display()))?;
    let b = fs::read(b).with_context(|| format!("could not read {}", b.display()))?;
    Ok(md5::compute(a).0 == md5::compute(b).0)
}

fn plan_file(src: &Path, target_root: &Path, rel: &SafeRelativePath) -> Result<Planned> {
    let mut dir = target_root.to_path_buf();
    let (name, parents) = rel
        .components()
        .split_last()
        .expect("a parsed path has a component");

    for component in parents {
        dir.push(component);
        match fs::symlink_metadata(&dir) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Ok(Planned::Conflict),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Planned::Copy),
            Err(e) => {
                return Err(e).with_context(|| format!("could not inspect {}", dir.display()));
            }
        }
    }

    let dest = dir.join(name);
    match fs::symlink_metadata(&dest) {
        Ok(meta) if meta.is_file() => {
            if same_contents(src, &dest)? {
                Ok(Planned::Identical)
            } else {
                Ok(Planned::Conflict)
            }
        }
        Ok(_) => Ok(Planned::Conflict),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Planned::Copy),
        Err(e) => Err(e).with_context(|| format!("could not inspect {}", dest.display())),
    }
}

fn conflict_error(paths: &[SafeRelativePath]) -> anyhow::Error {
    let paths: Vec<String> = paths.iter().map(ToString::to_string).collect();
    anyhow!(
        "the target already has different copies of {}",
        paths.join(", ")
    )
}

pub fn prepare_legacy_runtime_data(
    request: &LegacyRuntimeMigrationRequest,
) -> Result<MigrationReport> {
    let target_root = request.target.root();
    let mut report = MigrationReport::default();

    let mut files = Vec::new();
    for dir in RUNTIME_DIRS {
        files.extend(collect_files(&request.source, dir, &mut report)?);
    }
    files.sort_by_key(ToString::to_string);

    let mut to_copy = Vec::new();
    let mut conflicts = Vec::new();
    for rel in files {
        if request.is_owned(&rel) {
            report.skipped_owned.push(rel);
            continue;
        }
        if is_sidecar(rel.file_name()) {
            report.skipped_sidecars.push(rel);
            continue;
        }

        let src = rel.resolve_under(&request.source);
        match plan_file(&src, target_root, &rel)? {
            Planned::Copy => to_copy.push(rel),
            Planned::Identical => report.identical.push(rel),
            Planned::Conflict => conflicts.push(rel),
        }
    }

    if !conflicts.is_empty() {
        return Err(conflict_error(&conflicts));
    }

    for rel in to_copy {
        let src = rel.resolve_under(&request.source);
        let dest = rel.resolve_under(target_root);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }

        match copy_verified(&src, &dest)
            .with_context(|| format!("could not copy {rel} into {}", target_root.display()))?
        {
            CopyOutcome::Copied => report.copied.push(rel),
            CopyOutcome::Identical => report.identical.push(rel),
            CopyOutcome::Conflict => return Err(conflict_error(&[rel])),
        }
    }

    Ok(report)
}