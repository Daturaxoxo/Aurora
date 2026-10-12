use crate::bridge::engine;
use crate::classes::logwindow;
use crate::classes::pages::modmanager::ModManagerHandler;
use crate::classes::toast::ToastHandler;
use crate::runtime::{self, GameContext, GameRuntime, Reconfigured};
use crate::{MainWindow, Tr, TrKey};
use backend::classes::addons::scale;
use backend::classes::launch_args;
use backend::classes::rpc::RPC;
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use serde_json::Value;
use shared::classes::games::nte::version::StartMethod;
use shared::classes::games::{
    EngineKind, Game, InstallationFacts, capabilities::ScaleSetting,
    locate::transitional_selected_game, platform::PlatformPaths,
};
use shared::config::{self, ConfigChange, ProfileKey, ProfilePatch, key};
use shared::pathfind::resolve_game_root;
use shared::utils::open_folder;
use slint::{ComponentHandle as _, Model as _};

#[derive(serde::Deserialize)]
#[allow(dead_code)]
struct LangEntry {
    name: String,
    code: String,
}

static LANGUAGES: Lazy<Vec<LangEntry>> = Lazy::new(|| {
    serde_json::from_str(include_str!(
        "../../../../../production/Langs/lang-codes.json"
    ))
    .expect("lang-codes.json is missing or malformed!")
});

pub const IGNORE_CHECKSUM_POPUP_ID: &str = "ignore-checksum";
pub const DIRECT_MODE_POPUP_ID: &str = "direct-mode";

const ABOUT_LINKS: &[(&str, &str)] = &[
    ("website", "https://getaurora.moe/"),
    ("discord", "https://discord.gg/565jfeYsbp"),
    ("github", "https://github.com/Daturaxoxo/Aurora"),
    ("docs", "https://docs.getaurora.moe"),
    (
        "launch-arguments",
        "https://docs.getaurora.moe/engine/launch-arguments",
    ),
    (
        "contributors",
        "https://github.com/Daturaxoxo/Aurora/graphs/contributors",
    ),
    (
        "license",
        "https://github.com/Daturaxoxo/Aurora/blob/main/LICENSE",
    ),
    ("terms", "https://getaurora.moe/terms"),
    ("patreon", "https://www.patreon.com/cw/AuroraLauncher"),
    ("kofi", "https://ko-fi.com/auroralauncher"),
];

pub struct SettingsHandler;

