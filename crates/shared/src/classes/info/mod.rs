//! Constants, functions and structs related to NTE's files, versions, and directories.

pub mod paths;
pub mod version;

pub const NTE_PROCESSES: &[&str] = &[
    // GL
    "ntegloballauncher.exe",
    "nteglobal.exe",
    "nteglobalgame.exe",
    // CN
    "ntelauncher.exe",
    "ntegame.exe",
    // TW
    "ntetwlauncher.exe",
    "ntetwgame.exe",
    // ALL
    "htgame.exe",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    AsiPlugin,
    CNntfrmain,
    GLntfrmain,
    Cutils,
    Ntfrsub,
}

impl Target {
    pub const fn as_file(&self) -> &'static str {
        match self {
            Self::AsiPlugin => "Everlight.asi",
            Self::CNntfrmain => "cnntfrmain.asi",
            Self::GLntfrmain => "glntfrmain.asi",
            Self::Cutils => "cutils.dll",
            Self::Ntfrsub => "cnntfrsub.dll",
        }
    }
}
