pub mod capabilities;
pub mod identity;
pub mod launch;
pub mod markers;
pub mod nte;
pub mod payload;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::{Result, anyhow};
use serde_json::Value;

use crate::config::ProfileKey;
use capabilities::{AddonSupport, LauncherSupport};
use identity::{GameId, SafeRelativePath};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKind {
    Pak,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherIdentifier {
    pub file: SafeRelativePath,
    pub variant: &'static str,
}

#[derive(Debug, Clone)]
pub struct GameDescriptor {
    pub id: GameId,
    pub display_name: &'static str,
    pub aliases: &'static [&'static str],
    pub launchers: Vec<LauncherIdentifier>,
    pub markers: Vec<SafeRelativePath>,
    pub processes: &'static [&'static str],
    pub game_executable: SafeRelativePath,
    pub binaries: SafeRelativePath,
    pub payload_dir: SafeRelativePath,
    pub engine: EngineKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallationFacts {
    root: PathBuf,
    variant: &'static str,
    distribution: &'static str,
}

impl InstallationFacts {
    pub const fn new(root: PathBuf, variant: &'static str, distribution: &'static str) -> Self {
        Self {
            root,
            variant,
            distribution,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub const fn variant(&self) -> &'static str {
        self.variant
    }

    pub const fn distribution(&self) -> &'static str {
        self.distribution
    }
}

pub trait Game: Send + Sync {
    fn descriptor(&self) -> &GameDescriptor;
    fn inspect_installation(&self, root: &Path) -> Result<InstallationFacts>;
    fn profile_default(&self, key: ProfileKey) -> Value {
        let _ = key;
        Value::Null
    }

    fn launcher(&self) -> Option<&dyn LauncherSupport> {
        None
    }
    fn addons(&self) -> Option<&dyn AddonSupport> {
        None
    }
}

static REGISTRY: LazyLock<Vec<&'static dyn Game>> = LazyLock::new(|| {
    let games: Vec<&'static dyn Game> = vec![&*nte::NTE];
    if let Err(e) = validate_registry(&games) {
        panic!("invalid game registry: {e}");
    }
    games
});

pub fn registry() -> &'static [&'static dyn Game] {
    &REGISTRY
}

pub fn find_game(id_or_alias: &str) -> Option<&'static dyn Game> {
    registry().iter().copied().find(|game| {
        let d = game.descriptor();
        d.id.as_str().eq_ignore_ascii_case(id_or_alias)
            || d.aliases
                .iter()
                .any(|a| a.eq_ignore_ascii_case(id_or_alias))
    })
}

pub fn default_game() -> &'static dyn Game {
    registry()[0]
}

fn identifiers(d: &GameDescriptor) -> Vec<(&'static str, String)> {
    let mut values = vec![("id", d.id.to_string())];
    values.extend(d.aliases.iter().map(|a| ("aliases", (*a).to_string())));
    values.extend(
        d.launchers
            .iter()
            .map(|l| ("launchers", l.file.to_string())),
    );
    values.extend(d.markers.iter().map(|m| ("markers", m.to_string())));
    values
}

fn check_list<'a>(
    game: &str,
    field: &'static str,
    values: impl IntoIterator<Item = &'a str>,
) -> Result<()> {
    let mut seen: Vec<String> = Vec::new();
    for value in values {
        if value.trim().is_empty() {
            return Err(anyhow!("{game}: {field} has an empty value"));
        }
        let folded = value.to_lowercase();
        if seen.contains(&folded) {
            return Err(anyhow!("{game}: {field} lists '{value}' more than once"));
        }
        seen.push(folded);
    }
    Ok(())
}

fn check_capabilities(game: &dyn Game) -> Result<()> {
    let id = game.descriptor().id.to_string();

    if let Some(launcher) = game.launcher() {
        let methods = launcher.start_methods();
        if methods.is_empty() {
            return Err(anyhow!("{id}: launcher has no start methods"));
        }
        check_list(&id, "start_methods", methods.iter().map(|m| m.id))?;
    }

    if let Some(addons) = game.addons() {
        let shipped = addons.shipped_addons();
        check_list(&id, "shipped_addons.name", shipped.iter().map(|a| a.name))?;
        check_list(
            &id,
            "shipped_addons.config_key",
            shipped.iter().map(|a| a.config_key),
        )?;
    }

    Ok(())
}

pub fn validate_registry(games: &[&dyn Game]) -> Result<()> {
    if games.is_empty() {
        return Err(anyhow!("The registry has no games"));
    }

    let mut owners: HashMap<String, (String, &'static str)> = HashMap::new();
    for game in games {
        let d = game.descriptor();
        let id = d.id.to_string();

        check_list(&id, "aliases", d.aliases.iter().copied())?;
        let launchers: Vec<String> = d.launchers.iter().map(|l| l.file.to_string()).collect();
        check_list(&id, "launchers", launchers.iter().map(String::as_str))?;
        check_list(
            &id,
            "launchers.variant",
            d.launchers.iter().map(|l| l.variant),
        )?;
        let markers: Vec<String> = d.markers.iter().map(ToString::to_string).collect();
        check_list(&id, "markers", markers.iter().map(String::as_str))?;
        check_list(&id, "processes", d.processes.iter().copied())?;
        if d.display_name.trim().is_empty() {
            return Err(anyhow!("{id}: display name has an empty value"));
        }

        let mut claimed: Vec<(String, &'static str)> = Vec::new();
        for (field, value) in identifiers(d) {
            let folded = value.to_lowercase();
            if let Some(first) = owners.get(&folded) {
                return Err(anyhow!(
                    "'{value}' is claimed by {}.{} and {}.{}",
                    first.0,
                    first.1,
                    id,
                    field
                ));
            }
            claimed.push((folded, field));
        }
        for (folded, field) in claimed {
            owners.entry(folded).or_insert_with(|| (id.clone(), field));
        }

        check_capabilities(*game)?;
    }

    Ok(())
}