impl SettingsHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        info!("setup() called");
        Self::load(window);
        Self::bind(window);
        info!("setup() complete");
    }

    fn load(window: &slint::Weak<MainWindow>) {
        info!("load() started - reading config values");

        let Some(w) = window.upgrade() else {
            error!("load() failed - window handle is dead, cannot apply config to UI");
            return;
        };

        // General
        let raw_lang = config::get(key::LANGUAGE);
        let lang_code = raw_lang.as_str().unwrap_or("en").to_string();
        let lang_index = Self::code_to_index(&lang_code).unwrap_or(0);
        debug!("language: raw={raw_lang:?} → code={lang_code:?} → index={lang_index}");
        w.set_language_index(lang_index);

        let raw_minimization = config::get(key::UI_MINIMIZATION);
        let minimization = raw_minimization.as_bool().unwrap_or(true);
        debug!("interface_minimization: raw={raw_minimization:?} → {minimization}");
        w.set_interface_minimization(minimization);

        let raw_remember = config::get(key::REMEMBER_WINDOW_SIZE);
        let remember_window_size = raw_remember.as_bool().unwrap_or(true);
        debug!("remember_window_size: raw={raw_remember:?} → {remember_window_size}");
        w.set_remember_window_size(remember_window_size);

        let raw_rpc = config::get(key::DISCORD_RPC);
        let discord_rpc = raw_rpc.as_bool().unwrap_or(true);
        debug!("discord_rpc: raw={raw_rpc:?} → {discord_rpc}");
        w.set_discord_rpc(discord_rpc);
        if discord_rpc && let Err(e) = RPC.set_idle() {
            error!("could not set Discord RPC to idle: {e}");
        }

        // Launcher, until the runtime commits a game this shows the selected game's profile
        Self::load_game(&w, Self::current_game(), None);

        // Linux only
        w.set_is_linux(cfg!(target_os = "linux"));
        if cfg!(target_os = "linux") {
            let raw_entry = config::get(key::DESKTOP_ENTRY);
            let desktop_entry = raw_entry.as_bool().unwrap_or(false);
            debug!("desktop_entry: raw={raw_entry:?} → {desktop_entry}");
            w.set_desktop_entry(desktop_entry);
        }

        // Developer
        let raw_dev = config::get(key::DEV_MODE);
        let dev_mode = raw_dev.as_bool().unwrap_or(false);
        debug!("developer_mode: raw={raw_dev:?} → {dev_mode}");
        w.set_developer_mode(dev_mode);
        if dev_mode {
            debug!("developer_mode was left on, reopening the log window");
            logwindow::set_visible(true, window);
        }

        // About
        let build_timestamp = shared::utils::get_build_timestamp().unwrap_or_default();
        debug!("build_timestamp: {build_timestamp:?}");
        w.set_build_timestamp(build_timestamp.into());

        // Privacy
        let raw_opt_out = config::get(key::TELEMETRY_OPT_OUT);
        let telemetry_opt_out = raw_opt_out.as_bool().unwrap_or(false);
        debug!("telemetry_opt_out: raw={raw_opt_out:?} → {telemetry_opt_out}");
        w.set_telemetry_opt_out(telemetry_opt_out);

        info!("load() complete shortcut all config values applied to UI");
    }

    pub fn apply_context(w: &MainWindow, context: &GameContext) {
        Self::load_game(
            w,
            context.game(),
            context.installation().map(InstallationFacts::root),
        );
    }

    fn load_game(w: &MainWindow, game: &'static dyn Game, root: Option<&std::path::Path>) {
        let d = game.descriptor();
        debug!("loading the launcher settings of {}", d.id);

        let game_path = root.map_or_else(
            || {
                Self::profile_value(game, ProfileKey::GamePath)
                    .as_str()
                    .unwrap_or_default()
                    .to_string()
            },
            |root| root.to_string_lossy().into_owned(),
        );
        if game_path.is_empty() {
            warn!("{} has no game directory yet", d.id);
        }
        w.set_game_directory(game_path.into());

        let engine_settings = d.engine == Some(EngineKind::Pak);
        let start_method_choice = game.launcher().is_some_and(|l| l.start_methods().len() > 1);
        debug!(
            "{}: engine_settings={engine_settings} start_method_choice={start_method_choice}",
            d.id
        );
        w.set_engine_settings(engine_settings);
        w.set_start_method_choice(start_method_choice);

        let raw_engine = Self::profile_value(game, ProfileKey::EngineMethod);
        let engine_method = raw_engine
            .as_i64()
            .or_else(|| raw_engine.as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(0)
            .try_into()
            .unwrap_or(0);
        debug!("engine_method: raw={raw_engine:?} → {engine_method}");
        w.set_engine_method_index(engine_method);

        let raw_start = Self::profile_value(game, ProfileKey::StartMethod);
        let start_method = StartMethod::decode(&raw_start);
        debug!("start_method: raw={raw_start:?} → {start_method}");
        w.set_start_method_index(start_method.as_index());

        let scale_target = Self::scale_target(game).filter(|_| engine_settings);
        w.set_show_engine_scale(scale_target.is_some());
        if let Some((dirs, setting)) = scale_target {
            let current_scale = scale::get_current_scale(&dirs, &setting);
            let engine_scale = Self::scale_to_percent(current_scale);
            debug!("engine_scale: Engine.ini={current_scale} → {engine_scale}%");
            w.set_engine_scale(engine_scale);
        }

        let raw_ignore_checksum = Self::profile_value(game, ProfileKey::IgnoreChecksum);
        w.set_ignore_checksum(raw_ignore_checksum.as_bool().unwrap_or(false));

        let raw_launch_args = Self::profile_value(game, ProfileKey::LaunchArgs);
        w.set_launch_args(raw_launch_args.as_str().unwrap_or("").into());

        if cfg!(target_os = "linux") {
            let raw_proton = Self::profile_value(game, ProfileKey::ProtonArgs);
            w.set_proton_launch_args(raw_proton.as_str().unwrap_or("").into());
            Self::load_proton_versions(w);
        }
    }

    fn scale_target(game: &dyn Game) -> Option<(Vec<std::path::PathBuf>, ScaleSetting)> {
        if !scale::SUPPORTED {
            return None;
        }
        let unreal = game.unreal()?;
        let setting = unreal.application_scale()?;
        Some((
            unreal.config_locations(&PlatformPaths::observe_host(game)),
            setting,
        ))
    }

    fn refuse(window: &slint::Weak<MainWindow>, action: &str, error: &anyhow::Error) {
        warn!("refused to {action}: {error:#}");
        ToastHandler::show(window, format!("Could not {action}: {error:#}"), "error");
    }

    fn current_game() -> &'static dyn Game {
        runtime::get().map_or_else(|_| transitional_selected_game(), |r| r.snapshot().game)
    }

    fn profile_value(game: &dyn Game, key: ProfileKey) -> Value {
        match config::read_profile(&game.descriptor().id) {
            Ok(profile) => profile.value(key, game),
            Err(e) => {
                error!("could not read the {} profile: {e:#}", game.descriptor().id);
                game.profile_default(key)
            }
        }
    }

    fn save_profile(patch: &ProfilePatch) {
        let game = Self::current_game();
        match config::update_profile(&game.descriptor().id, patch) {
            Ok(()) => debug!("saved to the {} profile", game.descriptor().id),
            Err(e) => error!(
                "could not save to the {} profile: {e:#}",
                game.descriptor().id
            ),
        }
    }

    fn reconfigure_profile(patch: ProfilePatch) {
        let change = ConfigChange::new().profile(&Self::current_game().descriptor().id, patch);
        std::thread::spawn(move || {
            if let Err(e) = runtime::get().and_then(|r| r.reconfigure(change)) {
                error!("could not reconfigure the engine: {e:#}");
            }
        });
    }

    #[cfg(not(target_os = "linux"))]
    const fn load_proton_versions(w: &MainWindow) {
        let _ = w;
    }

    #[cfg(target_os = "linux")]
    fn load_proton_versions(w: &MainWindow) {
        use backend::classes::linux;

        let builds = linux::installed_dwproton_builds();
        let customs = linux::custom_proton_builds();
        debug!("installed DW-Proton builds: {builds:?}, added manually: {customs:?}");

        let builtin_count = builds.len() + 1;

        let game = Self::current_game();
        let raw_custom = Self::profile_value(game, ProfileKey::ProtonCustomPath);
        let selected_custom = raw_custom.as_str().unwrap_or("").trim().to_string();
        let raw_version = Self::profile_value(game, ProfileKey::ProtonVersion);
        let saved = raw_version.as_str().unwrap_or("").trim().to_string();

        let index = if !selected_custom.is_empty() {
            customs
                .iter()
                .position(|dir| dir.to_string_lossy() == selected_custom)
                .map_or_else(
                    || {
                        warn!(
                            "the selected Proton installation {selected_custom:?} is gone, \
                             showing Automatic instead"
                        );
                        0
                    },
                    |i| builtin_count + i,
                )
        } else if saved.is_empty() {
            0
        } else {
            builds.iter().position(|build| *build == saved).map_or_else(
                || {
                    warn!(
                        "saved proton_version {saved:?} is not installed any \
                         more, showing Automatic instead"
                    );
                    0
                },
                |i| i + 1,
            )
        };
        debug!("proton_version: raw={raw_version:?} custom={raw_custom:?} → index={index}");

        let mut names = builds;
        names.extend(customs.iter().map(|dir| Self::proton_build_name(dir)));

        let selected_name = index
            .checked_sub(1)
            .and_then(|i| names.get(i))
            .cloned()
            .unwrap_or_default();

        let mut options = Vec::with_capacity(names.len() + 1);
        options.push(slint::SharedString::from(crate::translations::tr(
            "settings.proton-version.automatic",
        )));
        options.extend(names.iter().map(slint::SharedString::from));

        w.set_proton_versions(slint::ModelRc::new(slint::VecModel::from(options)));
        w.set_proton_version_index(i32::try_from(index).unwrap_or(0));
        w.set_proton_builtin_count(i32::try_from(builtin_count).unwrap_or(1));
        w.set_proton_version_not_recommended(linux::is_proton_version_not_recommended(
            &selected_name,
        ));
    }

    #[cfg(target_os = "linux")]
    fn proton_build_name(dir: &std::path::Path) -> String {
        dir.file_name()
            .unwrap_or(dir.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    #[cfg(not(target_os = "linux"))]
    fn select_proton_version(window: &slint::Weak<MainWindow>, index: i32) {
        let _ = (window, index);
    }

    #[cfg(target_os = "linux")]
    fn select_proton_version(window: &slint::Weak<MainWindow>, index: i32) {
        use backend::classes::linux;

        let Some(w) = window.upgrade() else {
            error!("window handle dead when changing proton_version");
            return;
        };

        let name = usize::try_from(index)
            .ok()
            .filter(|_| index > 0)
            .and_then(|index| w.get_proton_versions().row_data(index))
            .map_or_default(|name| name.to_string());

        if let Ok(custom_index) = usize::try_from(index - w.get_proton_builtin_count()) {
            let path = linux::custom_proton_builds()
                .get(custom_index)
                .map_or_default(|dir| dir.to_string_lossy().into_owned());
            info!("proton_version changed → manually added {path:?}");
            Self::save_profile(
                &ProfilePatch::new()
                    .set(ProfileKey::ProtonCustomPath, path)
                    .set(ProfileKey::ProtonVersion, String::new()),
            );
        } else {
            info!("proton_version changed → index={index}, build={name:?}");
            Self::save_profile(
                &ProfilePatch::new()
                    .set(ProfileKey::ProtonCustomPath, String::new())
                    .set(ProfileKey::ProtonVersion, name.clone()),
            );
        }
        debug!("proton_version saved to config");

        w.set_proton_version_not_recommended(linux::is_proton_version_not_recommended(&name));
    }

    #[cfg(not(target_os = "linux"))]
    fn browse_proton_directory(window: &slint::Weak<MainWindow>) {
        let _ = window;
    }

    #[cfg(target_os = "linux")]
    fn browse_proton_directory(window: &slint::Weak<MainWindow>) {
        use backend::classes::linux;

        let ww = window.clone();
        std::thread::spawn(move || {
            let Some(picked) = rfd::FileDialog::new()
                .set_title("Select Proton Installation")
                .pick_folder()
            else {
                info!("browse_proton_directory cancelled - no folder selected");
                return;
            };

            let found = linux::resolve_proton_dirs(&picked);
            if found.is_empty() {
                warn!("{} holds no Proton installation", picked.display());
                ToastHandler::show(
                    &ww,
                    crate::translations::tr("settings.proton-version-invalid"),
                    "error",
                );
                return;
            }

            info!(
                "adding {} Proton installation(s) from {}",
                found.len(),
                picked.display()
            );
            linux::add_custom_proton_builds(&found);

            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = ww.upgrade() {
                    Self::load_proton_versions(&w);
                } else {
                    error!("window handle dead when listing the added Proton installations");
                }
            });
        });
    }

    #[cfg(not(target_os = "linux"))]
    fn remove_proton_version(window: &slint::Weak<MainWindow>, index: i32) {
        let _ = (window, index);
    }

    #[cfg(target_os = "linux")]
    fn remove_proton_version(window: &slint::Weak<MainWindow>, index: i32) {
        use backend::classes::linux;

        let Some(w) = window.upgrade() else {
            error!("window handle dead when removing a Proton installation");
            return;
        };

        let customs = linux::custom_proton_builds();
        let removed = usize::try_from(index - w.get_proton_builtin_count())
            .ok()
            .and_then(|custom_index| customs.get(custom_index));

        let Some(removed) = removed else {
            warn!("no manually added Proton installation at index {index}");
            return;
        };

        info!(
            "removing the manually added Proton installation {}",
            removed.display()
        );
        let selected = Self::profile_value(Self::current_game(), ProfileKey::ProtonCustomPath);
        if selected.as_str() == Some(&removed.to_string_lossy()) {
            debug!("it was the selected one, falling back to Automatic");
            Self::save_profile(
                &ProfilePatch::new().set(ProfileKey::ProtonCustomPath, String::new()),
            );
        }
        linux::remove_custom_proton_build(removed);

        Self::load_proton_versions(&w);
    }

    fn bind(window: &slint::Weak<MainWindow>) {
        info!("bind() started shortcut registering UI callbacks");
        let w = window.unwrap();

        // [GENERAL]
        let ww = window.clone();
        w.on_language_index_changed(move |index| {
            let code = Self::index_to_code(index);
            info!("language changed → index={index}, code={code:?}");
            config::set(key::LANGUAGE, code);
            debug!("language saved to config");

            if let Some(w) = ww.upgrade() {
                crate::translations::apply_language(&w, code);
                Self::load_proton_versions(&w);
            } else {
                error!("window handle dead when applying language change");
            }
            crate::classes::logwindow::apply_language(code);
        });

        let ww = window.clone();
        w.on_interface_minimization_changed(move |enabled| {
            info!("interface_minimization changed → {enabled}");
            config::set(key::UI_MINIMIZATION, enabled);
            debug!("interface_minimization saved to config");
            if enabled {
                if engine::session_active() {
                    crate::classes::tray::activate(&ww, false);
                }
            } else {
                crate::classes::tray::deactivate(&ww);
            }
        });

        w.on_remember_window_size_changed(move |enabled| {
            info!("remember_window_size changed → {enabled}");
            config::set(key::REMEMBER_WINDOW_SIZE, enabled);
            debug!("remember_window_size saved to config");
        });

        let ww = window.clone();
        w.on_reset_window_size(move || {
            info!("reset_window_size triggered");
            if let Some(w) = ww.upgrade() {
                crate::classes::windowsize::reset(&w);
            } else {
                error!("window handle dead when resetting the window size");
            }
        });

        w.on_discord_rpc_changed(move |enabled| {
            info!("discord_rpc changed → {enabled}");
            config::set(key::DISCORD_RPC, enabled);
            let res = if enabled { RPC.set_idle() } else { RPC.stop() };
            if let Err(e) = res {
                error!("could not update Discord RPC state: {e}");
            }
            debug!("discord_rpc saved to config");
        });

        // [LAUNCHER]
        let ww = window.clone();
        w.on_browse_game_directory(move || {
            info!("browse_game_directory triggered shortcut opening folder picker");
            let ww = ww.clone();
            std::thread::spawn(move || {
                debug!("file dialog thread spawned");
                let picked = rfd::FileDialog::new()
                    .set_title("Select Game Installation")
                    .pick_folder();

                match picked {
                    Some(path) => {
                        info!("game directory selected -> {:?}", path.display());

                        let game = Self::current_game();
                        let path = if let Some((root, _)) = resolve_game_root(game, &path) {
                            if root != path {
                                info!("resolved selection to install root -> {:?}", root.display());
                            }
                            root
                        } else {
                            warn!(
                                "{:?} does not look like a game install (no launcher \
                                 or game files found)",
                                path.display()
                            );
                            path
                        };

                        let path_str: String = path.to_string_lossy().into_owned();
                        let change = ConfigChange::new().profile(
                            &game.descriptor().id,
                            ProfilePatch::new().set(ProfileKey::GamePath, path_str.clone()),
                        );
                        match runtime::get().and_then(|r| r.reconfigure(change)) {
                            Ok(Reconfigured::Applied(context)) => {
                                debug!("game_path saved, now running {}", context.tag());
                            }
                            Ok(Reconfigured::Deferred) => {
                                info!("game_path will be saved once Aurora is free");
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(w) = ww.upgrade() {
                                        w.set_game_directory(path_str.into());
                                    }
                                });
                                return;
                            }
                            Err(e) => error!("could not change the game directory: {e:#}"),
                        }

                        ModManagerHandler::reload(&ww);
                    }
                    None => {
                        info!("browse_game_directory cancelled shortcut no folder selected");
                    }
                }
            });
        });

        let ww = window.clone();
        w.on_ignore_checksum_changed(move |enabled| {
            info!("ignore_checksum toggled → {enabled}");

            if !enabled {
                Self::reconfigure_profile(
                    ProfilePatch::new().set(ProfileKey::IgnoreChecksum, false),
                );
                return;
            }

            let Some(w) = ww.upgrade() else {
                error!("window handle dead when opening the ignore_checksum warning");
                return;
            };

            let keys = w.global::<TrKey>();
            let title = Self::translation(&w, keys.get_popup_ignore_checksum_title());
            let message = Self::translation(&w, keys.get_popup_ignore_checksum_message());

            w.set_popup_id(IGNORE_CHECKSUM_POPUP_ID.into());
            w.set_popup_kind("warning".into());
            w.set_popup_title(title);
            w.set_popup_message(message);
            w.set_popup_confirm_delay(0);
            w.set_popup_required_count(0);
            w.set_popup_checkboxes(slint::ModelRc::default());
            w.set_popup_active(true);
        });

        w.on_engine_method_index_changed(move |index| {
            info!("engine_method changed -> {index}");
            Self::reconfigure_profile(ProfilePatch::new().set(ProfileKey::EngineMethod, index));
        });

        let ww = window.clone();
        w.on_start_method_index_changed(move |index| {
            if StartMethod::from_num(i64::from(index)) == StartMethod::Manual {
                info!("start_method changed -> {index}");
                Self::reconfigure_profile(ProfilePatch::new().set(ProfileKey::StartMethod, index));
                return;
            }

            let Some(w) = ww.upgrade() else {
                error!("window handle dead when opening the direct mode warning");
                return;
            };

            let keys = w.global::<TrKey>();
            let title = Self::translation(&w, keys.get_popup_direct_mode_title());
            let message = Self::translation(&w, keys.get_popup_direct_mode_message());

            w.set_popup_id(DIRECT_MODE_POPUP_ID.into());
            w.set_popup_kind("warning".into());
            w.set_popup_title(title);
            w.set_popup_message(message);
            w.set_popup_confirm_delay(0);
            w.set_popup_required_count(0);
            w.set_popup_checkboxes(slint::ModelRc::default());
            w.set_popup_active(true);
        });

        let ww = window.clone();
        w.on_engine_scale_changed(move |percent| {
            info!("engine_scale changed -> {percent}%");

            if !scale::SUPPORTED {
                warn!("engine_scale is not supported on this platform, ignoring");
                return;
            }

            let guard = match runtime::get().and_then(GameRuntime::try_begin_mutation) {
                Ok(guard) => guard,
                Err(e) => {
                    Self::refuse(&ww, "save the application scale", &e);
                    return;
                }
            };
            let Some((dirs, setting)) = Self::scale_target(guard.context().game()) else {
                warn!("the current game has no application scale, ignoring");
                return;
            };

            if scale::apply_scale(&dirs, &setting, f64::from(percent) / 100.0) {
                debug!("engine_scale written to {}", setting.ini.file);
                return;
            }

            error!("engine_scale could not be written to {}", setting.ini.file);
            ToastHandler::show(&ww, "Failed to save the application scale.", "error");

            let actual = Self::scale_to_percent(scale::get_current_scale(&dirs, &setting));
            if let Some(w) = ww.upgrade() {
                w.set_engine_scale(actual);
            } else {
                error!("window handle dead when reverting engine_scale");
            }
        });

        let ww = window.clone();
        w.on_launch_args_changed(move |args| {
            info!("launch_args changed -> {args:?}");
            let guard = match runtime::get().and_then(GameRuntime::try_begin_mutation) {
                Ok(guard) => guard,
                Err(e) => {
                    Self::refuse(&ww, "save the launch arguments", &e);
                    if let Some(w) = ww.upgrade() {
                        let saved =
                            Self::profile_value(Self::current_game(), ProfileKey::LaunchArgs);
                        w.set_launch_args(saved.as_str().unwrap_or("").into());
                    }
                    return;
                }
            };
            Self::save_profile(&ProfilePatch::new().set(ProfileKey::LaunchArgs, args.as_str()));

            let game = guard.context().game();
            let (Some(unreal), Some(arguments)) = (game.unreal(), game.launch_arguments()) else {
                debug!("{} has no launch arguments to write", game.descriptor().id);
                return;
            };
            let dirs = unreal.config_locations(&PlatformPaths::observe_host(game));
            match launch_args::apply(&dirs, arguments.catalogue(), args.as_str()) {
                Ok(()) => debug!("launch_args written to the game's config files"),
                Err(e) => {
                    error!("launch_args could not be applied: {e}");
                    ToastHandler::show(
                        &ww,
                        format!("Could not apply launch arguments: {e}"),
                        "error",
                    );
                }
            }
        });

        // [LINUX]
        w.on_proton_launch_args_changed(move |args| {
            info!("proton_launch_args changed → {args:?}");
            Self::save_profile(&ProfilePatch::new().set(ProfileKey::ProtonArgs, args.as_str()));
        });

        let ww = window.clone();
        w.on_proton_version_index_changed(move |index| {
            Self::select_proton_version(&ww, index);
        });

        let ww = window.clone();
        w.on_browse_proton_directory(move || {
            info!("browse_proton_directory triggered - opening folder picker");
            Self::browse_proton_directory(&ww);
        });

        let ww = window.clone();
        w.on_remove_proton_version(move |index| {
            Self::remove_proton_version(&ww, index);
        });

        w.on_desktop_entry_changed(move |enabled| {
            info!("desktop_entry changed → {enabled}");

            #[cfg(target_os = "linux")]
            {
                crate::classes::desktop::apply(enabled);
                crate::classes::desktop::mark_prompted();
            }

            #[cfg(not(target_os = "linux"))]
            config::set(key::DESKTOP_ENTRY, enabled);

            debug!("desktop_entry saved to config");
        });

        // [DEVELOPER]

        let ww = window.clone();
        w.on_developer_mode_changed(move |enabled| {
            info!("developer_mode changed → {enabled}");
            config::set(key::DEV_MODE, enabled);
            debug!("developer_mode saved to config");
            logwindow::set_visible(enabled, &ww);
        });

        w.on_export_telemetry({
            let ww = window.clone();
            move || {
                info!("export_telemetry triggered");
                let ww = ww.clone();
                std::thread::spawn(move || {
                    debug!("telemetry export thread spawned");
                    match shared::telemetry::export_telemetry() {
                        Ok(()) => {
                            info!("telemetry export complete");
                            ToastHandler::show(
                                &ww,
                                "Exported debugging logs to the logs folder.",
                                "success",
                            );

                            let logs_dir = shared::logger::logs_directory();
                            if let Err(e) = open_folder(&logs_dir) {
                                error!("failed to open Logs directory: {e}");
                                ToastHandler::show(&ww, "Failed to open logs folder.", "error");
                            }
                        }
                        Err(e) => {
                            error!("telemetry export failed: {e}");
                            ToastHandler::show(
                                &ww,
                                format!("Telemetry export failed: {e}"),
                                "error",
                            );
                        }
                    }
                });
            }
        });

        // [ABOUT]
        w.on_open_about_link(move |id| {
            let Some((_, url)) = ABOUT_LINKS.iter().find(|(name, _)| *name == id.as_str()) else {
                error!("about link {id:?} has no URL mapped to it");
                return;
            };

            info!("opening about link {id:?} -> {url}");
            if let Err(e) = open::that(url) {
                error!("could not open {url}: {e}");
            }
        });

        let ww = window.clone();
        w.on_copy_build_info(move || {
            let info = Self::build_info();
            info!("copying build info to the clipboard");
            debug!("build info:\n{info}");

            match arboard::Clipboard::new().and_then(|mut c| c.set_text(info)) {
                Ok(()) => ToastHandler::show(
                    &ww,
                    crate::translations::tr("settings.about-copied"),
                    "success",
                ),
                Err(e) => {
                    error!("could not copy build info to clipboard: {e}");
                    ToastHandler::show(
                        &ww,
                        crate::translations::tr("settings.about-copy-failed"),
                        "error",
                    );
                }
            }
        });

        // [PRIVACY]
        w.on_telemetry_opt_out_changed(move |opted_out| {
            info!("telemetry_opt_out changed → {opted_out}");
            config::set(key::TELEMETRY_OPT_OUT, opted_out);
            debug!("telemetry_opt_out saved to config");
        });

        info!("bind() complete shortcut all callbacks registered");
    }

    fn build_info() -> String {
        use sysinfo::System;

        let build = shared::utils::get_build_timestamp().unwrap_or_else(|| "Unknown".to_string());
        let os_name = System::name().unwrap_or_else(|| "Unknown".to_string());
        let os_version = System::os_version()
            .or_else(System::kernel_version)
            .unwrap_or_else(|| "Unknown".to_string());

        format!(
            "Aurora v{}\nBuild deployed at: {build}\nOS: {os_name} ({os_version})\nArchitecture: {}",
            shared::utils::get_local_version(),
            System::cpu_arch(),
        )
    }

    #[allow(clippy::cast_possible_truncation)]
    fn scale_to_percent(scale: f64) -> i32 {
        let stepped = (scale * 20.0).round() * 5.0;
        stepped.clamp(50.0, 200.0) as i32
    }

    fn translation(w: &MainWindow, index: i32) -> slint::SharedString {
        w.global::<Tr>()
            .get_values()
            .row_data(index.try_into().unwrap_or(0))
            .unwrap_or_default()
    }

    pub fn confirm_ignore_checksum() {
        info!("ignore_checksum warning confirmed");
        Self::reconfigure_profile(ProfilePatch::new().set(ProfileKey::IgnoreChecksum, true));
    }

    pub fn confirm_direct_mode() {
        info!("direct mode warning confirmed");
        Self::reconfigure_profile(
            ProfilePatch::new().set(ProfileKey::StartMethod, StartMethod::Direct.as_index()),
        );
    }

    pub fn cancel_direct_mode(window: &slint::Weak<MainWindow>) {
        info!("direct mode warning cancelled, staying on manual");
        if let Some(w) = window.upgrade() {
            w.set_start_method_index(StartMethod::Manual.as_index());
        } else {
            error!("window handle dead when reverting start_method");
        }
    }

    pub fn cancel_ignore_checksum(window: &slint::Weak<MainWindow>) {
        info!("ignore_checksum warning cancelled, reverting the toggle");
        if let Some(w) = window.upgrade() {
            w.set_ignore_checksum(false);
        } else {
            error!("window handle dead when reverting ignore_checksum");
        }
    }

    pub fn index_to_code(index: i32) -> &'static str {
        let result = LANGUAGES
            .get(index.try_into().unwrap_or(0))
            .map_or("en", |l| l.code.as_str());

        if result == "en" && index != 0 {
            warn!(
                "index_to_code: index={index} is out of range ({} langs loaded), falling back to \"en\"",
                LANGUAGES.len()
            );
        }

        result
    }

    pub fn code_to_index(code: &str) -> Option<i32> {
        let result = LANGUAGES
            .iter()
            .position(|l| l.code == code)
            .map(|i| i.try_into().unwrap_or(0));

        if result.is_none() {
            warn!("code_to_index: unknown language code {code:?} shortcut will default to index 0");
        }

        result
    }
}
