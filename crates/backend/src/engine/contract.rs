use std::fmt;
use std::path::PathBuf;
use std::sync::mpsc;

use anyhow::Result;
use shared::classes::games::{
    InstallationFacts, identity::GameId, launch::LaunchPlan, payload::ResolvedPayloadLayout,
};

use crate::handler::EngineEvent;

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

impl DeploymentSelection {
    pub fn addon_enabled(&self, config_key: &str) -> bool {
        self.addons
            .iter()
            .any(|a| a.enabled && a.config_key == config_key)
    }
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

    pub fn remove(&mut self, record: InjectedPluginRecord) {
        self.added.retain(|r| *r != record);
        if !self.removed.contains(&record) {
            self.removed.push(record);
        }
    }

    pub fn add(&mut self, record: InjectedPluginRecord) {
        self.removed.retain(|r| *r != record);
        if !self.added.contains(&record) {
            self.added.push(record);
        }
    }

    pub fn extend(&mut self, later: Self) {
        later.removed.into_iter().for_each(|r| self.remove(r));
        later.added.into_iter().for_each(|r| self.add(r));
    }

    pub fn apply(&self, records: &[InjectedPluginRecord]) -> Vec<InjectedPluginRecord> {
        let mut out: Vec<InjectedPluginRecord> = Vec::with_capacity(records.len());
        for record in records.iter().chain(&self.added) {
            if !self.removed.contains(record) && !out.contains(record) {
                out.push(record.clone());
            }
        }
        out
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchInput {
    pub tag: CommandTag,
    pub plan: LaunchPlan,
    pub deployment: DeploymentSelection,
    pub injected_plugins: Vec<InjectedPluginRecord>,
    pub proton_args: String,
    pub proton_version: String,
    pub proton_custom_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateInput {
    pub tag: CommandTag,
    pub deployment: DeploymentSelection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizeInput {
    pub tag: CommandTag,
    pub stop_processes: bool,
    pub injected_plugins: Vec<InjectedPluginRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidationReport {
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionExit {
    GameExited,
    LauncherAbandoned,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationKind {
    Success,
    Warning,
    Error,
}

impl NotificationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
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

    pub const fn from_result(tag: CommandTag, result: Result<T>, records: RecordChanges) -> Self {
        Self {
            tag,
            result,
            records,
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

#[derive(Debug, Clone)]
pub struct EventSink {
    tag: CommandTag,
    tx: mpsc::Sender<EngineEvent>,
}

impl EventSink {
    pub const fn new(tag: CommandTag, tx: mpsc::Sender<EngineEvent>) -> Self {
        Self { tag, tx }
    }

    pub const fn tag(&self) -> &CommandTag {
        &self.tag
    }

    pub fn notify(&self, kind: NotificationKind, text: impl Into<String>) {
        self.tx
            .send(EngineEvent::Notification {
                tag: self.tag.clone(),
                kind,
                text: text.into(),
            })
            .ok();
    }
}

pub struct SessionHooks {
    pub events: EventSink,
    pub ended: Box<dyn FnOnce() + Send>,
}
