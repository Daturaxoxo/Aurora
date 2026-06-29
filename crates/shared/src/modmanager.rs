use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use log::*;

use crate::utils::{get_mods_path, read_dir_recursive};

const GROUP_PREFIX: &str = "AU GRP - ";

#[derive(Debug, Clone)]
pub struct Group {
    pub name: Option<String>,
    pub path: Option<PathBuf>,
    pub mods: Vec<Mod>,
}

impl Group {
    pub const fn new(name: Option<String>, path: Option<PathBuf>) -> Self {
        Self {
            name,
            path,
            mods: vec![],
        }
    }

    pub const fn new_with_mods(
        name: Option<String>,
        path: Option<PathBuf>,
        mods: Vec<Mod>,
    ) -> Self {
        Self { name, path, mods }
    }

    pub fn add_mod(&mut self, mod_: Mod) {
        self.mods.push(mod_);
    }

    pub fn remove_mod(&mut self, index: usize) {
        self.mods.remove(index);
    }

    pub fn remove_mod_by_name(&mut self, name: &str) {
        self.mods.retain(|mod_| mod_.folder_name != name);
    }
}

#[derive(Debug, Clone)]
pub struct Mod {
    pub folder_name: String,
    pub display_name: String,
    pub path: PathBuf,
    pub group: Option<Group>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub support_link: Option<String>,
    pub icon: Option<String>,
    pub is_enabled: bool,
    pub has_json: bool,
}

