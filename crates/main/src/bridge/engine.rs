use anyhow::{Result, anyhow};
use backend::engine::contract::{
    AddonSelection, CommandTag, DeploymentSelection, EngineInput, EngineSettings,
    InjectedPluginRecord, LaunchInput, ModuleSelection, Readiness, RecordChanges, SanitizeInput,
    ValidateInput,
};
use backend::handler::EngineCommand;
use log::*;
use serde_json::Value;
use shared::classes::games::{
    Game, InstallationFacts,
    capabilities::AddonSupport,
    launch::{LaunchPlatform, LaunchRequest},
    nte::version::StartMethod,
    payload::{PayloadResolutionMode, PayloadRoots, ResolvedPayloadLayout, resolve_payload},
};
use shared::config::{self, GameProfile, ProfileKey, ProfilePatch, key};
use shared::utils;
use crate::runtime::{self, Operation};
pub fn session_active() -> bool {
    runtime::get().is_ok_and(runtime::GameRuntime::session_active)
}

pub fn launch() -> Result<()> {
    let runtime = runtime::get()?;
    let guard = runtime.try_begin_session()?;
    let context = guard.context();
    let game = context.game();
    let installation = context
        .installation()
        .filter(|_| context.readiness() == Readiness::Ready)
        .ok_or_else(|| anyhow!("Engine not initialized, set a valid game path in settings"))?;
    let name = game.descriptor().display_name;
    let launcher = game
        .launcher()
        .ok_or_else(|| anyhow!("{name} cannot be launched"))?;

    let profile = config::read_profile(context.id())?;
    let start_method = StartMethod::decode(&profile.value(ProfileKey::StartMethod, game));
    info!("Start method: {start_method}");
    let plan = launcher
        .launch_plan(&LaunchRequest {
            installation,
            start_method: start_method.id(),
            platform: LaunchPlatform::host(),
        })
        .map_err(|e| anyhow!("Cannot launch {name}: {e}"))?;

    let input = LaunchInput {
        tag: context.tag(),
        plan,
        deployment: deployment(game, &profile),
        injected_plugins: records(&profile),
        proton_args: text(&profile, game, ProfileKey::ProtonArgs),
        proton_version: text(&profile, game, ProfileKey::ProtonVersion),
        proton_custom_path: text(&profile, game, ProfileKey::ProtonCustomPath),
    };
    runtime.submit(Operation::Launch, guard, EngineCommand::Launch(input))
}

pub fn validate() -> Result<()> {
    let runtime = runtime::get()?;
    let guard = runtime.try_begin_mutation()?;
    let profile = config::read_profile(guard.context().id())?;
    let input = ValidateInput {
        tag: guard.context().tag(),
        deployment: deployment(guard.context().game(), &profile),
    };
    runtime.submit(Operation::Validate, guard, EngineCommand::Validate(input))
}

pub fn sanitize() -> Result<()> {
    let runtime = runtime::get()?;
    let guard = runtime.try_begin_mutation()?;
    let profile = config::read_profile(guard.context().id())?;
    let input = SanitizeInput {
        tag: guard.context().tag(),
        stop_processes: true,
        injected_plugins: records(&profile),
    };
    runtime.submit(Operation::Sanitize, guard, EngineCommand::Sanitize(input))
}

pub fn kill() -> Result<()> {
    let runtime = runtime::get()?;
    let guard = runtime.try_begin_mutation()?;
    let tag = guard.context().tag();
    runtime.submit(Operation::Kill, guard, EngineCommand::KillProcesses(tag))
}

pub fn engine_input(
    game: &dyn Game,
    tag: CommandTag,
    installation: Option<InstallationFacts>,
    payload: ResolvedPayloadLayout,
) -> Result<EngineInput> {
    let profile = config::read_profile(&tag.game_id)?;
    Ok(EngineInput {
        injected_plugins: records(&profile),
        tag,
        installation,
        settings: settings(&profile, game)?,
        payload,
        deployment: deployment(game, &profile),
    })
}

pub fn persist_records(tag: &CommandTag, changes: &RecordChanges) {
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

fn records(profile: &GameProfile) -> Vec<InjectedPluginRecord> {
    parse_records(profile.get(ProfileKey::InjectedPlugins))
}

pub fn start_method(game: &dyn Game) -> StartMethod {
    let raw = config::read_profile(&game.descriptor().id)
        .map_or_else(|_| game.profile_default(ProfileKey::StartMethod), |p| p.value(ProfileKey::StartMethod, game));
    StartMethod::decode(&raw)
}

fn text(profile: &GameProfile, game: &dyn Game, key: ProfileKey) -> String {
    profile.value(key, game).as_str().unwrap_or_default().to_string()
}

fn settings(profile: &GameProfile, game: &dyn Game) -> Result<EngineSettings> {
    let raw = profile.value(ProfileKey::EngineMethod, game);
    let engine_method = match raw.as_i64() {
        Some(v) => v,
        None => raw.as_str().unwrap_or("0").parse::<i64>()?,
    };
    Ok(EngineSettings {
        engine_method,
        ignore_checksum: profile.value(ProfileKey::IgnoreChecksum, game).as_bool().unwrap_or(false),
    })
}

fn deployment(game: &dyn Game, profile: &GameProfile) -> DeploymentSelection {
    let addons = game
        .addons()
        .map_or_default(AddonSupport::shipped_addons)
        .iter()
        .map(|addon| AddonSelection {
            config_key: addon.config_key.to_string(),
            enabled: ProfileKey::from_key(addon.config_key)
                .and_then(|key| profile.get(key))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect();

    // TODO(lane A, a5-1): the modules page still writes these to the legacy keys
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

pub fn payload(game: &dyn Game) -> Result<ResolvedPayloadLayout> {
    let bin = utils::get_bin_path().ok_or_else(|| anyhow!("Could not resolve bin path"))?;
    resolve_payload(
        game,
        &PayloadRoots::new(bin),
        PayloadResolutionMode::LegacyFlatOnly,
    )
}
