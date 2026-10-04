use super::profile::{ProfileKey, Store, modules_path_under, profile_mut, profile_of};
use super::{get_userdata_path, write_durable};
use crate::classes::games::{Game, identity::GameId, identity::SafeRelativePath, nte};
use anyhow::{Context, Result, anyhow};
use log::*;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

// don't add .dll here until we get our custom engine working, or well never add it idk (needs deciding from me ofc) -daturas
const MODULE_SUFFIXES: [&str; 2] = [".asi", ".asi.disabled"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModuleMigration {
    pub copied: Vec<String>,
    pub identical: Vec<String>,
    pub conflicting: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub skipped: Vec<String>,
    verified: HashMap<String, PathBuf>,
}

impl ModuleMigration {
    pub fn destination(&self, name: &str) -> Option<&Path> {
        self.verified.get(name).map(PathBuf::as_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileMigrationReport {
    pub game: GameId,
    pub copied_keys: Vec<ProfileKey>,
    pub modules: Option<ModuleMigration>,
    pub dropped_references: Vec<String>,
}

fn legacy_modules_under(root: &Path) -> PathBuf {
    root.join("ThirdParty").join("Modules")
}

pub fn legacy_modules_path() -> PathBuf {
    legacy_modules_under(&get_userdata_path())
}

pub fn migrate_games() -> Result<ProfileMigrationReport> {
    let game = &nte::NTE.descriptor().id;
    let report = migrate_legacy_in(&Store::system(), game)?;

    if !report.copied_keys.is_empty() {
        info!(
            "Copied {} legacy settings into {}.{game}",
            report.copied_keys.len(),
            super::profile::GAMES
        );
    }
    if let Some(modules) = &report.modules {
        info!(
            "Modules for {game}: {} copied, {} already present",
            modules.copied.len(),
            modules.identical.len()
        );
        for name in &modules.conflicting {
            warn!("Left the differing {game} module '{name}' alone");
        }
        for (name, e) in &modules.failed {
            warn!("Could not copy the {game} module '{name}': {e}");
        }
    }
    for reference in &report.dropped_references {
        warn!("Dropped the reference to the missing module {reference}");
    }

    Ok(report)
}

pub(super) fn migrate_legacy_in(store: &Store, game: &GameId) -> Result<ProfileMigrationReport> {
    let legacy_dir = legacy_modules_under(store.root());
    let target_dir = modules_path_under(store.root(), game);

    store.transact(|data| {
        let mut report = ProfileMigrationReport {
            game: game.clone(),
            copied_keys: Vec::new(),
            modules: None,
            dropped_references: Vec::new(),
        };

        let mut candidates = candidates(data, game)?;
        if candidates.is_empty() {
            return Ok((report, false));
        }
        if let Some((_, value)) = candidates
            .iter_mut()
            .find(|(k, _)| *k == ProfileKey::CustomAddons)
        {
            let modules = copy_modules(&legacy_dir, &target_dir)?;
            *value =
                remap_references(value, &legacy_dir, &modules, &mut report.dropped_references)?;
            report.modules = Some(modules);
        }

        let profile = profile_mut(data, game)?;
        for (key, value) in candidates {
            profile.insert(key.as_str().to_string(), value);
            report.copied_keys.push(key);
        }

        Ok((report, true))
    })
}

fn candidates(data: &Map<String, Value>, game: &GameId) -> Result<Vec<(ProfileKey, Value)>> {
    let profile = profile_of(data, game)?;

    Ok(ProfileKey::ALL
        .iter()
        .filter(|k| !profile.is_some_and(|p| p.contains_key(k.as_str())))
        .filter_map(|k| data.get(k.as_str()).map(|v| (*k, v.clone())))
        .collect())
}

fn is_module_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    MODULE_SUFFIXES
        .iter()
        .any(|suffix| lower.len() > suffix.len() && lower.ends_with(suffix))
}

fn copy_modules(legacy_dir: &Path, target_dir: &Path) -> Result<ModuleMigration> {
    let mut report = ModuleMigration::default();

    let entries = match fs::read_dir(legacy_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(report),
        Err(e) => {
            return Err(e).with_context(|| format!("could not list {}", legacy_dir.display()));
        }
    };

    let mut sources = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("could not list {}", legacy_dir.display()))?;
        let raw = entry.file_name().to_string_lossy().into_owned();

        let is_file = entry.file_type().is_ok_and(|t| t.is_file());
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            report.skipped.push(raw);
            continue;
        };
        if !is_file || !is_module_file(&name) {
            report.skipped.push(name);
            continue;
        }
        match SafeRelativePath::parse(&name) {
            Ok(path) if path.components().len() == 1 => sources.push(name),
            _ => report.skipped.push(name),
        }
    }
    sources.sort();

    if !sources.is_empty() {
        fs::create_dir_all(target_dir)
            .with_context(|| format!("could not create {}", target_dir.display()))?;
    }

    for name in sources {
        let dest = target_dir.join(&name);
        match copy_verified(&legacy_dir.join(&name), &dest) {
            Ok(CopyOutcome::Copied) => {
                report.verified.insert(name.clone(), dest);
                report.copied.push(name);
            }
            Ok(CopyOutcome::Identical) => {
                report.verified.insert(name.clone(), dest);
                report.identical.push(name);
            }
            Ok(CopyOutcome::Conflict) => report.conflicting.push(name),
            Err(e) => report.failed.push((name, e.to_string())),
        }
    }

    Ok(report)
}

