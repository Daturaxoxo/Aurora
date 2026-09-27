pub mod addons;
pub mod patcher;
pub mod paths;
pub mod version;

pub use crate::classes::games::nte::{NTE_GAME_EXE, NTE_PROCESSES};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    AsiPlugin,
    AuroraTf,
    CNAuroraTF,
    Cutils,
}

impl Target {
    pub const fn as_file(&self) -> &'static str {
        match self {
            Self::AsiPlugin => "Everlight.asi",
            Self::AuroraTf => "AuroraTF.asi",
            Self::CNAuroraTF => "CNAuroraTF.asi",
            Self::Cutils => "cutils.dll",
        }
    }
}
