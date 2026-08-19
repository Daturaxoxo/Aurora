use super::version::Version;
const UNAVAILABLE: &[(&str, &[Version])] = &[
    ("drv_lin", &[Version::CN, Version::Global]),
    ("col_tim", &[Version::CN, Version::Global]),
    ("collectibles", &[Version::CN, Version::Global]),
];

pub fn is_unavailable(config_key: &str, version: Version) -> bool {
    UNAVAILABLE
        .iter()
        .any(|(key, versions)| *key == config_key && versions.contains(&version))
}