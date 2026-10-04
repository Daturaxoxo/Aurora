use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError, mpsc};
use anyhow::{Result, anyhow};
use backend::engine::contract::{
    AddonSelection, CommandTag, DeploymentSelection, EngineInput, EngineSettings,
    InjectedPluginRecord, LaunchInput, ModuleSelection, RecordChanges, SanitizeInput,
    ValidateInput,
};
use backend::handler::{EngineCommand, EngineEvent, EngineHandle};
use log::*;
use serde_json::Value;
use shared::classes::games::{
    Game, InstallationFacts,
    capabilities::AddonSupport,
    launch::{LaunchPlatform, LaunchRequest},
    locate::locate_installation_root,
    nte::{NTE, version::StartMethod},
    payload::{PayloadResolutionMode, PayloadRoots, ResolvedPayloadLayout, resolve_payload},
};
use shared::config::{self, ProfileKey, ProfilePatch, key};
use shared::utils;
struct Current {
    tag: CommandTag,
    installation: Option<InstallationFacts>,
}
static ENGINE: OnceLock<EngineHandle> = OnceLock::new();
static CURRENT: Mutex<Option<Current>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static RECONFIGURE_PENDING: AtomicBool = AtomicBool::new(false);
static RECORDS: Mutex<()> = Mutex::new(());

// TODO(lane B, phase 4): use the runtime's selected game instead of NTE.
fn game() -> &'static dyn Game {
    &*NTE
}

pub fn start() -> Result<mpsc::Receiver<EngineEvent>> {
    let (handle, events) = EngineHandle::spawn();
    ENGINE
        .set(handle)
        .map_err(|_| anyhow!("the engine was already started"))?;
    Ok(events)
}

pub fn shutdown() -> Result<()> {
    handle()?.shutdown()
}

fn handle() -> Result<&'static EngineHandle> {
    ENGINE
        .get()
        .ok_or_else(|| anyhow!("Engine has not been started yet!"))
}

fn current() -> Result<(CommandTag, Option<InstallationFacts>)> {
    CURRENT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|c| (c.tag.clone(), c.installation.clone()))
        .ok_or_else(|| anyhow!("Engine not initialized, set a valid game path in settings"))
}

fn current_tag() -> Result<CommandTag> {
    current().map(|(tag, _)| tag)
}

pub fn installation_root() -> Option<PathBuf> {
    current()
        .ok()
        .and_then(|(_, facts)| facts)
        .map(|facts| facts.root().to_path_buf())
}

pub fn session_active() -> bool {
    SESSION_ACTIVE.load(Ordering::SeqCst)
}

pub fn session_ended() -> bool {
    SESSION_ACTIVE.store(false, Ordering::SeqCst);
    RECONFIGURE_PENDING.swap(false, Ordering::SeqCst)
}

pub fn configure() -> Result<()> {
    if session_active() {
        info!("A game session is running, the engine will be reconfigured once it closes");
        RECONFIGURE_PENDING.store(true, Ordering::SeqCst);
        return Ok(());
    }

    let game = game();
    let tag = CommandTag {
        game_id: game.descriptor().id.clone(),
        generation: GENERATION.fetch_add(1, Ordering::SeqCst) + 1,
    };

    let (installation, problem) = match locate_installation_root(game) {
        Ok(root) => match game.inspect_installation(&root) {
            Ok(facts) => (Some(facts), None),
            Err(e) => (None, Some(e)),
        },
        Err(e) => (None, Some(anyhow!("Game path not found: {e}"))),
    };

    let input = EngineInput {
        tag: tag.clone(),
        installation: installation.clone(),
        settings: settings()?,
        payload: payload()?,
        deployment: deployment(),
        injected_plugins: records()?,
    };

    *CURRENT.lock().unwrap_or_else(PoisonError::into_inner) = Some(Current { tag, installation });
    handle()?.send(EngineCommand::Configure(input))?;
    problem.map_or(Ok(()), Err)
}