impl Default for Mod {
    fn default() -> Self {
        Self {
            folder_name: String::new(),
            display_name: String::new(),
            path: PathBuf::new(),
            group: None,
            version: Some("Unknown".to_string()),
            author: Some("Unknown".to_string()),
            support_link: None,
            icon: None,
            is_enabled: false,
            has_json: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModManager {
    pub mods: Vec<Mod>,
}

impl Default for ModManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ModManager {
    pub const fn new() -> Self {
        Self { mods: vec![] }
    }

    pub fn add_mod(&mut self, mod_: Mod) {
        self.mods.push(mod_);
    }

    pub fn remove_mod(&mut self, index: usize) {
        self.mods.remove(index);
    }

    pub fn remove_mod_by_name(&mut self, name: &str) {
        self.mods.retain(|mod_| mod_.folder_name != name);
    }

    pub fn get_mod_by_name(&self, name: &str) -> Option<&Mod> {
        self.mods.iter().find(|mod_| mod_.folder_name == name)
    }

    fn get_mod_data(folder: &PathBuf) -> Option<Mod> {
        let mod_name = folder.file_name().unwrap().to_str()?;

        let files = read_dir_recursive(folder);
        let is_enabled = files
            .iter()
            .filter(|p| {
                Path::new(p.file_name().to_str().unwrap())
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
            })
            .count()
            > 0;

        let mut mod_data = Mod {
            folder_name: mod_name.to_string(),
            display_name: mod_name.to_string().replace("_P", ""),
            path: folder.clone(),
            is_enabled,
            ..Default::default()
        };

        let mut json_path = folder.join("mod.json");
        if !json_path.exists() {
            for sub in folder.read_dir().ok()? {
                let sub = sub.ok()?;
                if sub.file_type().ok()?.is_dir() && sub.path().join("mod.json").exists() {
                    json_path = sub.path().join("mod.json");
                    break;
                }
            }

            if !json_path.exists() {
                return Some(mod_data);
            }
        }

        let json = std::fs::read_to_string(json_path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&json).ok()?;
        mod_data.has_json = true;

        let binding = serde_json::Map::new();
        let optionals = json["Optionals"].as_object().unwrap_or(&binding);

        let mut support_link = optionals
            .iter()
            .find(|(k, _)| *k.to_lowercase() == *"support link")
            .and_then(|(_, v)| v.as_str().map(ToString::to_string));
        if let Some(link) = &mut support_link {
            if !link.starts_with("http://") && !link.starts_with("https://") {
                support_link = format!("https://{link}").into();
            }
        }

        let mut icon = json["Icon"].as_str().map(ToString::to_string);
        let mut custom_image_url = optionals
            .iter()
            .find(|(k, _)| *k.to_lowercase() == *"custom image url")
            .and_then(|(_, v)| v.as_str().map(ToString::to_string));
        if custom_image_url.is_some() {
            let url = custom_image_url.as_mut().unwrap();
            if !url.starts_with("http://") && !url.starts_with("https://") {
                custom_image_url = format!("https://{url}").into();
            }
            icon = custom_image_url;
        }
        Some(Mod {
            display_name: json["Name"]
                .as_str()
                .or(Some(&mod_data.display_name))
                .map(ToString::to_string)?,
            version: json["Version"]
                .as_str()
                .or(Some("1.0.0"))
                .map(ToString::to_string),
            author: json["Author"]
                .as_str()
                .or(Some("Unknown"))
                .map(ToString::to_string),
            support_link,
            icon,
            ..mod_data
        })
    }

    pub fn scan_mods(&mut self) -> Option<Vec<Group>> {
        let mods_path = get_mods_path()?;

        if !mods_path.exists() {
            return Some(vec![]);
        }

        let mut groups: Vec<Group> = vec![];

        // Mods that don't have a group
        let mut root_group = Group::new(None, None);

        for entry in mods_path.read_dir().ok()? {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_dir() {
                continue;
            }

            if entry.file_name().to_str()?.starts_with(GROUP_PREFIX) {
                let group_name = entry.file_name().to_str()?.replace(GROUP_PREFIX, "");
                let mut group = Group::new(Some(group_name), Some(entry.path()));
                // TODO: idk if this should be recursive or not @daturas
                for item in read_dir_recursive(&entry.path()) {
                    if Path::new(item.file_name().to_str()?)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
                        || item.file_name().to_str()?.ends_with(".pak.disabled")
                    {
                        let mod_ = Self::get_mod_data(&item.parent_path().to_path_buf())?;
                        group.add_mod(mod_);
                    }
                }
                if !group.mods.is_empty() {
                    group.mods.sort_by(|a, b| a.folder_name.cmp(&b.folder_name));
                }
                groups.push(group);
            } else {
                for item in read_dir_recursive(&entry.path()) {
                    if Path::new(item.file_name().to_str()?)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
                        || item.file_name().to_str()?.ends_with(".pak.disabled")
                    {
                        let mod_ = Self::get_mod_data(&item.parent_path().to_path_buf())?;
                        root_group.add_mod(mod_);
                    }
                }
            }
        }

        if !root_group.mods.is_empty() {
            root_group
                .mods
                .sort_by(|a, b| a.folder_name.cmp(&b.folder_name));

            groups.insert(0, root_group);
        }

        Some(groups)
    }

    pub fn toggle_mod(&mut self, mod_: &Mod) -> Result<()> {
        let folder = mod_.path.clone();
        if !folder.exists() {
            return Err(anyhow!(
                "Cannot toggle mod: folder not found for {}",
                mod_.folder_name
            ));
        }
        let files = read_dir_recursive(&folder);

        if mod_.is_enabled {
            let targets = files
                .iter()
                .filter(|p| {
                    Path::new(p.file_name().to_str().unwrap())
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
                })
                .collect::<Vec<_>>();

            if targets.is_empty() {
                return Err(anyhow!(
                    "Cannot toggle mod: no .pak files found for {}",
                    mod_.folder_name
                ));
            }

            for pak in &targets {
                pak.path().set_file_name(format!(
                    "{}.disabled",
                    pak.file_name()
                        .to_str()
                        .ok_or_else(|| anyhow!("Could not get file name"))?
                ));
            }
            info!(
                "Mod disabled: renamed {} file(s) in {}",
                targets.len(),
                mod_.folder_name
            );
        } else {
            let targets = files
                .iter()
                .filter(|p| p.file_name().to_str().unwrap().ends_with(".pak.disabled"))
                .collect::<Vec<_>>();

            if targets.is_empty() {
                return Err(anyhow!(
                    "Cannot toggle mod: no .pak.disabled files found for {}",
                    mod_.folder_name
                ));
            }

            for pak in &targets {
                pak.path()
                    .set_file_name(pak.file_name().to_str().unwrap().replace(".disabled", ""));
            }
            info!(
                "Mod enabled: renamed {} file(s) in {}",
                targets.len(),
                mod_.folder_name
            );
        }

        Ok(())
    }
}
