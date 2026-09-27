use serde_json::{Map, Value};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::time::Duration;

use super::{
    CONFIG_LOCK, LOCK_TIMEOUT, default_value, get_userdata_path, key, load_raw_at,
    lock_with_timeout, write_atomic,
};
use crate::classes::games::{Game, identity::GameId};
pub(super) const GAMES: &str = "games";

macro_rules! config_keys {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $key:ident,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)*
        }

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => key::$key,)*
                }
            }

            pub fn from_key(raw: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|k| k.as_str() == raw)
            }
        }
    };
}

config_keys! {
    GlobalKey {
        SelectedGame => SELECTED_GAME,
        Language => LANGUAGE,
        DevMode => DEV_MODE,
        DiscordRpc => DISCORD_RPC,
        ExportConsole => EXPORT_CONSOLE,
        UiScaling => UI_SCALING,
        UiMinimization => UI_MINIMIZATION,
        Theme => THEME,
        IconPack => ICON_PACK,
        ShowNsfwMods => SHOW_NSFW_MODS,
        GbNsfw => GB_NSFW,
        TelemetryOptOut => TELEMETRY_OPT_OUT,
        ModmngViewGrid => MODMNG_VIEW_GRID,
        ModmngSort => MODMNG_SORT,
        ProtonCustomPaths => PROTON_CUSTOM_PATHS,
        AppLocation => APP_LOCATION,
        QuickStartCreated => QUICK_START_CREATED,
        DesktopEntry => DESKTOP_ENTRY,
        DesktopEntryPrompted => DESKTOP_ENTRY_PROMPTED,
    }
}

config_keys! {
    ProfileKey {
        GamePath => GAME_PATH,
        EngineMethod => ENGINE_METHOD,
        StartMethod => START_METHOD,
        CensorshipRemove => CENSORSHIP_REMOVE,
        NoDriveLine => NO_DRIVE_LINE,
        HideUid => HIDE_UID,
        HideNotifDots => HIDE_NOTIF_DOTS,
        UiModPack => UI_MOD_PACK,
        AddonAutoUpdates => ADDON_AUTO_UPDATES,
        CustomAddons => CUSTOM_ADDONS,
        CustomAddonsToggled => CUSTOM_ADDONS_TOGGLED,
        ModmngNotes => MODMNG_NOTES,
        ModmngDisplayNames => MODMNG_DISPLAY_NAMES,
        ModNotes => MOD_NOTES,
        ModDisplayNames => MOD_DISPLAY_NAMES,
        ScreenshotFavorites => SCREENSHOT_FAVORITES,
        ProtonArgs => PROTON_ARGS,
        ProtonVersion => PROTON_VERSION,
        ProtonCustomPath => PROTON_CUSTOM_PATH,
        IgnoreChecksum => IGNORE_CHECKSUM,
        LaunchArgs => LAUNCH_ARGS,
        InjectedPlugins => INJECTED_PLUGINS,
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Locked(PathBuf),
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    Unreadable(anyhow::Error),
    Serialize(serde_json::Error),
    Shape(String),
    Conflict(String),
    Migration(String),
}

impl ConfigError {
    pub(super) fn io(action: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_path_buf(),
            source,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked(path) => write!(f, "could not lock {}", path.display()),
            Self::Io {
                action,
                path,
                source,
            } => write!(f, "could not {action} {}: {source}", path.display()),
            Self::Unreadable(e) => write!(f, "the config could not be read: {e:#}"),
            Self::Serialize(e) => write!(f, "the config could not be serialized: {e}"),
            Self::Shape(field) => write!(f, "{field} is not a JSON object"),
            Self::Conflict(field) => {
                write!(
                    f,
                    "{field} changed after it was committed, Aurora wont roll it back"
                )
            }
            Self::Migration(reason) => write!(f, "migration failed: {reason}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Serialize(e) => Some(e),
            _ => None,
        }
    }
}

pub(super) struct Store {
    root: PathBuf,
    lock_timeout: Duration,
}

impl Store {
    pub(super) fn system() -> Self {
        Self {
            root: get_userdata_path(),
            lock_timeout: LOCK_TIMEOUT,
        }
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn config_path(&self) -> PathBuf {
        self.root.join("config.json")
    }

    pub(super) fn lock_path(&self) -> PathBuf {
        self.config_path().with_extension("json.lock")
    }

    fn locked<T>(&self, f: impl FnOnce(&Path) -> Result<T, ConfigError>) -> Result<T, ConfigError> {
        fs::create_dir_all(&self.root).map_err(|e| ConfigError::io("create", &self.root, e))?;

        let _guard = CONFIG_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let lock = self.lock_path();
        let _cross_guard =
            lock_with_timeout(&lock, self.lock_timeout).ok_or(ConfigError::Locked(lock))?;

        f(&self.config_path())
    }

    pub(super) fn read(&self) -> Result<Map<String, Value>, ConfigError> {
        self.locked(|path| load_raw_at(path).map_err(ConfigError::Unreadable))
    }

    pub(super) fn transact<T>(
        &self,
        f: impl FnOnce(&mut Map<String, Value>) -> Result<(T, bool), ConfigError>,
    ) -> Result<T, ConfigError> {
        self.locked(|path| {
            let mut data = load_raw_at(path).map_err(ConfigError::Unreadable)?;
            let (out, changed) = f(&mut data)?;
            if changed {
                write_atomic(path, &data)?;
            }
            Ok(out)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Field {
    Global(GlobalKey),
    Profile(GameId, ProfileKey),
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Global(k) => write!(f, "{}", k.as_str()),
            Self::Profile(game, k) => write!(f, "{GAMES}.{game}.{}", k.as_str()),
        }
    }
}

pub(super) fn profile_of<'a>(
    data: &'a Map<String, Value>,
    game: &GameId,
) -> Result<Option<&'a Map<String, Value>>, ConfigError> {
    let Some(games) = data.get(GAMES) else {
        return Ok(None);
    };
    let games = games
        .as_object()
        .ok_or_else(|| ConfigError::Shape(GAMES.to_string()))?;
    match games.get(game.as_str()) {
        None => Ok(None),
        Some(Value::Object(profile)) => Ok(Some(profile)),
        Some(_) => Err(ConfigError::Shape(format!("{GAMES}.{game}"))),
    }
}

pub(super) fn profile_mut<'a>(
    data: &'a mut Map<String, Value>,
    game: &GameId,
) -> Result<&'a mut Map<String, Value>, ConfigError> {
    let games = data
        .entry(GAMES)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| ConfigError::Shape(GAMES.to_string()))?;
    games
        .entry(game.as_str())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| ConfigError::Shape(format!("{GAMES}.{game}")))
}

