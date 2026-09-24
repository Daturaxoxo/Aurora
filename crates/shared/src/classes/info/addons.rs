use super::version::Version;
const UNAVAILABLE: &[(&str, &[Version])] = &[];

pub fn is_unavailable(config_key: &str, version: Version) -> bool {
    UNAVAILABLE
        .iter()
        .any(|(key, versions)| *key == config_key && versions.contains(&version))
}
