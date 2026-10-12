use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value};

use shared::archive::{verify_zip, zip_directories};
use shared::classes::games::{Game, identity::GameId, nte};
use shared::config::{GAMES, ProfileKey, key};

pub const BIN_DIR: &str = "Bin";
const USERDATA_DIR: &str = "UserData";
const CONFIG_FILE: &str = "config.json";
const THIRD_PARTY_DIR: &str = "ThirdParty";
const PROTECTED_ENV_VARS: [&str; 9] = [
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "ProgramData",
    "SystemRoot",
    "windir",
    "PUBLIC",
    "USERPROFILE",
    "LOCALAPPDATA",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameRoot {
    pub game_id: GameId,
    pub root: PathBuf,
}

impl GameRoot {
    pub fn to_arg(&self) -> String {
        format!("{}={}", self.game_id, self.root.display())
    }

    pub fn parse_arg(raw: &str) -> Result<Self> {
        let (id, root) = raw
            .split_once('=')
            .ok_or_else(|| anyhow!("'{raw}' is not <game>=<path>"))?;

        if root.is_empty() {
            bail!("'{raw}' has no path");
        }

        Ok(Self {
            game_id: GameId::parse(id)?,
            root: PathBuf::from(root),
        })
    }
}

pub fn configured_roots(games: &[&dyn Game], config: &Value) -> Vec<GameRoot> {
    let legacy_owner = &nte::NTE.descriptor().id;

    games
        .iter()
        .filter(|game| game.mod_manager().is_some())
        .filter_map(|game| {
            let id = &game.descriptor().id;
            let profile = config
                .get(GAMES)
                .and_then(|games| games.get(id.as_str()))
                .and_then(|profile| profile.get(ProfileKey::GamePath.as_str()));

            let path = match profile {
                Some(path) => path,
                None if id == legacy_owner => config.get(key::GAME_PATH)?,
                None => return None,
            };
            let root = path.as_str().filter(|path| !path.is_empty())?;

            Some(GameRoot {
                game_id: id.clone(),
                root: PathBuf::from(root),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModTarget {
    pub game_id: GameId,
    pub configured_root: PathBuf,
    pub root: PathBuf,
    pub path: PathBuf,
}

impl ModTarget {
    pub fn revalidate(&self, games: &[&dyn Game]) -> Result<()> {
        let scope = mod_targets(
            games,
            &[GameRoot {
                game_id: self.game_id.clone(),
                root: self.configured_root.clone(),
            }],
        );

        if let Some(e) = scope.rejected.first() {
            bail!("{e}");
        }

        if scope.targets.contains(self) {
            Ok(())
        } else {
            bail!("{} changed since it was checked", self.path.display())
        }
    }
}

#[derive(Debug, Default)]
pub struct ModScope {
    pub targets: Vec<ModTarget>,
    pub rejected: Vec<String>,
}

pub fn mod_targets(games: &[&dyn Game], roots: &[GameRoot]) -> ModScope {
    let mut scope = ModScope::default();

    for root in roots {
        let canonical_root = match game_root(games, root) {
            Ok(Some(canonical)) => canonical,
            Ok(None) => continue,
            Err(e) => {
                scope.rejected.push(format!("{}: {e:#}", root.game_id));
                continue;
            }
        };
        let Some(mods) = games
            .iter()
            .find(|game| game.descriptor().id == root.game_id)
            .and_then(|game| game.mod_manager())
        else {
            continue;
        };

        for lexical in mods.layout(&root.root).roots {
            match mod_folder(&canonical_root, &lexical) {
                Ok(Some(path)) => {
                    if !scope.targets.iter().any(|target| target.path == path) {
                        scope.targets.push(ModTarget {
                            game_id: root.game_id.clone(),
                            configured_root: root.root.clone(),
                            root: canonical_root.clone(),
                            path,
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => scope.rejected.push(format!("{}: {e:#}", root.game_id)),
            }
        }
    }

    scope
}

fn game_root(games: &[&dyn Game], root: &GameRoot) -> Result<Option<PathBuf>> {
    let game = games
        .iter()
        .find(|game| game.descriptor().id == root.game_id)
        .ok_or_else(|| anyhow!("not a registered game"))?;

    if game.mod_manager().is_none() {
        bail!("has no mod folders");
    }

    if !root.root.is_absolute() {
        bail!("{} is not an absolute path", root.root.display());
    }

    if !root.root.exists() {
        return Ok(None);
    }

    let canonical = fs::canonicalize(&root.root)
        .with_context(|| format!("could not resolve {}", root.root.display()))?;
    if is_protected_dir(&canonical) {
        bail!(
            "refusing {}: it is a system or user folder",
            root.root.display()
        );
    }

    Ok(Some(canonical))
}

fn mod_folder(canonical_root: &Path, lexical: &Path) -> Result<Option<PathBuf>> {
    let Ok(meta) = fs::symlink_metadata(lexical) else {
        return Ok(None);
    };

    if meta.file_type().is_symlink() {
        bail!("refusing {}: it is a link", lexical.display());
    }

    if !meta.is_dir() {
        bail!("refusing {}: it is not a folder", lexical.display());
    }

    let canonical = fs::canonicalize(lexical)
        .with_context(|| format!("could not resolve {}", lexical.display()))?;
    if canonical == canonical_root || !canonical.starts_with(canonical_root) {
        bail!(
            "refusing {}: it is not inside {}",
            lexical.display(),
            canonical_root.display()
        );
    }

    if is_protected_dir(&canonical) {
        bail!(
            "refusing {}: it is a system or user folder",
            lexical.display()
        );
    }

    Ok(Some(canonical))
}

pub fn backup_mods(
    targets: &[ModTarget],
    dest: &Path,
    progress: impl FnMut(&Path, u64, u64),
) -> Result<()> {
    let prefixes = archive_prefixes(targets);
    let sources: Vec<(&str, &Path)> = prefixes
        .iter()
        .zip(targets)
        .map(|(prefix, target)| (prefix.as_str(), target.path.as_path()))
        .collect();

    let partial = dest.with_extension("zip.part");
    let result = zip_directories(&sources, &partial, progress)
        .and_then(|files| verify_zip(&partial, &files))
        .and_then(|()| {
            fs::rename(&partial, dest)
                .with_context(|| format!("could not move the backup to {}", dest.display()))
        });

    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn archive_prefixes(targets: &[ModTarget]) -> Vec<String> {
    let mut used: Vec<String> = Vec::new();
    for target in targets {
        let base = target.game_id.to_string();
        let mut prefix = base.clone();
        let mut n = 2;
        while used.contains(&prefix) {
            prefix = format!("{base}-{n}");
            n += 1;
        }
        used.push(prefix);
    }
    used
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct BinScope {
    pub trees: Vec<PathBuf>,
    pub files: Vec<PathBuf>,
    pub kept: Vec<PathBuf>,
}

pub fn bin_scope(app_dir: &Path, games: &[&dyn Game], listed: &[PathBuf]) -> BinScope {
    let bin = app_dir.join(BIN_DIR);
    let mut scope = BinScope::default();
    if !is_real_dir(&bin) {
        return scope;
    }

    let mut claimed: BTreeSet<String> = listed
        .iter()
        .filter_map(|path| path.components().next())
        .map(|first| fold(first.as_os_str()))
        .collect();

    for game in games {
        let d = game.descriptor();
        let dir = d.payload_dir.resolve_under(&bin);

        if is_real_dir(&dir)
            && d.payload_files
                .iter()
                .any(|file| file.resolve_under(&dir).is_file())
        {
            claimed.insert(d.payload_dir.components()[0].to_lowercase());
            scope.trees.push(dir);
        }
    }

    if nte::LEGACY_BIN_SIGNATURE
        .iter()
        .any(|name| is_real_file(&bin.join(name)))
    {
        for name in nte::LEGACY_BIN_FILES {
            let path = bin.join(name);
            if is_real_file(&path) {
                claimed.insert(name.to_lowercase());
                scope.files.push(path);
            }
        }

        for name in nte::LEGACY_BIN_DIRS {
            let path = bin.join(name);
            if is_real_dir(&path) {
                claimed.insert(name.to_lowercase());
                scope.trees.push(path);
            }
        }
    }

    if let Ok(entries) = fs::read_dir(&bin) {
        scope.kept = entries
            .flatten()
            .filter(|entry| !claimed.contains(&fold(&entry.file_name())))
            .map(|entry| entry.path())
            .collect();
        scope.kept.sort();
    }

    scope
}

pub fn remove_download_dir(dir: &Path) -> Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("failed to delete {}", dir.display()))
        }
        _ => Ok(()),
    }
}

pub fn remove_data_dir(data_dir: &Path, games: &[&dyn Game]) -> Result<Vec<PathBuf>> {
    if !data_dir.exists() {
        return Ok(Vec::new());
    }

    let known = |id: &str| games.iter().any(|game| game.descriptor().id.as_str() == id);
    let userdata = data_dir.join(USERDATA_DIR);
    let config_path = userdata.join(CONFIG_FILE);

    let unknown_profiles: Map<String, Value> = fs::read_to_string(&config_path)
        .ok()
        .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())
        .and_then(|config| config.get(GAMES).and_then(Value::as_object).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| !known(id))
        .collect();

    let mut keep = Vec::new();
    if !unknown_profiles.is_empty() {
        keep.push(config_path.clone());
    }
    if let Ok(entries) = fs::read_dir(userdata.join(THIRD_PARTY_DIR)) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let unknown_game = name
                .to_str()
                .and_then(|name| GameId::parse(name).ok())
                .is_some_and(|id| !known(id.as_str()));
            if unknown_game && is_real_dir(&entry.path()) {
                keep.push(entry.path());
            }
        }
    }

    if keep.is_empty() {
        fs::remove_dir_all(data_dir)
            .with_context(|| format!("failed to delete {}", data_dir.display()))?;
        return Ok(keep);
    }

    if !unknown_profiles.is_empty() {
        let mut preserved = Map::new();
        preserved.insert(GAMES.to_string(), Value::Object(unknown_profiles));
        write_config(&config_path, &preserved)?;
    }
    remove_except(data_dir, &keep)?;
    keep.sort();

    Ok(keep)
}

fn write_config(path: &Path, data: &Map<String, Value>) -> Result<()> {
    let tmp = path.with_extension("json.uninstall-tmp");
    let result = serde_json::to_vec_pretty(data)
        .map_err(anyhow::Error::from)
        .and_then(|bytes| {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, path)?;
            Ok(())
        })
        .with_context(|| format!("could not rewrite {}", path.display()));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }

    result
}

fn remove_except(dir: &Path, keep: &[PathBuf]) -> Result<()> {
    let entries = fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;
    for entry in entries {
        let path = entry?.path();
        if keep.contains(&path) {
            continue;
        }
        let file_type = fs::symlink_metadata(&path)?.file_type();
        let result = if file_type.is_dir() && keep.iter().any(|kept| kept.starts_with(&path)) {
            remove_except(&path, keep)
        } else if file_type.is_dir() {
            fs::remove_dir_all(&path).map_err(Into::into)
        } else {
            fs::remove_file(&path)
                .or_else(|_| fs::remove_dir(&path))
                .map_err(Into::into)
        };
        result.with_context(|| format!("failed to delete {}", path.display()))?;
    }
    Ok(())
}

pub fn is_protected_dir(dir: &Path) -> bool {
    let target = normalize(dir);
    if target.parent().is_none() {
        return true;
    }

    protected_dirs()
        .iter()
        .any(|protected| normalize(protected) == target)
}

fn normalize(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn protected_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        dirs::home_dir(),
        dirs::desktop_dir(),
        dirs::download_dir(),
        dirs::document_dir(),
        dirs::picture_dir(),
        dirs::video_dir(),
        dirs::audio_dir(),
        dirs::data_dir(),
        dirs::data_local_dir(),
        dirs::config_dir(),
        dirs::executable_dir(),
    ]
    .into_iter()
    .flatten()
    .collect();

    for var in PROTECTED_ENV_VARS {
        if let Some(value) = std::env::var_os(var) {
            let path = PathBuf::from(value);
            dirs.push(path.join("System32"));
            dirs.push(path);
        }
    }

    dirs
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn is_real_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file())
}

fn fold(name: &OsStr) -> String {
    name.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::*;
    use shared::classes::games::{
        GameDescriptor, InstallationFacts,
        capabilities::{ModLayout, ModManagerSupport, ModNaming},
        identity::SafeRelativePath, validate_registry,
    };

    static NEXT_TEST_DIR: AtomicUsize = AtomicUsize::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let id = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("aurora-uninstall-test-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.0.join(rel)
        }

        fn file(&self, rel: &str, contents: &[u8]) -> PathBuf {
            let path = self.path(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, contents).unwrap();
            path
        }

        fn dir(&self, rel: &str) -> PathBuf {
            let path = self.path(rel);
            fs::create_dir_all(&path).unwrap();
            path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Synthetic {
        descriptor: GameDescriptor,
        mod_root: Option<&'static str>,
    }

    fn rel(raw: &str) -> SafeRelativePath {
        SafeRelativePath::parse(raw).unwrap()
    }

    impl Synthetic {
        fn new(id: &str, mod_root: Option<&'static str>) -> Self {
            Self {
                descriptor: GameDescriptor {
                    id: GameId::parse(id).unwrap(),
                    display_name: "Synthetic",
                    aliases: &[],
                    launchers: Vec::new(),
                    markers: Vec::new(),
                    processes: &[],
                    game_executable: Some(rel("Game.exe")),
                    binaries: Some(rel("Binaries")),
                    payload_dir: rel(id),
                    payload_files: vec![rel("core.dll")],
                    steam: None,
                    engine: None,
                },
                mod_root,
            }
        }
    }

    impl Game for Synthetic {
        fn descriptor(&self) -> &GameDescriptor {
            &self.descriptor
        }

        fn inspect_installation(&self, root: &Path) -> Result<InstallationFacts> {
            Ok(InstallationFacts::new(root.to_path_buf(), "test", "test"))
        }

        fn mod_manager(&self) -> Option<&dyn ModManagerSupport> {
            self.mod_root
                .is_some()
                .then_some(self as &dyn ModManagerSupport)
        }
    }

    const NAMING: ModNaming = ModNaming {
        extensions: &["pak"],
        toggle_extensions: &["pak"],
        disabled_suffix: ".disabled",
        display_suffixes: &[],
    };

    impl ModManagerSupport for Synthetic {
        fn layout(&self, game_root: &Path) -> ModLayout {
            ModLayout {
                roots: vec![game_root.join(self.mod_root.unwrap())],
                naming: NAMING,
            }
        }

        fn naming(&self) -> &ModNaming {
            &NAMING
        }
    }

    fn id(raw: &str) -> GameId {
        GameId::parse(raw).unwrap()
    }

    fn root(game: &str, path: &Path) -> GameRoot {
        GameRoot {
            game_id: id(game),
            root: path.to_path_buf(),
        }
    }

    #[test]
    fn download_dir_removal_covers_every_game_and_tolerates_absence() {
        let dir = TestDir::new();
        dir.file("GameBanana/nte/mod.zip", b"zip");
        dir.file("GameBanana/unknown/mod.zip", b"zip");

        remove_download_dir(&dir.path("GameBanana")).unwrap();
        assert!(!dir.path("GameBanana").exists());
        remove_download_dir(&dir.path("GameBanana")).unwrap();
    }

    #[test]
    fn synthetic_registration_is_valid() {
        let synth = Synthetic::new("synth", Some("Mods"));
        validate_registry(&[&*nte::NTE, &synth]).unwrap();
    }

    #[test]
    fn mod_root_outside_the_game_fails_registry_validation() {
        let synth = Synthetic::new("synth", Some("../Mods"));
        assert!(validate_registry(&[&synth]).is_err());
    }

    #[test]
    fn configured_roots_prefer_the_profile_and_skip_unknown_or_modless_games() {
        let synth = Synthetic::new("synth", Some("Mods"));
        let bare = Synthetic::new("bare", None);
        let games: [&dyn Game; 3] = [&*nte::NTE, &synth, &bare];
        let config = json!({
            "game_path": "/legacy/nte",
            "games": {
                "nte": { "game_path": "/profile/nte" },
                "synth": { "game_path": "/profile/synth" },
                "bare": { "game_path": "/profile/bare" },
                "other": { "game_path": "/profile/other" },
            }
        });

        assert_eq!(
            configured_roots(&games, &config),
            vec![
                root("nte", Path::new("/profile/nte")),
                root("synth", Path::new("/profile/synth")),
            ]
        );
    }

    #[test]
    fn legacy_game_path_only_fills_a_missing_nte_profile() {
        let synth = Synthetic::new("synth", Some("Mods"));
        let games: [&dyn Game; 2] = [&*nte::NTE, &synth];

        let config = json!({ "game_path": "/legacy/nte", "games": {} });
        assert_eq!(
            configured_roots(&games, &config),
            vec![root("nte", Path::new("/legacy/nte"))]
        );

        let cleared =
            json!({ "game_path": "/legacy/nte", "games": { "nte": { "game_path": "" } } });
        assert_eq!(configured_roots(&games, &cleared), []);
    }

    #[test]
    fn game_root_arg_round_trips() {
        let original = root("nte", Path::new("/games/NTE=x"));
        assert_eq!(GameRoot::parse_arg(&original.to_arg()).unwrap(), original);
        assert!(GameRoot::parse_arg("nte=").is_err());
        assert!(GameRoot::parse_arg("../x=/games").is_err());
    }

    #[test]
    fn multiple_profiles_yield_one_target_each() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        let b = Synthetic::new("beta", Some("Data/Mods"));
        let games: [&dyn Game; 2] = [&a, &b];
        tmp.file("alpha/Mods/x.pak", b"a");
        tmp.file("beta/Data/Mods/x.pak", b"b");

        let scope = mod_targets(
            &games,
            &[
                root("alpha", &tmp.path("alpha")),
                root("beta", &tmp.path("beta")),
            ],
        );

        assert!(scope.rejected.is_empty(), "{:?}", scope.rejected);
        let found: Vec<_> = scope
            .targets
            .iter()
            .map(|t| (t.game_id.to_string(), t.path.clone()))
            .collect();
        assert_eq!(
            found,
            vec![
                ("alpha".into(), tmp.path("alpha/Mods")),
                ("beta".into(), tmp.path("beta/Data/Mods")),
            ]
        );
    }

    #[test]
    fn missing_mod_folders_and_game_folders_are_skipped() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        tmp.dir("alpha");

        let scope = mod_targets(
            &[&a],
            &[
                root("alpha", &tmp.path("alpha")),
                root("alpha", &tmp.path("gone")),
            ],
        );

        assert_eq!(scope.targets, []);
        assert_eq!(scope.rejected, Vec::<String>::new());
    }

    #[test]
    fn unknown_modless_and_relative_roots_are_rejected() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        let bare = Synthetic::new("bare", None);
        tmp.dir("alpha/Mods");

        let scope = mod_targets(
            &[&a, &bare],
            &[
                root("other", &tmp.path("alpha")),
                root("bare", &tmp.path("alpha")),
                root("alpha", Path::new("relative/alpha")),
            ],
        );

        assert_eq!(scope.targets, []);
        assert_eq!(scope.rejected.len(), 3, "{:?}", scope.rejected);
    }

    #[test]
    fn protected_game_roots_are_rejected() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let a = Synthetic::new("alpha", Some("Mods"));

        let scope = mod_targets(&[&a], &[root("alpha", &home)]);

        assert_eq!(scope.targets, []);
        assert_eq!(scope.rejected.len(), 1);
    }

    #[test]
    fn traversal_out_of_the_game_folder_is_rejected() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("../outside"));
        tmp.dir("alpha");
        tmp.dir("outside");

        let scope = mod_targets(&[&a], &[root("alpha", &tmp.path("alpha"))]);

        assert_eq!(scope.targets, []);
        assert_eq!(scope.rejected.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn shared_physical_folders_are_deduplicated() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        let b = Synthetic::new("beta", Some("Mods"));
        tmp.dir("shared/Mods");
        std::os::unix::fs::symlink(tmp.path("shared"), tmp.path("link")).unwrap();

        let scope = mod_targets(
            &[&a, &b],
            &[
                root("alpha", &tmp.path("shared")),
                root("beta", &tmp.path("link")),
            ],
        );

        assert!(scope.rejected.is_empty(), "{:?}", scope.rejected);
        assert_eq!(scope.targets.len(), 1);
        assert_eq!(scope.targets[0].game_id, id("alpha"));
        assert_eq!(scope.targets[0].path, tmp.path("shared/Mods"));
    }

    #[cfg(unix)]
    #[test]
    fn linked_mod_folders_are_rejected() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        let b = Synthetic::new("beta", Some("Data/Mods"));
        tmp.dir("alpha");
        tmp.dir("elsewhere/Mods");
        std::os::unix::fs::symlink(tmp.path("elsewhere/Mods"), tmp.path("alpha/Mods")).unwrap();
        tmp.dir("beta");
        std::os::unix::fs::symlink(tmp.path("elsewhere"), tmp.path("beta/Data")).unwrap();

        let scope = mod_targets(
            &[&a, &b],
            &[
                root("alpha", &tmp.path("alpha")),
                root("beta", &tmp.path("beta")),
            ],
        );

        assert_eq!(scope.targets, []);
        assert_eq!(scope.rejected.len(), 2, "{:?}", scope.rejected);
    }

    #[cfg(unix)]
    #[test]
    fn revalidation_refuses_a_target_replaced_after_planning() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        tmp.dir("alpha/Mods");
        tmp.file("precious/keep.txt", b"keep");
        let scope = mod_targets(&[&a], &[root("alpha", &tmp.path("alpha"))]);
        let target = scope.targets[0].clone();
        target.revalidate(&[&a]).unwrap();

        fs::remove_dir(tmp.path("alpha/Mods")).unwrap();
        std::os::unix::fs::symlink(tmp.path("precious"), tmp.path("alpha/Mods")).unwrap();

        assert!(target.revalidate(&[&a]).is_err());
        assert!(tmp.path("precious/keep.txt").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn revalidation_refuses_a_replaced_game_root() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        tmp.dir("real/Mods");
        tmp.dir("other/Mods");
        std::os::unix::fs::symlink(tmp.path("real"), tmp.path("alpha")).unwrap();
        let scope = mod_targets(&[&a], &[root("alpha", &tmp.path("alpha"))]);
        let target = scope.targets[0].clone();

        fs::remove_file(tmp.path("alpha")).unwrap();
        std::os::unix::fs::symlink(tmp.path("other"), tmp.path("alpha")).unwrap();

        assert!(target.revalidate(&[&a]).is_err());
    }

    #[test]
    fn backup_puts_each_game_under_its_own_prefix() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        let b = Synthetic::new("beta", Some("Mods"));
        tmp.file("alpha/Mods/same.pak", b"from alpha");
        tmp.file("beta/Mods/same.pak", b"from beta");
        tmp.file("beta/Mods/nested/deep.pak", b"deep");
        let scope = mod_targets(
            &[&a, &b],
            &[
                root("alpha", &tmp.path("alpha")),
                root("beta", &tmp.path("beta")),
            ],
        );
        let archive = tmp.path("backup/mods.zip");

        backup_mods(&scope.targets, &archive, |_, _, _| {}).unwrap();

        let file = |name: &str, size| shared::archive::ZippedFile {
            name: name.into(),
            size,
        };
        verify_zip(
            &archive,
            &[
                file("alpha/same.pak", 10),
                file("beta/nested/deep.pak", 4),
                file("beta/same.pak", 9),
            ],
        )
        .unwrap();
        assert!(!archive.with_extension("zip.part").exists());
    }

    #[test]
    fn repeated_games_get_distinct_prefixes() {
        let target = |game: &str| ModTarget {
            game_id: id(game),
            configured_root: PathBuf::new(),
            root: PathBuf::new(),
            path: PathBuf::new(),
        };
        assert_eq!(
            archive_prefixes(&[target("nte"), target("synth"), target("nte"), target("nte")]),
            vec!["nte", "synth", "nte-2", "nte-3"]
        );
    }

    #[test]
    fn failed_backup_leaves_no_archive_and_keeps_mods() {
        let tmp = TestDir::new();
        let a = Synthetic::new("alpha", Some("Mods"));
        tmp.file("alpha/Mods/x.pak", b"x");
        let scope = mod_targets(&[&a], &[root("alpha", &tmp.path("alpha"))]);
        tmp.file("blocked", b"not a folder");
        let archive = tmp.path("blocked/mods.zip");

        assert!(backup_mods(&scope.targets, &archive, |_, _, _| {}).is_err());
        assert!(!archive.exists());
        assert!(tmp.path("alpha/Mods/x.pak").is_file());
    }

    #[test]
    fn verification_rejects_an_archive_missing_files() {
        let tmp = TestDir::new();
        let src = tmp.dir("src");
        tmp.file("src/a.pak", b"a");
        let archive = tmp.path("a.zip");
        let mut files = zip_directories(&[("", &src)], &archive, |_, _, _| {}).unwrap();
        verify_zip(&archive, &files).unwrap();

        files.push(shared::archive::ZippedFile {
            name: "b.pak".into(),
            size: 1,
        });
        assert!(verify_zip(&archive, &files).is_err());
    }

    fn names(paths: &[PathBuf], base: &Path) -> Vec<String> {
        let mut out: Vec<String> = paths
            .iter()
            .map(|p| {
                p.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        out.sort();
        out
    }

    #[test]
    fn bin_scope_deletes_registered_payloads_and_the_legacy_allowlist_only() {
        let tmp = TestDir::new();
        let synth = Synthetic::new("synth", Some("Mods"));
        let games: [&dyn Game; 2] = [&*nte::NTE, &synth];
        tmp.file("Bin/nte/Everlight.asi", b"");
        tmp.file("Bin/nte/Lua/user.lua", b"");
        tmp.file("Bin/synth/core.dll", b"");
        tmp.file("Bin/stranger/core.dll", b"");
        tmp.file("Bin/Everlight.asi", b"");
        tmp.file("Bin/cutils.dll", b"");
        tmp.file("Bin/Plugins/chksum.asi", b"");
        tmp.file("Bin/Addons/HideUID/hideUID.auadd", b"");
        tmp.file("Bin/ffmpeg/ffmpeg.exe", b"");
        tmp.file("Bin/notes.txt", b"");
        tmp.file("Bin/listed/tool.exe", b"");

        let scope = bin_scope(&tmp.0, &games, &[PathBuf::from("listed/tool.exe")]);
        let bin = tmp.path("Bin");

        assert_eq!(
            names(&scope.trees, &bin),
            ["Addons", "Plugins", "nte", "synth"]
        );
        assert_eq!(names(&scope.files, &bin), ["Everlight.asi", "cutils.dll"]);
        assert_eq!(
            names(&scope.kept, &bin),
            ["ffmpeg", "notes.txt", "stranger"]
        );
    }

    #[test]
    fn bin_scope_without_signature_keeps_the_flat_layout() {
        let tmp = TestDir::new();
        tmp.file("Bin/Plugins/chksum.asi", b"");
        tmp.file("Bin/AuroraEngine.dll", b"");
        tmp.dir("Bin/nte/Lua");

        let scope = bin_scope(&tmp.0, &[&*nte::NTE], &[]);
        let bin = tmp.path("Bin");

        assert_eq!(scope.trees, Vec::<PathBuf>::new());
        assert_eq!(scope.files, Vec::<PathBuf>::new());
        assert_eq!(
            names(&scope.kept, &bin),
            ["AuroraEngine.dll", "Plugins", "nte"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn bin_scope_never_follows_links() {
        let tmp = TestDir::new();
        tmp.file("outside/Everlight.asi", b"");
        tmp.file("Bin/Everlight.asi", b"");
        std::os::unix::fs::symlink(tmp.path("outside"), tmp.path("Bin/nte")).unwrap();
        std::os::unix::fs::symlink(tmp.path("outside"), tmp.path("Bin/Plugins")).unwrap();

        let scope = bin_scope(&tmp.0, &[&*nte::NTE], &[]);
        let bin = tmp.path("Bin");

        assert_eq!(scope.trees, Vec::<PathBuf>::new());
        assert_eq!(names(&scope.kept, &bin), ["Plugins", "nte"]);
    }

    #[test]
    fn data_dir_without_unknown_games_is_removed_whole() {
        let tmp = TestDir::new();
        let data = tmp.path("Aurora");
        tmp.file(
            "Aurora/UserData/config.json",
            br#"{"games":{"nte":{"game_path":"x"}}}"#,
        );
        tmp.file("Aurora/UserData/ThirdParty/nte/Modules/a.asi", b"");
        tmp.file("Aurora/UserData/ThirdParty/Modules/a.asi", b"");

        let kept = remove_data_dir(&data, &[&*nte::NTE]).unwrap();

        assert_eq!(kept, Vec::<PathBuf>::new());
        assert!(!data.exists());
    }

    #[test]
    fn data_dir_keeps_unknown_profiles_and_their_folders() {
        let tmp = TestDir::new();
        let data = tmp.path("Aurora");
        tmp.file(
            "Aurora/UserData/config.json",
            br#"{"language":"en","game_path":"x","games":{"nte":{"game_path":"x"},"future":{"game_path":"y","extra":[1]}}}"#,
        );
        tmp.file("Aurora/UserData/ThirdParty/nte/Modules/a.asi", b"");
        tmp.file("Aurora/UserData/ThirdParty/future/Modules/b.asi", b"keep");
        tmp.file("Aurora/UserData/ThirdParty/Modules/a.asi", b"");
        tmp.file("Aurora/UserData/cache.bin", b"");
        tmp.file("Aurora/Logs/aurora.log", b"");

        let kept = remove_data_dir(&data, &[&*nte::NTE]).unwrap();

        assert_eq!(
            names(&kept, &data),
            ["UserData/ThirdParty/future", "UserData/config.json"]
        );
        let config: Value = serde_json::from_str(
            &fs::read_to_string(tmp.path("Aurora/UserData/config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            config,
            json!({ "games": { "future": { "game_path": "y", "extra": [1] } } })
        );
        assert_eq!(
            fs::read(tmp.path("Aurora/UserData/ThirdParty/future/Modules/b.asi")).unwrap(),
            b"keep"
        );
        assert!(!tmp.path("Aurora/UserData/ThirdParty/nte").exists());
        assert!(!tmp.path("Aurora/UserData/ThirdParty/Modules").exists());
        assert!(!tmp.path("Aurora/UserData/cache.bin").exists());
        assert!(!tmp.path("Aurora/Logs").exists());
    }

    #[test]
    fn synthetic_game_data_is_removed_when_registered() {
        let tmp = TestDir::new();
        let synth = Synthetic::new("synth", Some("Mods"));
        let data = tmp.path("Aurora");
        tmp.file(
            "Aurora/UserData/config.json",
            br#"{"games":{"synth":{"game_path":"y"}}}"#,
        );
        tmp.file("Aurora/UserData/ThirdParty/synth/x", b"");

        assert_eq!(
            remove_data_dir(&data, &[&*nte::NTE, &synth]).unwrap(),
            Vec::<PathBuf>::new()
        );
        assert!(!data.exists());

        tmp.file(
            "Aurora/UserData/config.json",
            br#"{"games":{"synth":{"game_path":"y"}}}"#,
        );
        tmp.file("Aurora/UserData/ThirdParty/synth/x", b"");
        let kept = remove_data_dir(&data, &[&*nte::NTE]).unwrap();
        assert_eq!(
            names(&kept, &data),
            ["UserData/ThirdParty/synth", "UserData/config.json"]
        );
    }
}