pub fn launch() -> Result<()> {
    let (tag, installation) = current()?;
    let installation = installation
        .ok_or_else(|| anyhow!("Engine not initialized, set a valid game path in settings"))?;
    let launcher = game()
        .launcher()
        .ok_or_else(|| anyhow!("{} cannot be launched", tag.game_id))?;

    let start_method = StartMethod::decode(&config::get(key::START_METHOD));
    info!("Start method: {start_method}");
    let plan = launcher
        .launch_plan(&LaunchRequest {
            installation: &installation,
            start_method: start_method.id(),
            platform: LaunchPlatform::host(),
        })
        .map_err(|e| anyhow!("Cannot launch NTE: {e}"))?;

    let input = LaunchInput {
        tag,
        plan,
        deployment: deployment(),
        injected_plugins: records()?,
        proton_args: text(key::PROTON_ARGS),
        proton_version: text(key::PROTON_VERSION),
        proton_custom_path: text(key::PROTON_CUSTOM_PATH),
    };

    SESSION_ACTIVE.store(true, Ordering::SeqCst);
    handle()?
        .send(EngineCommand::Launch(input))
        .inspect_err(|_| {
            session_ended();
        })
}

pub fn validate() -> Result<()> {
    let input = ValidateInput {
        tag: current_tag()?,
        deployment: deployment(),
    };
    handle()?.send(EngineCommand::Validate(input))
}

pub fn sanitize() -> Result<()> {
    let input = SanitizeInput {
        tag: current_tag()?,
        stop_processes: true,
        injected_plugins: records()?,
    };
    handle()?.send(EngineCommand::Sanitize(input))
}

pub fn kill() -> Result<()> {
    handle()?.send(EngineCommand::KillProcesses(current_tag()?))
}

// TODO(lane B, b4-7): persist under the mutex guard
pub fn persist_records(tag: &CommandTag, changes: &RecordChanges) {
    if changes.is_empty() {
        return;
    }
    let _lock = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);

    let result = config::read_profile(&tag.game_id).and_then(|profile| {
        let records = changes.apply(&parse_records(profile.get(ProfileKey::InjectedPlugins)));
        let paths: Vec<String> = records
            .iter()
            .map(|r| r.path.to_string_lossy().into_owned())
            .collect();
        trace!("Recording {} injected plugin(s) for {tag}", paths.len());
        config::update_profile(
            &tag.game_id,
            &ProfilePatch::new().set(ProfileKey::InjectedPlugins, paths),
        )
    });
    if let Err(e) = result {
        error!("Could not save the injected plugins for {tag}: {e}");
    }
}

fn parse_records(value: Option<&Value>) -> Vec<InjectedPluginRecord> {
    value.and_then(Value::as_array).map_or_default(|list| {
        list.iter()
            .filter_map(Value::as_str)
            .map(|p| InjectedPluginRecord { path: p.into() })
            .collect()
    })
}

fn records() -> Result<Vec<InjectedPluginRecord>> {
    let profile = config::read_profile(&game().descriptor().id)?;
    Ok(parse_records(profile.get(ProfileKey::InjectedPlugins)))
}

// TODO(lane B, b5): the inputs below still read the legacy global keys because the UI writes them. switch to read_profile once the writers move to games.<id>.
fn text(key: &str) -> String {
    config::get(key).as_str().unwrap_or_default().to_string()
}

fn settings() -> Result<EngineSettings> {
    let raw = config::get(key::ENGINE_METHOD);
    let engine_method = match raw.as_i64() {
        Some(v) => v,
        None => raw.as_str().unwrap_or("0").parse::<i64>()?,
    };
    Ok(EngineSettings {
        engine_method,
        ignore_checksum: config::get(key::IGNORE_CHECKSUM).as_bool().unwrap_or(false),
    })
}

fn deployment() -> DeploymentSelection {
    let addons = game()
        .addons()
        .map_or_default(AddonSupport::shipped_addons)
        .iter()
        .map(|addon| AddonSelection {
            config_key: addon.config_key.to_string(),
            enabled: config::get(addon.config_key).as_bool().unwrap_or(false),
        })
        .collect();

    let enabled = config::get(key::CUSTOM_ADDONS_TOGGLED)
        .as_bool()
        .unwrap_or(false);
    let modules = config::get(key::CUSTOM_ADDONS)
        .as_array()
        .map_or_default(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(|p| ModuleSelection {
                    path: p.into(),
                    enabled,
                })
                .collect()
        });

    DeploymentSelection { addons, modules }
}

fn payload() -> Result<ResolvedPayloadLayout> {
    let bin = utils::get_bin_path().ok_or_else(|| anyhow!("Could not resolve bin path"))?;
    resolve_payload(
        game(),
        &PayloadRoots::new(bin),
        PayloadResolutionMode::LegacyFlatOnly,
    )
}
