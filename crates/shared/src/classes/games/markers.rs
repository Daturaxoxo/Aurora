pub struct GameMarkers {
    pub folder_names: &'static [&'static str],
    pub markers: &'static [&'static str],
}

const NTE: &[&str] = &[
    "NTELauncher.exe",
    "NTEGlobalLauncher.exe",
    "NTETWLauncher.exe",
    "Client",
];

const SP: &[&str] = &["temp"];

pub const MARKERS: &[GameMarkers] = &[
    GameMarkers {
        folder_names: &["Neverness To Everness", "異環", "NTE"],
        markers: NTE,
    },
    GameMarkers {
        folder_names: &["Silver Palace"], // confirm the folder name later! -daturas
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
    match find_game(folder_name) {
        Some(game) => game.folder_names.iter().map(|n| n.to_string()).collect(),
        None => vec![folder_name.to_string()],
    }
}

pub fn folder_name_matches(candidate: &str, folder_name: &str) -> bool {
    match find_game(folder_name) {
        Some(game) => game
            .folder_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case(candidate)),
        None => candidate.eq_ignore_ascii_case(folder_name),
    }
}