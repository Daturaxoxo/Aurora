use std::{
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Result, anyhow};

const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const RESERVED: &[&str] = &["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"];
const RESERVED_NUMBERED: &[&str] = &["COM", "LPT"];
const RESERVED_DIGITS: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '¹', '²', '³',
];

fn validate_component(component: &str) -> Result<()> {
    if component.is_empty() {
        return Err(anyhow!("path component is empty"));
    }
    if component == "." || component == ".." {
        return Err(anyhow!("path component {component} is a traversal"));
    }
    if let Some(ch) = component
        .chars()
        .find(|c| INVALID_CHARS.contains(c) || c.is_control())
    {
        return Err(anyhow!("path component {component} contains {ch:?}"));
    }
    if component.ends_with(['.', ' ']) {
        return Err(anyhow!(
            "path component {component} ends with a dot or space"
        ));
    }

    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .trim_end_matches(' ');
    let reserved = RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r))
        || RESERVED_NUMBERED.iter().any(|prefix| {
            let mut chars = stem.chars();
            let head: String = chars.by_ref().take(3).collect();
            let tail: Vec<char> = chars.collect();
            head.eq_ignore_ascii_case(prefix)
                && tail.len() == 1
                && RESERVED_DIGITS.contains(&tail[0])
        });
    if reserved {
        return Err(anyhow!(
            "path component {component} is a reserved device name"
        ));
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SafeRelativePath {
    components: Vec<String>,
}

impl SafeRelativePath {
    pub fn parse(raw: &str) -> Result<Self> {
        if raw.is_empty() {
            return Err(anyhow!("path is empty"));
        }
        if raw.starts_with(['/', '\\']) {
            return Err(anyhow!("path is rooted"));
        }

        let components = raw
            .split(['/', '\\'])
            .map(|component| validate_component(component).map(|()| component.to_string()))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { components })
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }

    pub fn file_name(&self) -> &str {
        self.components.last().map_or("", String::as_str)
    }

    pub fn resolve_under(&self, root: &Path) -> PathBuf {
        self.components
            .iter()
            .fold(root.to_path_buf(), |path, component| path.join(component))
    }
}

impl fmt::Display for SafeRelativePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.components.join("/"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameId(String);

impl GameId {
    pub fn parse(raw: &str) -> Result<Self> {
        if raw.is_empty() {
            return Err(anyhow!("game id is empty"));
        }
        if raw.contains(['/', '\\']) {
            return Err(anyhow!("game id '{raw}' has more than one component"));
        }
        if raw.chars().any(char::is_uppercase) {
            return Err(anyhow!("game id '{raw}' is not lowercase"));
        }
        validate_component(raw)
            .map_err(|e| anyhow!("game id is not a safe directory name: {e}"))?;
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