fn field_get(data: &Map<String, Value>, field: &Field) -> Result<Option<Value>, ConfigError> {
    match field {
        Field::Global(k) => Ok(data.get(k.as_str()).cloned()),
        Field::Profile(game, k) => {
            Ok(profile_of(data, game)?.and_then(|p| p.get(k.as_str()).cloned()))
        }
    }
}

fn field_set(
    data: &mut Map<String, Value>,
    field: &Field,
    value: Option<Value>,
) -> Result<(), ConfigError> {
    match (field, value) {
        (Field::Global(k), Some(v)) => {
            data.insert(k.as_str().to_string(), v);
        }
        (Field::Global(k), None) => {
            data.remove(k.as_str());
        }
        (Field::Profile(game, k), Some(v)) => {
            profile_mut(data, game)?.insert(k.as_str().to_string(), v);
        }
        (Field::Profile(game, k), None) => {
            if profile_of(data, game)?.is_none() {
                return Ok(());
            }
            let profile = profile_mut(data, game)?;
            profile.remove(k.as_str());
            if profile.is_empty()
                && let Some(Value::Object(games)) = data.get_mut(GAMES)
            {
                games.remove(game.as_str());
                if games.is_empty() {
                    data.remove(GAMES);
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameProfile {
    game: GameId,
    fields: Map<String, Value>,
}

impl GameProfile {
    pub const fn game(&self) -> &GameId {
        &self.game
    }

    pub fn get(&self, key: ProfileKey) -> Option<&Value> {
        self.fields.get(key.as_str())
    }

    pub fn value(&self, key: ProfileKey, game: &dyn Game) -> Value {
        debug_assert_eq!(&game.descriptor().id, &self.game);
        self.get(key)
            .cloned()
            .unwrap_or_else(|| game.profile_default(key))
    }

    pub const fn raw(&self) -> &Map<String, Value> {
        &self.fields
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfilePatch {
    entries: Vec<(ProfileKey, Option<Value>)>,
}

impl ProfilePatch {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn set(mut self, key: ProfileKey, value: impl Into<Value>) -> Self {
        self.entries.push((key, Some(value.into())));
        self
    }

    #[must_use]
    pub fn remove(mut self, key: ProfileKey) -> Self {
        self.entries.push((key, None));
        self
    }

    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigChange {
    entries: Vec<(Field, Option<Value>)>,
}

impl ConfigChange {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn set_global(mut self, key: GlobalKey, value: impl Into<Value>) -> Self {
        self.entries.push((Field::Global(key), Some(value.into())));
        self
    }

    #[must_use]
    pub fn remove_global(mut self, key: GlobalKey) -> Self {
        self.entries.push((Field::Global(key), None));
        self
    }

    #[must_use]
    pub fn profile(mut self, game: &GameId, patch: ProfilePatch) -> Self {
        self.entries.extend(
            patch
                .entries
                .into_iter()
                .map(|(k, v)| (Field::Profile(game.clone(), k), v)),
        );
        self
    }

    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
struct UndoEntry {
    field: Field,
    before: Option<Value>,
    after: Option<Value>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigUndo {
    entries: Vec<UndoEntry>,
}

impl ConfigUndo {
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub(super) fn commit_in(store: &Store, change: &ConfigChange) -> Result<ConfigUndo, ConfigError> {
    store.transact(|data| {
        let mut undo = ConfigUndo::default();

        for (field, value) in &change.entries {
            let current = field_get(data, field)?;
            if current == *value {
                continue;
            }
            field_set(data, field, value.clone())?;

            match undo.entries.iter_mut().find(|e| e.field == *field) {
                Some(entry) => entry.after.clone_from(value),
                None => undo.entries.push(UndoEntry {
                    field: field.clone(),
                    before: current,
                    after: value.clone(),
                }),
            }
        }

        undo.entries.retain(|e| e.before != e.after);
        let changed = !undo.is_empty();
        Ok((undo, changed))
    })
}

pub(super) fn rollback_in(store: &Store, undo: &ConfigUndo) -> Result<(), ConfigError> {
    if undo.is_empty() {
        return Ok(());
    }

    store.transact(|data| {
        for entry in &undo.entries {
            if field_get(data, &entry.field)? != entry.after {
                return Err(ConfigError::Conflict(entry.field.to_string()));
            }
        }
        for entry in undo.entries.iter().rev() {
            field_set(data, &entry.field, entry.before.clone())?;
        }
        Ok(((), true))
    })
}

pub(super) fn read_profile_in(store: &Store, game: &GameId) -> Result<GameProfile, ConfigError> {
    let data = store.read()?;
    Ok(GameProfile {
        game: game.clone(),
        fields: profile_of(&data, game)?.cloned().unwrap_or_default(),
    })
}

pub(super) fn read_global_in(store: &Store, key: GlobalKey) -> Result<Value, ConfigError> {
    let data = store.read()?;
    Ok(data
        .get(key.as_str())
        .cloned()
        .unwrap_or_else(|| default_value(key.as_str())))
}

pub fn read_global(key: GlobalKey) -> Result<Value, ConfigError> {
    read_global_in(&Store::system(), key)
}

pub fn read_profile(game: &GameId) -> Result<GameProfile, ConfigError> {
    read_profile_in(&Store::system(), game)
}

pub fn update_profile(game: &GameId, update: &ProfilePatch) -> Result<(), ConfigError> {
    commit_change(&ConfigChange::new().profile(game, update.clone())).map(drop)
}

pub fn commit_change(change: &ConfigChange) -> Result<ConfigUndo, ConfigError> {
    commit_in(&Store::system(), change)
}

pub fn rollback_change(undo: &ConfigUndo) -> Result<(), ConfigError> {
    rollback_in(&Store::system(), undo)
}

pub(super) fn modules_path_under(root: &Path, game: &GameId) -> PathBuf {
    root.join("ThirdParty").join(game.as_str()).join("Modules")
}

pub fn game_modules_path(game: &GameId) -> PathBuf {
    modules_path_under(&get_userdata_path(), game)
}
