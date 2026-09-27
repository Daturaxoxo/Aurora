use std::fmt;
use anyhow::Result;
use std::path::PathBuf;
use shared::classes::games::{InstallationFacts, identity::GameId, payload::ResolvedPayloadLayout};
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommandTag {
    pub game_id: GameId,
    pub generation: u64,
}

impl fmt::Display for CommandTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.game_id, self.generation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSettings {
    pub engine_method: i64,
    pub ignore_checksum: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonSelection {
    pub config_key: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSelection {
    pub path: PathBuf,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeploymentSelection {
    pub addons: Vec<AddonSelection>,
    pub modules: Vec<ModuleSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InjectedPluginRecord {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordChanges {
    pub added: Vec<InjectedPluginRecord>,
    pub removed: Vec<InjectedPluginRecord>,
}

impl RecordChanges {
    pub const fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineInput {
    pub tag: CommandTag,
    pub installation: Option<InstallationFacts>,
    pub settings: EngineSettings,
    pub payload: ResolvedPayloadLayout,
    pub deployment: DeploymentSelection,
    pub injected_plugins: Vec<InjectedPluginRecord>,
}

#[derive(Debug)]
pub struct OperationOutcome<T> {
    pub tag: CommandTag,
    pub result: Result<T>,
    pub records: RecordChanges,
}

impl<T> OperationOutcome<T> {
    pub fn ok(tag: CommandTag, value: T) -> Self {
        Self {
            tag,
            result: Ok(value),
            records: RecordChanges::default(),
        }
    }

    pub fn failed(tag: CommandTag, error: anyhow::Error) -> Self {
        Self {
            tag,
            result: Err(error),
            records: RecordChanges::default(),
        }
    }

    #[must_use]
    pub fn with_records(mut self, records: RecordChanges) -> Self {
        self.records = records;
        self
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> OperationOutcome<U> {
        OperationOutcome {
            tag: self.tag,
            result: self.result.map(f),
            records: self.records,
        }
    }
}
