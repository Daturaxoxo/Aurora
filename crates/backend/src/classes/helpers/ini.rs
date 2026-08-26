use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use anyhow::{Context as _, Result, anyhow};
use log::*;
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}
// if we ever add Mac support (never lol), just add macos here
pub const SUPPORTED: bool = cfg!(any(target_os = "windows", target_os = "linux"));

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum IniFile {
    Engine,
    GameUserSettings,
}

impl IniFile {
    pub const ALL: &'static [Self] = &[Self::Engine, Self::GameUserSettings];

    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Engine => "Engine.ini",
            Self::GameUserSettings => "GameUserSettings.ini",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|file| file.file_name().eq_ignore_ascii_case(name))
    }
}

impl std::fmt::Display for IniFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.file_name())
    }
}

#[cfg(target_os = "windows")]
fn saved_config_dirs() -> Vec<PathBuf> {
    let local_app_data = std::env::var("LOCALAPPDATA").unwrap_or_default();
    let ht = PathBuf::from(local_app_data).join("HT");

    let Ok(entries) = ht.read_dir() else {
        debug!("ini: {} does not exist", ht.display());
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|dir| dir.file_name().to_string_lossy().contains("Saved"))
        .map(|dir| dir.path().join("Config").join("Windows"))
        .collect()
}

#[cfg(unix)]
fn saved_config_dirs() -> Vec<PathBuf> {
    use shared::classes::steam;

    let Some(prefix) = steam::aurora_prefix() else {
        debug!("ini: no Proton prefix yet");
        return Vec::new();
    };

    let ht = prefix
        .join("drive_c")
        .join("users")
        .join("steamuser")
        .join("AppData")
        .join("Local")
        .join("HT");

    let Ok(entries) = ht.read_dir() else {
        debug!("ini: {} does not exist", ht.display());
        return Vec::new();
    };

    entries
        .flatten()
        .filter(|dir| dir.file_name().to_string_lossy().contains("Saved_Global"))
        .map(|dir| dir.path().join("Config").join("Windows"))
        .collect()
}

#[cfg(not(any(target_os = "windows", unix)))]
const fn saved_config_dirs() -> Vec<PathBuf> {
    Vec::new()
}

pub fn config_dirs() -> Vec<PathBuf> {
    if SUPPORTED {
        saved_config_dirs()
    } else {
        Vec::new()
    }
}

pub fn paths(file: IniFile) -> Vec<PathBuf> {
    config_dirs()
        .into_iter()
        .map(|dir| dir.join(file.file_name()))
        .collect()
}

pub fn value(file: IniFile, section: &str, key: &str) -> Option<String> {
    let _guard = lock();
    let section = normalize_section(section);

    paths(file).iter().find_map(|path| {
        let text = fs::read_to_string(path).ok()?;
        value_in(&text, &section, key)
    })
}

#[derive(Clone, Debug)]
struct Edit {
    section: String,
    key: String,
    value: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Ini {
    file: IniFile,
    edits: Vec<Edit>,
    read_only: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Report {
    pub found: usize,
    pub written: usize,
}

impl Ini {
    pub const fn file(file: IniFile) -> Self {
        Self {
            file,
            edits: Vec::new(),
            read_only: true,
        }
    }

    #[must_use]
    pub fn set(mut self, section: &str, key: &str, value: impl Into<String>) -> Self {
        self.edits.push(Edit {
            section: normalize_section(section),
            key: key.to_string(),
            value: Some(value.into()),
        });
        self
    }

    #[must_use]
    pub fn remove(mut self, section: &str, key: &str) -> Self {
        self.edits.push(Edit {
            section: normalize_section(section),
            key: key.to_string(),
            value: None,
        });
        self
    }

    #[must_use]
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn commit(self) -> Result<Report> {
        if self.edits.is_empty() {
            return Ok(Report::default());
        }

        let _guard = lock();

        let paths = paths(self.file);
        let mut report = Report {
            found: paths.len(),
            written: 0,
        };

        if paths.is_empty() {
            warn!("ini: found no {} to write", self.file);
            return Ok(report);
        }

        for path in &paths {
            if self.write(path)? {
                report.written += 1;
            }
        }

        Ok(report)
    }

    /// Returns whether the file needed changing at all.
    fn write(&self, path: &Path) -> Result<bool> {
        let original = fs::read_to_string(path).ok();
        let updated = edited(original.as_deref().unwrap_or_default(), &self.edits);

        if original.as_deref() == Some(updated.as_str()) {
            trace!("ini: {} is already up to date", path.display());
            return Ok(false);
        }

        if original.is_none() && updated.trim().is_empty() {
            trace!("ini: nothing to write into {}", path.display());
            return Ok(false);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }

        set_read_only(path, false);

        if let Some(existing) = &original {
            let backup = sibling(path, "bak");
            if let Err(e) = fs::write(&backup, existing) {
                warn!("ini: could not back {} up: {e}", path.display());
            }
        }

        let tmp = sibling(path, "tmp");
        let written = fs::write(&tmp, &updated).and_then(|()| fs::rename(&tmp, path));

        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);

            if let Some(existing) = &original {
                warn!("ini: restoring {} after a failed write", path.display());
                let _ = fs::write(path, existing);
                if self.read_only {
                    set_read_only(path, true);
                }
            }

            return Err(anyhow!("could not write {}: {e}", path.display()));
        }

        if self.read_only {
            set_read_only(path, true);
        }

        info!("ini: updated {}", path.display());
        Ok(true)
    }
}

