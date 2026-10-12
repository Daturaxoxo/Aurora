pub mod capabilities;
pub mod identity;
pub mod launch;
pub mod locate;
pub mod nte;
pub mod payload;
pub mod platform;
pub mod sp;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use crate::config::ProfileKey;
use anyhow::{Result, anyhow};
use capabilities::{
    AddonSupport, CharacterSupport, GameBananaSupport, LaunchArgumentsSupport, LauncherSupport,
    LuaSupport, ModManagerSupport, ModuleSupport, ScreenshotSupport, UnrealSupport,
};
use identity::{GameId, SafeRelativePath};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKind {
    Pak,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherIdentifier {
    pub file: SafeRelativePath,
    pub variant: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteamMetadata {
    pub app_id: &'static str,
}

#[derive(Debug, Clone)]
pub struct GameDescriptor {
    pub id: GameId,
    pub display_name: &'static str,
    pub aliases: &'static [&'static str],
    pub launchers: Vec<LauncherIdentifier>,
    pub markers: Vec<SafeRelativePath>,
    pub processes: &'static [&'static str],
    pub game_executable: Option<SafeRelativePath>,
    pub binaries: Option<SafeRelativePath>,
    pub payload_dir: SafeRelativePath,
    pub payload_files: Vec<SafeRelativePath>,
    pub steam: Option<SteamMetadata>,
    pub engine: Option<EngineKind>,
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
    fn mod_manager(&self) -> Option<&dyn ModManagerSupport> {
        None
    }
    fn modules(&self) -> Option<&dyn ModuleSupport> {
        None
    }
    fn lua(&self) -> Option<&dyn LuaSupport> {
        None
    }
    fn characters(&self) -> Option<&dyn CharacterSupport> {
        None
    }
    fn gamebanana(&self) -> Option<&dyn GameBananaSupport> {
        None
    }
    fn screenshots(&self) -> Option<&dyn ScreenshotSupport> {
        None
    }
    fn unreal(&self) -> Option<&dyn UnrealSupport> {
        None
    }
    fn launch_arguments(&self) -> Option<&dyn LaunchArgumentsSupport> {
        None
    }
}

static REGISTRY: LazyLock<Vec<&'static dyn Game>> = LazyLock::new(|| {
    let games: Vec<&'static dyn Game> = vec![&*nte::NTE, &*sp::SP];
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

fn check_inside(game: &str, field: &str, root: &Path, path: &Path) -> Result<()> {
    let inside = path.strip_prefix(root).is_ok_and(|rest| {
        rest.components().next().is_some()
            && rest
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
    });
    if inside {
        Ok(())
    } else {
        Err(anyhow!(
            "{game}: {field} '{}' is not inside '{}'",
            path.display(),
            root.display()
        ))
    }
}

