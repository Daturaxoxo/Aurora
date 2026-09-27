use super::{Nte, version::Version};
use crate::classes::games::{
    InstallationFacts,
    capabilities::{AddonSupport, ShippedAddon},
};

const UNAVAILABLE: &[(&str, &[Version])] = &[];

pub(super) const SHIPPED: &[ShippedAddon] = &[
    ShippedAddon {
        name: "Censorship Remover",
        config_key: "csn_rem",
    },
    ShippedAddon {
        name: "UI Mod Pack",
        config_key: "ui_pack",
    },
    ShippedAddon {
        name: "Hide UID",
        config_key: "uid_rem",
    },
    ShippedAddon {
        name: "No 3D Driving Waypoint",
        config_key: "drv_lin",
    },
    ShippedAddon {
        name: "Hide Notification Dots",
        config_key: "nor_rem",
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_without_installation() {
        for addon in SHIPPED {
            assert!(super::super::NTE.is_available(addon.config_key, None));
        }
    }
}
