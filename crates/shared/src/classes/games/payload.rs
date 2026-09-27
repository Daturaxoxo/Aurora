use std::path::{Path, PathBuf};
use anyhow::{Result, anyhow};
const WRAPPERS: &str = "Wrappers";
const PLUGINS: &str = "Plugins";
const ADDONS: &str = "Addons";
const LUA: &str = "Lua";
const LUA_MARKER: [&str; 2] = ["ue4ss", "UE4SS.dll"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadLayoutKind {
    Target,
    LegacyFlat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPayloadLayout {
    root: PathBuf,
    kind: PayloadLayoutKind,
    wrappers: PathBuf,
    plugins: PathBuf,
    addons: PathBuf,
    lua: Option<PathBuf>,
}

impl ResolvedPayloadLayout {
    pub fn inspect(root: &Path, kind: PayloadLayoutKind) -> Result<Self> {
        if !root.is_dir() {
            return Err(anyhow!("payload root {} is missing", root.display()));
        }

        let required = |name: &str| {
            let path = root.join(name);
            if path.is_dir() {
                Ok(path)
            } else {
                Err(anyhow!(
                    "{kind:?} payload is incomplete: {} is missing",
                    path.display()
                ))
            }
        };

        let lua = root.join(LUA);
        let has_lua = LUA_MARKER
            .iter()
            .fold(lua.clone(), |path, part| path.join(part))
            .is_file();

        Ok(Self {
            root: root.to_path_buf(),
            kind,
            wrappers: required(WRAPPERS)?,
            plugins: required(PLUGINS)?,
            addons: required(ADDONS)?,
            lua: has_lua.then_some(lua),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub const fn kind(&self) -> PayloadLayoutKind {
        self.kind
    }

    pub fn wrappers(&self) -> &Path {
        &self.wrappers
    }

    pub fn plugins(&self) -> &Path {
        &self.plugins
    }

    pub fn addons(&self) -> &Path {
        &self.addons
    }

    pub fn lua(&self) -> Option<&Path> {
        self.lua.as_deref()
    }
}