pub enum CopyOutcome {
    Copied,
    Identical,
    Conflict,
}

pub fn copy_verified(src: &Path, dest: &Path) -> std::io::Result<CopyOutcome> {
    let bytes = fs::read(src)?;
    let hash = md5::compute(&bytes).0;

    match fs::read(dest) {
        Ok(existing) if md5::compute(&existing).0 == hash => return Ok(CopyOutcome::Identical),
        Ok(_) => return Ok(CopyOutcome::Conflict),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let name = dest.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dest.with_file_name(format!(".{name}.migrating-{}", std::process::id()));
    let staged = write_durable(&tmp, &bytes).and_then(|()| {
        if md5::compute(fs::read(&tmp)?).0 == hash {
            Ok(())
        } else {
            Err(std::io::Error::other("the staged copy does not match"))
        }
    });
    if let Err(e) = staged {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    if let Err(e) = fs::rename(&tmp, dest) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    if md5::compute(fs::read(dest)?).0 != hash {
        let _ = fs::remove_file(dest);
        return Err(std::io::Error::other("the copy does not match"));
    }

    Ok(CopyOutcome::Copied)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        let fold = |p: &Path| {
            p.to_string_lossy()
                .replace('/', "\\")
                .trim_end_matches('\\')
                .to_lowercase()
        };
        if fold(a) == fold(b) {
            return true;
        }
    }

    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn remap_references(
    value: &Value,
    legacy_dir: &Path,
    modules: &ModuleMigration,
    dropped: &mut Vec<String>,
) -> Result<Value> {
    let Value::Array(items) = value else {
        return Ok(value.clone());
    };

    let mut mapped = Vec::with_capacity(items.len());
    for item in items {
        let Some(raw) = item.as_str() else {
            mapped.push(item.clone());
            continue;
        };

        let path = Path::new(raw);
        let name = path.file_name().and_then(|n| n.to_str());
        let legacy = path.parent().is_some_and(|dir| same_dir(dir, legacy_dir));
        let (true, Some(name)) = (legacy, name) else {
            mapped.push(item.clone());
            continue;
        };

        if let Some(dest) = modules.destination(name) {
            mapped.push(Value::from(dest.to_string_lossy().into_owned()));
        } else if legacy_dir.join(name).exists() {
            return Err(anyhow!(
                "migration failed: the module {raw} has no verified copy"
            ));
        } else {
            dropped.push(raw.to_string());
        }
    }

    Ok(Value::Array(mapped))
}
