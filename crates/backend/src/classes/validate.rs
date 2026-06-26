use std::{fs, path::PathBuf};

use anyhow::{anyhow, Result};
use scandir::Walk;

const ARCHIVE_EXTENSIONS: [&str; 7] = [".zip", ".rar", ".7z", ".tar", ".gz", ".bz2", ".xz"];
const MOD_EXTENSIONS: [&str; 3] = [".pak", ".utoc", ".ucas"];
const IGNORED_INI_FILES: [&str; 1] = ["desktop.ini"];

#[derive(Debug, Clone)]
pub struct Issue {
    pub file: String,
    pub issue: String,
}

impl Issue {
    #[must_use]
    pub const fn new(file: String, issue: String) -> Self {
        Self { file, issue }
    }
}

pub fn validate_mods(mod_folder: impl Into<PathBuf>) -> Result<Vec<Issue>> {
    let mod_folder = mod_folder.into();
    let mut issues = vec![];

    if !mod_folder.exists() {
        return Ok(issues);
    }

    for entry in fs::read_dir(mod_folder)? {
        let entry = entry?;
        let path = entry.path();
        let extension = &path
            .extension()
            .and_then(|os| os.to_str())
            .ok_or_else(|| anyhow!("Could not get file extension"))?;
        if entry.file_type()?.is_file() {
            let name = entry
                .path()
                .to_str()
                .ok_or_else(|| anyhow!("Could not get file path"))?
                .to_string();
            if ARCHIVE_EXTENSIONS.contains(extension) {
                issues.push(Issue::new(
                    name,
                    "Archive File: You must extract the mod first".to_string(),
                ));
            } else if !MOD_EXTENSIONS.contains(extension) {
                issues.push(Issue::new(
                    name,
                    format!("Unsupported file type ({extension})"),
                ));
            }
        } else if entry.file_type()?.is_dir() {
            let toc = Walk::new(entry.path(), None)?.collect()?;
            for file in toc.files() {
                if IGNORED_INI_FILES.iter().any(|f| *f == file) {
                    continue;
                }
                issues.push(Issue::new(
                    format!("{}/{file:#?}", entry.file_name().display()),
                    "INI mod: This mod is made for 3DMigoto, not Aurora.".to_string(),
                ));
            }
            for arc in fs::read_dir(entry.path())? {
                let arc = arc?;
                if arc.file_type()?.is_file()
                    && ARCHIVE_EXTENSIONS.contains(
                        &arc.path()
                            .extension()
                            .ok_or_else(|| anyhow!("Could not get file extension"))?
                            .to_str()
                            .unwrap(),
                    )
                {
                    issues.push(Issue::new(
                        format!(
                            "{}/{}",
                            entry.file_name().display(),
                            arc.file_name().display()
                        ),
                        "Nested archive: Extract the inner mod first".to_string(),
                    ));
                }
            }
        }
    }

    Ok(issues)
}

pub fn validate_builtins(
    bin_dir: impl Into<PathBuf>,
    required_names: Vec<String>,
) -> Result<Vec<String>> {
    let bin_dir = bin_dir.into();
    let mut res = vec![];
    for name in required_names {
        if bin_dir.join(&name).exists() {
            continue;
        }
        res.push(name);
    }
    Ok(res)
}

pub fn ensure_dir(path: PathBuf) -> Result<()> {
    if path.exists() && !path.is_dir() {
        fs::remove_file(path.clone())?;
    }
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}
