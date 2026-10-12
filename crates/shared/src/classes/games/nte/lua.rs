use std::path::Path;

use super::{Nte, path, paths};
use crate::classes::games::capabilities::{LuaDeployment, LuaLayout, LuaSupport};

const RUNTIME_URL: &str = "https://host.getaurora.moe/files/addons/lua/core.zip";
const RUNTIME: &str = "Lua";
const LOADER: &str = "dwmapi.dll";
const UE4SS: &str = "ue4ss";

impl LuaSupport for Nte {
    fn layout(&self, payload_root: &Path, game_root: Option<&Path>) -> LuaLayout {
        let runtime = path(RUNTIME).resolve_under(payload_root);
        let ue4ss = runtime.join(UE4SS);
        let deploy = game_root.map_or_default(|root| {
            let win64 = path(paths::CLIENT_WIN64).resolve_under(root);
            vec![
                LuaDeployment {
                    label: "Lua dwmapi.dll",
                    source: runtime.join(LOADER),
                    destination: win64.join(LOADER),
                },
                LuaDeployment {
                    label: "Lua ue4ss folder",
                    source: ue4ss.clone(),
                    destination: win64.join(UE4SS),
                },
            ]
        });

        LuaLayout {
            marker: ue4ss.join("UE4SS.dll"),
            scripts: ue4ss.join("Mods"),
            settings: ue4ss.join("UE4SS-settings.ini"),
            runtime,
            deploy,
        }
    }

    fn runtime_url(&self) -> &str {
        RUNTIME_URL
    }
}
