use super::{GameDescriptor, identity::SafeRelativePath, registry};

fn find_game(name: &str) -> Option<&'static GameDescriptor> {
    registry()
        .iter()
        .map(|game| game.descriptor())
        .find(|d| d.aliases.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

pub fn find_marker(folder_name: &str) -> Option<&'static [SafeRelativePath]> {
    find_game(folder_name).map(|d| d.markers.as_slice())
}

pub fn known_folder_names(folder_name: &str) -> Vec<String> {
    find_game(folder_name).map_or_else(
        || vec![folder_name.to_string()],
        |d| d.aliases.iter().map(ToString::to_string).collect(),
    )
}

pub fn folder_name_matches(candidate: &str, folder_name: &str) -> bool {
    find_game(folder_name).map_or_else(
        || candidate.eq_ignore_ascii_case(folder_name),
        |d| d.aliases.iter().any(|n| n.eq_ignore_ascii_case(candidate)),
    )
}
