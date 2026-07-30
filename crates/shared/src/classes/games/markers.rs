pub struct GameMarkers {
    pub folder_name: &'static str,
    pub markers: &'static [&'static str],
}

const NTE: &[&str] = &[
    "NTELauncher.exe",
    "NTEGlobalLauncher.exe",
    "NTETWLauncher.exe",
    "Client"
];

const SP: &[&str] = &[
    "temp",
];

pub const MARKERS: &[GameMarkers] = &[
    GameMarkers {
        folder_name: "Neverness To Everness",
        markers: NTE
    },
    GameMarkers {
        folder_name: "Silver Palace", // confirm the folder name later! -daturas
        markers: SP
    }
];

pub fn find_marker(folder_name: &str) -> Option<&'static [&'static str]> {
    MARKERS
        .iter()
        .find(|g| g.folder_name == folder_name)
        .map(|g| g.markers)
}