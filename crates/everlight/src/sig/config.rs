use crate::process::scan::Pattern;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadMethod {
    SigBypass,
    AssetLoader,
}

impl LoadMethod {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "sigbypass" => Some(Self::SigBypass),
            "assetloader" => Some(Self::AssetLoader),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::SigBypass => "sigbypass",
            Self::AssetLoader => "assetloader",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchType {
    FollowJump,
    Overwrite,
    Nop,
    ResolveCall,
}

impl PatchType {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "follow_jump" | "0" => Some(Self::FollowJump),
            "overwrite" | "1" => Some(Self::Overwrite),
            "nop" | "2" => Some(Self::Nop),
            "resolve_call" | "3" => Some(Self::ResolveCall),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    pub folder: String,
    pub debug: bool,
    pub method: LoadMethod,
}

impl Default for Settings {
    fn default() -> Self {
        Self { folder: "AuroraMods".into(), debug: false, method: LoadMethod::SigBypass }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Target {
    pub label: String,
    pub exe: String,
    pub method: LoadMethod,
    pub kind: PatchType,
    pub pattern: Pattern,
    pub offset: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub schema: u32,
    pub settings: Settings,
    pub targets: Vec<Target>,
}
