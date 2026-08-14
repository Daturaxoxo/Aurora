use std::fs;
use std::path::{Path, PathBuf};
use log::*;
use super::version::Version;
const PATCHER_CONFIG: &[&str] = &["UserData", "Patcher", "PatcherSDK", "config.xml"];
const RES_VERSION_TAG: &str = "ResVersion";

fn data(version: Version) -> Option<&'static str> {
    version.spec().launcher_process.strip_suffix("Launcher.exe")
}

fn config_in(data: &Path) -> Option<PathBuf> {
    let config = PATCHER_CONFIG
        .iter()
        .fold(data.to_path_buf(), |path, part| path.join(part));
    config.is_file().then_some(config)
}

pub fn config_path(game_path: &Path, version: Version) -> Option<PathBuf> {
    if let Some(config) = data(version).and_then(|f| config_in(&game_path.join(f))) {
        return Some(config);
    }
    fs::read_dir(game_path)
        .ok()?
        .flatten()
        .find_map(|entry| config_in(&entry.path()))
}

fn text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = xml.find(&format!("<{tag}"))?;
    let text = open + xml[open..].find('>')? + 1;
    let close = text + xml[text..].find(&format!("</{tag}>"))?;
    Some(xml[text..close].trim())
}

pub fn res_version(game_path: &Path, version: Version) -> Option<String> {
    let config = config_path(game_path, version)?;

    let xml = match fs::read(&config) {
        Ok(data) => String::from_utf8_lossy(&data).into_owned(),
        Err(e) => {
            warn!("Could not read patcher config {}: {e}", config.display());
            return None;
        }
    };

    let res_version = text(&xml, RES_VERSION_TAG).filter(|v| !v.is_empty());
    if res_version.is_none() {
        warn!(
            "Patcher config {} has no <{RES_VERSION_TAG}>",
            config.display()
        );
    }

    res_version.map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"<?xml version="1.0" encoding="utf-8" ?>
<config>
    <LocalBranch>publish_PC</LocalBranch>
    <ResVersion>1.2.26</ResVersion>
    <AppVersion>0.0</AppVersion>
    <UpdateResVersion>0.0</UpdateResVersion>
    <BaseVerson appVersion="0.0">
        <Res section="0.103" version="0.103.45" Tag="pakchunk103" ResSize="1230793604" />
    </BaseVerson>
</config>"#;

    #[test]
    fn reads_res_version() {
        assert_eq!(text(CONFIG, RES_VERSION_TAG), Some("1.2.26"));
    }

    #[test]
    fn reads_tags_with_attributes() {
        assert!(text(CONFIG, "BaseVerson").is_some_and(|t| t.contains("pakchunk103")));
    }

    #[test]
    fn missing_tag_is_none() {
        assert_eq!(text(CONFIG, "Hash"), None);
        assert_eq!(text("", RES_VERSION_TAG), None);
    }

    #[test]
    fn derives_data_from_the_launcher() {
        assert_eq!(data(Version::Global), Some("NTEGlobal"));
        assert_eq!(data(Version::CN), Some("NTE"));
        assert_eq!(data(Version::TW), Some("NTETW"));
        assert_eq!(data(Version::Unknown), None);
    }
}
