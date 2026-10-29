use super::{
    NTE_GAME_EXE, Nte, patcher, paths,
    version::{BypassMethod, Version},
};
use crate::classes::games::{
    InstallationFacts,
    pak::{self, EngineComponent, EngineLoader, PakLayout, PakSupport},
};
use anyhow::Result;

const SIGNATURE: &[u8] = include_bytes!("../../../../../../production/engine/NTE/everlight.sig");
const CENSORSHIP: &str = "csn_rem";
const LOADERS: &[EngineLoader] = &[
    EngineLoader {
        dll: BypassMethod::Version.dll_name(),
        label: "settings.engine-method.default",
    },
    EngineLoader {
        dll: BypassMethod::DSound.dll_name(),
        label: "settings.engine-method.alternate",
    },
];

const COMPONENTS: &[EngineComponent] = &[
    pak::EVERLIGHT,
    pak::CHKSUM,
    pak::IPC,
    EngineComponent::addon("Addons/Censorship/AuroraTF.asi", CENSORSHIP)
        .only_on(&[Version::Global.key(), Version::TW.key()]),
    EngineComponent::addon("Addons/Censorship/CNAuroraTF.asi", CENSORSHIP)
        .only_on(&[Version::CN.key()]),
];

fn version(installation: &InstallationFacts) -> Version {
    Version::from_key(installation.variant()).unwrap_or_default()
}

impl PakSupport for Nte {
    fn layout(&self, installation: &InstallationFacts) -> PakLayout {
        let root = installation.root();
        let spec = version(installation).spec();
        let mods = root.join(paths::CLIENT_PAK_DIR);
        PakLayout {
            binaries: root.join(paths::CLIENT_WIN64),
            paks: mods.parent().unwrap_or(root).to_path_buf(),
            mods,
            launcher_process: spec.launcher_process,
            game_process: NTE_GAME_EXE,
            helper_processes: spec.helper_processes.to_vec(),
        }
    }

    fn signature(&self) -> &'static [u8] {
        SIGNATURE
    }

    fn loaders(&self) -> &[EngineLoader] {
        LOADERS
    }

    fn loader(&self, installation: &InstallationFacts, engine_method: i64) -> Result<&'static str> {
        BypassMethod::resolve(engine_method, version(installation)).map(BypassMethod::dll_name)
    }

    fn components(&self) -> &[EngineComponent] {
        COMPONENTS
    }

    fn incompatible_files(&self) -> &[&'static str] {
        &["anticensor"]
    }

    fn flagged_paks(&self) -> &[&'static str] {
        &["gctip_p", "uidrm_p"]
    }

    fn content_version(&self, installation: &InstallationFacts) -> Option<String> {
        patcher::res_version(installation.root(), version(installation))
    }
}
