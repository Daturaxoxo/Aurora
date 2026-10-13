use super::{
    Nte,
    version::{Distribution, StartMethod, Version},
};
use crate::classes::games::{
    capabilities::LauncherSupport,
    launch::{
        CompatRequirement, LaunchPlan, LaunchPlatform, LaunchRequest, StartMethodConfirmation,
        StartMethodDescriptor,
    },
};

use anyhow::{Result, anyhow};

const START_METHODS: &[StartMethodDescriptor] = &[
    StartMethodDescriptor {
        id: StartMethod::Direct.id(),
        label: "settings.start-method.direct",
        opens_launcher: false,
        confirmation: Some(StartMethodConfirmation {
            title: "popup.direct-mode.title",
            message: "popup.direct-mode.message",
        }),
    },
    StartMethodDescriptor {
        id: StartMethod::Manual.id(),
        label: "settings.start-method.manual",
        opens_launcher: true,
        confirmation: None,
    },
];

const PROTON_DLL_OVERRIDES: &[&str] = &["version", "dsound", "dwmapi"];

impl LauncherSupport for Nte {
    fn start_methods(&self) -> &[StartMethodDescriptor] {
        START_METHODS
    }

    fn launch_plan(&self, request: &LaunchRequest<'_>) -> Result<LaunchPlan> {
        let installation = request.installation;
        let root = installation.root();
        let (Some(version), Some(distribution)) = (
            Version::from_key(installation.variant()),
            Distribution::from_key(installation.distribution()),
        ) else {
            return Err(anyhow!(
                "game installation {} is missing",
                root.to_string_lossy()
            ));
        };
        if !root.is_dir() {
            return Err(anyhow!(
                "game installation {} is missing",
                root.to_string_lossy()
            ));
        }

        let start_method = StartMethod::from_id(request.start_method)
            .ok_or_else(|| anyhow!("unsupported method: {}", request.start_method))?;

        let executable = root.join(version.spec().launcher_process);
        if !executable.is_file() {
            return Err(anyhow!(
                "launcher {} is missing",
                executable.to_string_lossy()
            ));
        }

        let arguments = distribution
            .launch_args()
            .iter()
            .chain(start_method.launch_args())
            .map(ToString::to_string)
            .collect();

        let (compatibility, dll_overrides) = match request.platform {
            LaunchPlatform::Windows => (None, Vec::new()),
            LaunchPlatform::Linux => (
                Some(CompatRequirement::Proton),
                PROTON_DLL_OVERRIDES
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
            ),
        };

        Ok(LaunchPlan {
            executable,
            working_directory: root.to_path_buf(),
            arguments,
            environment: Vec::new(),
            compatibility,
            dll_overrides,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classes::games::{InstallationFacts, nte::NTE};
    use std::{fs, path::PathBuf};

    fn install(name: &str, launcher: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("aurora-nte-launch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(launcher), b"").unwrap();
        root
    }

    fn plan(
        root: &std::path::Path,
        version: Version,
        distribution: Distribution,
        method: &str,
        platform: LaunchPlatform,
    ) -> Result<LaunchPlan> {
        let facts = InstallationFacts::new(root.to_path_buf(), version.key(), distribution.key());
        NTE.launch_plan(&LaunchRequest {
            installation: &facts,
            start_method: method,
            platform,
        })
    }

    #[test]
    fn epic_direct_arguments() {
        let root = install("epic", "NTEGlobalLauncher.exe");
        let plan = plan(
            &root,
            Version::Global,
            Distribution::Epic,
            "direct",
            LaunchPlatform::Windows,
        )
        .unwrap();
        assert_eq!(
            plan.arguments,
            [
                "-AUTH_PASSWORD=1234",
                "-AUTH_TYPE=exchangecode",
                "/autoplay"
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn linux_requires_proton() {
        let root = install("linux", "NTELauncher.exe");
        let plan = plan(
            &root,
            Version::CN,
            Distribution::Standalone,
            "manual",
            LaunchPlatform::Linux,
        )
        .unwrap();
        assert_eq!(plan.compatibility, Some(CompatRequirement::Proton));
        assert_eq!(plan.dll_overrides, ["version", "dsound", "dwmapi"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn typed_errors() {
        let root = install("errors", "NTETWLauncher.exe");
        let platform = LaunchPlatform::Windows;

        assert!(
            plan(
                &root,
                Version::TW,
                Distribution::Standalone,
                "launcher",
                platform,
            )
            .is_err(),
        );
        assert!(
            plan(
                &root,
                Version::Global,
                Distribution::Standalone,
                "direct",
                platform
            )
            .is_err()
        );
        assert!(
            plan(
                &root,
                Version::Unknown,
                Distribution::Standalone,
                "direct",
                platform
            )
            .is_err(),
        );

        fs::remove_dir_all(&root).unwrap();
        assert!(
            plan(
                &root,
                Version::TW,
                Distribution::Standalone,
                "direct",
                platform
            )
            .is_err(),
        );
    }
}