fn check_naming(game: &str, field: &'static str, naming: &capabilities::ModNaming) -> Result<()> {
    if naming.extensions.is_empty() {
        return Err(anyhow!("{game}: {field} has no file extensions"));
    }
    check_list(game, field, naming.extensions.iter().copied())?;
    if let Some(ext) = naming.extensions.iter().find(|e| e.contains('.')) {
        return Err(anyhow!("{game}: {field} extension '{ext}' contains a dot"));
    }
    if naming.toggle_extensions.is_empty() {
        return Err(anyhow!("{game}: {field} has nothing to toggle"));
    }
    if let Some(ext) = naming
        .toggle_extensions
        .iter()
        .find(|t| !naming.extensions.iter().any(|e| e.eq_ignore_ascii_case(t)))
    {
        return Err(anyhow!(
            "{game}: {field} toggles '{ext}', which is not one of its extensions"
        ));
    }
    if naming.disabled_suffix.len() < 2 || !naming.disabled_suffix.starts_with('.') {
        return Err(anyhow!(
            "{game}: {field} disabled suffix '{}' is not an extension",
            naming.disabled_suffix
        ));
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
        check_list(
            &id,
            "shipped_addons.name",
            shipped
                .iter()
                .flat_map(|a| std::iter::once(a.name).chain(a.legacy_names.iter().copied())),
        )?;
        check_list(
            &id,
            "shipped_addons.config_key",
            shipped.iter().map(|a| a.config_key),
        )?;

        let overlays = addons.overlay_addons();
        check_list(
            &id,
            "overlay_addons.config_key",
            overlays.iter().map(|o| o.config_key),
        )?;
        check_list(
            &id,
            "overlay_addons.folder",
            overlays.iter().map(|o| o.folder),
        )?;
        for overlay in overlays {
            if !shipped.iter().any(|a| a.config_key == overlay.config_key) {
                return Err(anyhow!(
                    "{id}: overlay addon '{}' has config key '{}', which no shipped addon uses",
                    overlay.folder,
                    overlay.config_key
                ));
            }
            check_list(
                &id,
                "overlay_addons.persist",
                overlay.persist.iter().copied(),
            )?;
        }
    }

    if let Some(mods) = game.mod_manager() {
        let probe = Path::new("probe-root");
        let layout = mods.layout(probe);
        if layout.roots.is_empty() {
            return Err(anyhow!("{id}: mod manager has no mod roots"));
        }
        for root in &layout.roots {
            check_inside(&id, "mod root", probe, root)?;
        }
        check_naming(&id, "mod_manager", mods.naming())?;
        if layout.naming != *mods.naming() {
            return Err(anyhow!("{id}: mod manager layout and naming disagree"));
        }
    }

    if let Some(modules) = game.modules() {
        check_naming(&id, "modules", modules.naming())?;
    }

    if let Some(lua) = game.lua() {
        let payload = Path::new("probe-payload");
        let root = Path::new("probe-root");
        let layout = lua.layout(payload, Some(root));
        for (field, path) in [
            ("runtime", &layout.runtime),
            ("marker", &layout.marker),
            ("scripts", &layout.scripts),
            ("settings", &layout.settings),
        ] {
            check_inside(&id, field, payload, path)?;
        }
        for deploy in &layout.deploy {
            check_inside(&id, "deploy source", &layout.runtime, &deploy.source)?;
            check_inside(&id, "deploy destination", root, &deploy.destination)?;
        }
        if !lua.layout(payload, None).deploy.is_empty() {
            return Err(anyhow!("{id}: lua deploys without a game root"));
        }
        if !lua.runtime_url().starts_with("https://") {
            return Err(anyhow!("{id}: lua runtime url is not https"));
        }
    }

    if let Some(characters) = game.characters() {
        let catalogue = characters.catalogue();
        SafeRelativePath::parse(catalogue.assets)
            .ok()
            .filter(|path| path.components().len() == 1)
            .ok_or_else(|| {
                anyhow!(
                    "{id}: character assets '{}' is not a folder name",
                    catalogue.assets
                )
            })?;
        check_list(
            &id,
            "characters.aliases",
            catalogue.aliases.iter().map(|(alias, _)| *alias),
        )?;
        for (alias, target) in catalogue.aliases {
            if target.trim().is_empty() {
                return Err(anyhow!(
                    "{id}: character alias '{alias}' has an empty target"
                ));
            }
        }
        check_list(&id, "characters.hidden", catalogue.hidden.iter().copied())?;
        check_list(
            &id,
            "characters.renamed",
            catalogue.renamed.iter().map(|(slug, _)| *slug),
        )?;
    }

    if let Some(gamebanana) = game.gamebanana() {
        if gamebanana.remote_game_id() == 0 {
            return Err(anyhow!("{id}: gamebanana remote game id is 0"));
        }
        if game.mod_manager().is_none() {
            return Err(anyhow!(
                "{id}: gamebanana needs a mod manager to install into"
            ));
        }
        let characters = gamebanana.characters();
        check_list(
            &id,
            "gamebanana.characters",
            characters.iter().map(|c| c.name),
        )?;
        if let Some(c) = characters.iter().find(|c| c.category == 0) {
            return Err(anyhow!(
                "{id}: gamebanana character '{}' has category 0",
                c.name
            ));
        }
    }

    if let Some(screenshots) = game.screenshots() {
        check_list(
            &id,
            "screenshots.ignored",
            screenshots.ignored_directories().iter().copied(),
        )?;
        let root = Path::new("probe-root");
        let installation = InstallationFacts::new(root.to_path_buf(), "probe", "probe");
        let platform = platform::PlatformPaths {
            local_app_data: Some(PathBuf::from("probe-local")),
            pictures: Some(PathBuf::from("probe-pictures")),
        };
        let sources = screenshots.sources(Some(&installation), &platform);
        if sources.is_empty() {
            return Err(anyhow!("{id}: screenshots have no sources"));
        }
        for source in &sources {
            let base = match source.kind {
                capabilities::ScreenshotSourceKind::Game => root,
                capabilities::ScreenshotSourceKind::Pictures => Path::new("probe-pictures"),
            };
            check_inside(&id, "screenshot source", base, &source.dir)?;
        }
    }

    if let Some(unreal) = game.unreal()
        && let Some(scale) = unreal.application_scale()
    {
        if scale.min <= 0.0 || scale.min >= scale.max {
            return Err(anyhow!(
                "{id}: application scale range {}..{} is invalid",
                scale.min,
                scale.max
            ));
        }
        check_list(
            &id,
            "application_scale",
            [scale.ini.file, scale.ini.section, scale.ini.key],
        )?;
    }

    if let Some(arguments) = game.launch_arguments() {
        if game.unreal().is_none() {
            return Err(anyhow!(
                "{id}: launch arguments need unreal config locations to write to"
            ));
        }
        serde_json::from_str::<serde_json::Map<String, Value>>(arguments.catalogue())
            .map_err(|e| anyhow!("{id}: launch argument catalogue is malformed: {e}"))?;
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
        let payload_files: Vec<String> = d.payload_files.iter().map(ToString::to_string).collect();
        check_list(
            &id,
            "payload_files",
            payload_files.iter().map(String::as_str),
        )?;
        if let Some(steam) = d.steam
            && (steam.app_id.is_empty() || !steam.app_id.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(anyhow!(
                "{id}: steam.app_id '{}' is not a numeric app id",
                steam.app_id
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::capabilities::{
        FileState, GameBananaSupport, LaunchArgumentsSupport, ModLayout, ModNaming,
        ScreenshotSourceKind, UnrealSupport,
    };
    use super::platform::PlatformPaths;
    use super::*;

    const NAMING: ModNaming = ModNaming {
        extensions: &["pak"],
        toggle_extensions: &["pak"],
        disabled_suffix: ".off",
        display_suffixes: &[],
    };

    #[derive(Default)]
    struct Synthetic {
        descriptor: Option<GameDescriptor>,
        mods: Option<(&'static str, ModNaming)>,
        gamebanana: bool,
        unreal: bool,
        arguments: Option<&'static str>,
    }

    impl Synthetic {
        fn new() -> Self {
            Self {
                descriptor: Some(GameDescriptor {
                    id: GameId::parse("synthetic").unwrap(),
                    display_name: "Synthetic",
                    aliases: &[],
                    launchers: Vec::new(),
                    markers: Vec::new(),
                    processes: &[],
                    game_executable: None,
                    binaries: None,
                    payload_dir: SafeRelativePath::parse("synthetic").unwrap(),
                    payload_files: Vec::new(),
                    steam: None,
                    engine: None,
                }),
                ..Self::default()
            }
        }
    }

    impl Game for Synthetic {
        fn descriptor(&self) -> &GameDescriptor {
            self.descriptor.as_ref().unwrap()
        }
        fn inspect_installation(&self, root: &Path) -> Result<InstallationFacts> {
            Ok(InstallationFacts::new(root.to_path_buf(), "test", "test"))
        }
        fn mod_manager(&self) -> Option<&dyn ModManagerSupport> {
            self.mods.map(|_| self as &dyn ModManagerSupport)
        }
        fn gamebanana(&self) -> Option<&dyn GameBananaSupport> {
            self.gamebanana.then_some(self as &dyn GameBananaSupport)
        }
        fn unreal(&self) -> Option<&dyn UnrealSupport> {
            self.unreal.then_some(self as &dyn UnrealSupport)
        }
        fn launch_arguments(&self) -> Option<&dyn LaunchArgumentsSupport> {
            self.arguments.map(|_| self as &dyn LaunchArgumentsSupport)
        }
    }

    impl ModManagerSupport for Synthetic {
        fn layout(&self, game_root: &Path) -> ModLayout {
            let (root, naming) = self.mods.unwrap();
            ModLayout {
                roots: vec![game_root.join(root)],
                naming,
            }
        }
        fn naming(&self) -> &ModNaming {
            &self.mods.as_ref().unwrap().1
        }
    }

    impl GameBananaSupport for Synthetic {
        fn remote_game_id(&self) -> u32 {
            1
        }
    }

    impl UnrealSupport for Synthetic {
        fn config_locations(&self, _: &PlatformPaths) -> Vec<PathBuf> {
            Vec::new()
        }
    }

    impl LaunchArgumentsSupport for Synthetic {
        fn catalogue(&self) -> &'static str {
            self.arguments.unwrap()
        }
    }

    fn check(game: &Synthetic) -> Result<()> {
        validate_registry(&[game])
    }

    #[test]
    fn production_registry_is_valid() {
        validate_registry(registry()).unwrap();
    }

    #[test]
    fn nte_has_every_lane_a_capability_and_sp_has_none() {
        let nte = find_game("nte").unwrap();
        assert!(nte.mod_manager().is_some());
        assert!(nte.modules().is_some());
        assert!(nte.lua().is_some());
        assert!(nte.characters().is_some());
        assert!(nte.gamebanana().is_some());
        assert!(nte.screenshots().is_some());
        assert!(nte.unreal().is_some());
        assert!(nte.launch_arguments().is_some());

        let sp = find_game("sp").unwrap();
        assert!(sp.mod_manager().is_none());
        assert!(sp.modules().is_none());
        assert!(sp.lua().is_none());
        assert!(sp.characters().is_none());
        assert!(sp.gamebanana().is_none());
        assert!(sp.screenshots().is_none());
        assert!(sp.unreal().is_none());
        assert!(sp.launch_arguments().is_none());
    }

    #[test]
    fn a_game_without_capabilities_is_valid() {
        let game = Synthetic::new();
        assert!(game.mod_manager().is_none());
        check(&game).unwrap();
    }

    #[test]
    fn gamebanana_needs_a_mod_manager() {
        let mut game = Synthetic::new();
        game.gamebanana = true;
        assert!(check(&game).is_err());
        game.mods = Some(("Mods", NAMING));
        check(&game).unwrap();
    }

    #[test]
    fn launch_arguments_need_unreal_and_a_valid_catalogue() {
        let mut game = Synthetic::new();
        game.arguments = Some("{}");
        assert!(check(&game).is_err());
        game.unreal = true;
        check(&game).unwrap();
        game.arguments = Some("not json");
        assert!(check(&game).is_err());
    }

    #[test]
    fn mod_roots_must_stay_inside_the_game() {
        let mut game = Synthetic::new();
        game.mods = Some(("../Elsewhere", NAMING));
        assert!(check(&game).is_err());
    }

    #[test]
    fn naming_must_be_coherent() {
        let mut game = Synthetic::new();
        for naming in [
            ModNaming {
                extensions: &[],
                ..NAMING
            },
            ModNaming {
                toggle_extensions: &["asi"],
                ..NAMING
            },
            ModNaming {
                toggle_extensions: &[],
                ..NAMING
            },
            ModNaming {
                disabled_suffix: "off",
                ..NAMING
            },
            ModNaming {
                extensions: &["pak", "PAK"],
                ..NAMING
            },
        ] {
            game.mods = Some(("Mods", naming));
            assert!(check(&game).is_err(), "{naming:?} should be rejected");
        }
    }

    #[test]
    fn nte_mod_naming_matches_the_old_rules() {
        let mods = find_game("nte").unwrap().mod_manager().unwrap();
        assert_eq!(mods.classify_name("a.pak"), FileState::Enabled);
        assert_eq!(mods.classify_name("a.UTOC"), FileState::Enabled);
        assert_eq!(mods.classify_name("a.ucas.disabled"), FileState::Disabled);
        assert_eq!(mods.classify_name("a.txt"), FileState::Other);
        assert_eq!(mods.classify_name("a.txt.disabled"), FileState::Other);

        // only paks are renamed off, anything disabled comes back
        assert_eq!(
            mods.toggle_name("a.pak", false).as_deref(),
            Some("a.pak.disabled")
        );
        assert_eq!(mods.toggle_name("a.utoc", false), None);
        assert_eq!(
            mods.toggle_name("a.utoc.disabled", true).as_deref(),
            Some("a.utoc")
        );
        assert_eq!(mods.toggle_name("a.pak", true), None);

        assert_eq!(mods.naming().display_name("Shinku_P"), "Shinku");
        assert!(mods.validate_entry(Path::new("x/a.pak")).is_ok());
        assert!(mods.validate_entry(Path::new("x/a.zip")).is_err());
    }

    #[test]
    fn nte_modules_reject_missing_and_foreign_files() {
        let modules = find_game("nte").unwrap().modules().unwrap();
        let dir = std::env::temp_dir().join(format!("aurora-modules-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let module = dir.join("a.asi");
        let other = dir.join("b.dll");
        std::fs::write(&module, b"asi").unwrap();
        std::fs::write(&other, b"dll").unwrap();

        let accepted =
            modules.validate_selection(&[module.clone(), other, dir.join("missing.asi")]);
        assert_eq!(accepted, vec![module]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nte_screenshot_sources_put_selfies_first() {
        let shots = find_game("nte").unwrap().screenshots().unwrap();
        let facts = InstallationFacts::new(PathBuf::from("game"), "test", "test");
        let platform = PlatformPaths {
            local_app_data: None,
            pictures: Some(PathBuf::from("pictures")),
        };

        let kinds: Vec<_> = shots
            .sources(Some(&facts), &platform)
            .iter()
            .map(|s| s.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![ScreenshotSourceKind::Game, ScreenshotSourceKind::Pictures]
        );
        assert!(shots.ignored_directories().contains(&"66666"));

        // no installation still lists Pictures, no Pictures still lists the game
        assert_eq!(shots.sources(None, &platform).len(), 1);
        assert_eq!(
            shots.sources(Some(&facts), &PlatformPaths::default()).len(),
            1
        );
    }

    #[test]
    fn nte_lua_layout_stays_in_the_payload() {
        let lua = find_game("nte").unwrap().lua().unwrap();
        let layout = lua.layout(Path::new("payload"), Some(Path::new("game")));
        assert!(layout.scripts.starts_with("payload"));
        assert!(layout.settings.starts_with("payload"));
        assert_eq!(layout.deploy.len(), 2);
        assert!(
            layout
                .deploy
                .iter()
                .all(|d| d.destination.starts_with("game"))
        );
        assert_eq!(lua.layout(Path::new("payload"), None).deploy, Vec::new());
    }

    #[test]
    fn nte_unreal_reads_nothing_without_a_local_app_data_folder() {
        let unreal = find_game("nte").unwrap().unreal().unwrap();
        assert_eq!(
            unreal.config_locations(&PlatformPaths::default()),
            Vec::<PathBuf>::new()
        );
        let scale = unreal.application_scale().unwrap();
        assert_eq!(scale.ini.file, "Engine.ini");
    }
}