fn set_read_only(path: &Path, read_only: bool) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };

    let mut perms = metadata.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(read_only);
    let _ = fs::set_permissions(path, perms);
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.{suffix}"))
}

fn normalize_section(section: &str) -> String {
    section
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim()
        .to_string()
}

fn section_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    Some(trimmed.strip_prefix('[')?.strip_suffix(']')?.trim())
}

fn is_key(line: &str, key: &str) -> bool {
    line.split_once('=')
        .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(key))
}

fn value_in(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;

    for line in text.lines() {
        if let Some(name) = section_name(line) {
            inside = name.eq_ignore_ascii_case(section);
            continue;
        }

        if inside && is_key(line, key) {
            return line
                .split_once('=')
                .map(|(_, value)| value.trim().to_string());
        }
    }

    None
}

fn edited(text: &str, edits: &[Edit]) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(ToString::to_string).collect();

    for edit in edits {
        apply_edit(&mut lines, edit);
    }

    let mut out = lines.join(eol);
    if !out.is_empty() {
        out.push_str(eol);
    }
    out
}

fn apply_edit(lines: &mut Vec<String>, edit: &Edit) {
    let Some((header, end)) = section_bounds(lines, &edit.section) else {
        let Some(value) = &edit.value else {return};

        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("[{}]", edit.section));
        lines.push(format!("{}={value}", edit.key));
        return;
    };

    let matches: Vec<usize> = (header + 1..end)
        .filter(|&i| is_key(&lines[i], &edit.key))
        .collect();

    for &i in matches.iter().skip(1).rev() {
        lines.remove(i);
    }

    match (&edit.value, matches.first()) {
        (Some(value), Some(&i)) => lines[i] = format!("{}={value}", edit.key),
        (Some(value), None) => {
            let at = (header + 1..end)
                .rev()
                .find(|&i| !lines[i].trim().is_empty())
                .map_or(header + 1, |i| i + 1);
            lines.insert(at, format!("{}={value}", edit.key));
        }
        (None, Some(&i)) => {
            lines.remove(i);
        }
        (None, None) => {}
    }
}

fn section_bounds(lines: &[String], section: &str) -> Option<(usize, usize)> {
    let header = lines.iter().position(|line| {
        section_name(line).is_some_and(|name| name.eq_ignore_ascii_case(section))
    })?;

    let end = lines
        .iter()
        .skip(header + 1)
        .position(|line| section_name(line).is_some())
        .map_or(lines.len(), |i| header + 1 + i);

    Some((header, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(text: &str, section: &str, key: &str, value: &str) -> String {
        edited(
            text,
            &[Edit {
                section: normalize_section(section),
                key: key.to_string(),
                value: Some(value.to_string()),
            }],
        )
    }

    fn remove(text: &str, section: &str, key: &str) -> String {
        edited(
            text,
            &[Edit {
                section: normalize_section(section),
                key: key.to_string(),
                value: None,
            }],
        )
    }

    #[test]
    fn replaces_a_key_and_leaves_its_neighbours_alone() {
        let text = "[/Script/Engine.GameUserSettings]\nResolutionSizeX=1920\nFullscreenMode=0\n";
        let out = set(
            text,
            "[/Script/Engine.GameUserSettings]",
            "FullscreenMode",
            "2",
        );
        assert_eq!(
            out,
            "[/Script/Engine.GameUserSettings]\nResolutionSizeX=1920\nFullscreenMode=2\n"
        );
    }

    #[test]
    fn appends_a_missing_key_to_its_section() {
        let text = "[A]\nOne=1\n\n[B]\nTwo=2\n";
        assert_eq!(
            set(text, "A", "Three", "3"),
            "[A]\nOne=1\nThree=3\n\n[B]\nTwo=2\n"
        );
    }

    #[test]
    fn appends_a_missing_section() {
        assert_eq!(
            set("[A]\nOne=1\n", "B", "Two", "2"),
            "[A]\nOne=1\n\n[B]\nTwo=2\n"
        );
    }

    #[test]
    fn removes_only_the_named_key() {
        let text = "[A]\nOne=1\nTwo=2\n";
        assert_eq!(remove(text, "A", "One"), "[A]\nTwo=2\n");
        assert_eq!(remove(text, "A", "Missing"), text);
    }

    #[test]
    fn matches_sections_and_keys_regardless_of_case() {
        let text = "[a]\nONE=1\n";
        assert_eq!(set(text, "A", "one", "2"), "[a]\none=2\n");
    }

    #[test]
    fn keeps_the_line_endings_the_file_came_with() {
        let text = "[A]\r\nOne=1\r\n";
        assert_eq!(set(text, "A", "One", "2"), "[A]\r\nOne=2\r\n");
    }

    #[test]
    fn drops_duplicate_keys() {
        let text = "[A]\nOne=1\nOne=2\nTwo=3\n";
        assert_eq!(set(text, "A", "One", "9"), "[A]\nOne=9\nTwo=3\n");
    }

    #[test]
    fn writing_into_nothing_creates_the_section() {
        assert_eq!(set("", "A", "One", "1"), "[A]\nOne=1\n");
    }

    #[test]
    fn reads_a_value_back() {
        let text = "[A]\nOne=1\n\n[B]\nOne=2\n";
        assert_eq!(value_in(text, "B", "One"), Some("2".to_string()));
        assert_eq!(value_in(text, "C", "One"), None);
    }
}
