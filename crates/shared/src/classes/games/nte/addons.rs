use super::{Nte, version::Version};
use crate::classes::games::{
    InstallationFacts,
    capabilities::{AddonSupport, OverlayAddon, PakAddon, ShippedAddon},
};

const UNAVAILABLE: &[(&str, &[Version])] = &[];

const OVERLAYS: &[OverlayAddon] = &[
    OverlayAddon {
        config_key: "rshd",
        folder: "ReShade",
        persist: &["ReShade.ini", "ReShadePreset.ini"],
    },
    OverlayAddon {
        config_key: "optsc",
        folder: "OptiScaler",
        persist: &["OptiScaler.ini", "fakenvapi.ini"],
    },
];

const PAKS: &[PakAddon] = &[
    PakAddon {
        config_key: "drv_lin",
        folder: "NoDriveLine",
        base_names: &["DisableDrivingLine_P"],
    },
    PakAddon {
        config_key: "uid_rem",
        folder: "HideUID",
        base_names: &["HideUI_UserID_P"],
    },
    PakAddon {
        config_key: "nor_rem",
        folder: "HideRedDots",
        base_names: &["Disable_RedDot_P"],
    },
    PakAddon {
        config_key: "ui_pack",
        folder: "UIModPack",
        base_names: &["ModPack_P", "ModPack_Launcher_P"],
    },
    PakAddon {
        config_key: "util_pack",
        folder: "UtilityModPack",
        base_names: &["UtilityPack_P"],
    },
];

pub(super) const SHIPPED: &[ShippedAddon] = &[
    ShippedAddon {
        name: "Censorship Remover",
        config_key: "csn_rem",
        legacy_names: &[],
    },
    ShippedAddon {
        name: "QoL Mod Pack",
        config_key: "ui_pack",
        legacy_names: &["UI Mod Pack"],
    },
    ShippedAddon {
        name: "Utility Mod Pack",
        config_key: "util_pack",
        legacy_names: &[],
    },
    ShippedAddon {
        name: "Hide UID",
        config_key: "uid_rem",
        legacy_names: &[],
    },
    ShippedAddon {
        name: "No 3D Driving Waypoint",
        config_key: "drv_lin",
        legacy_names: &[],
    },
    ShippedAddon {
        name: "Hide Notification Dots",
        config_key: "nor_rem",
        legacy_names: &[],
    },
    ShippedAddon {
        name: "ReShade",
        config_key: "rshd",
        legacy_names: &[],
    },
    ShippedAddon {
        name: "OptiScaler",
        config_key: "optsc",
        legacy_names: &[],
    },
];

pub fn is_unavailable(config_key: &str, version: Version) -> bool {
    UNAVAILABLE
        .iter()
        .any(|(key, versions)| *key == config_key && versions.contains(&version))
}

impl AddonSupport for Nte {
    fn shipped_addons(&self) -> &[ShippedAddon] {
        SHIPPED
    }

    fn is_available(&self, config_key: &str, installation: Option<&InstallationFacts>) -> bool {
        let version = installation
            .and_then(|facts| Version::from_key(facts.variant()))
            .unwrap_or_default();
        !is_unavailable(config_key, version)
    }

    fn overlay_addons(&self) -> &[OverlayAddon] {
        OVERLAYS
    }

    fn pak_addons(&self) -> &[PakAddon] {
        PAKS
    }
}
