pub struct GameMarkers {
    pub folder_names: &'static [&'static str],
    pub markers: &'static [&'static str],
}

const NTE: &[&str] = &[
    "NTELauncher.exe",
    "NTEGlobalLauncher.exe",
    "NTETWLauncher.exe",
    "Client/WindowsNoEditor/HT/Content/Paks",
];

const SP: &[&str] = &["temp"];

pub const MARKERS: &[GameMarkers] = &[
    GameMarkers {
        folder_names: &["Neverness To Everness", "異環", "NTE"],
        markers: NTE,
    },
    GameMarkers {
        folder_names: &["Silver Palace"], // TODO: confirm the folder name later! -daturas
        markers: SP,
    },
];

fn find_game(name: &str) -> Option<&'static GameMarkers> {
    MARKERS
        .iter()
        .find(|g| g.folder_names.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

pub fn find_marker(folder_name: &str) -> Option<&'static [&'static str]> {
    find_game(folder_name).map(|g| g.markers)
}

pub fn known_folder_names(folder_name: &str) -> Vec<String> {
    find_game(folder_name).map_or_else(
        || vec![folder_name.to_string()],
        |game| game.folder_names.iter().map(ToString::to_string).collect(),
    )
}

pub fn folder_name_matches(candidate: &str, folder_name: &str) -> bool {
    find_game(folder_name).map_or_else(
        || candidate.eq_ignore_ascii_case(folder_name),
        |game| {
            game.folder_names
                .iter()
                .any(|n| n.eq_ignore_ascii_case(candidate))
        },
    )
}
