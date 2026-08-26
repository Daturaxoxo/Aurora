use std::collections::BTreeMap;
use std::sync::LazyLock;
use anyhow::{Result, anyhow};
use log::*;
use serde::Deserialize;
use serde_json::Value;
use crate::classes::helpers::ini::{self, Ini, IniFile};
const HARDCODE_GAME: &str = "NTE"; // when we add more games, remove this and actually implement game-based configs; even though there pretty much already is implementation for it
const PLACEHOLDER: &str = "{}";

static CATALOG: LazyLock<BTreeMap<String, Argument>> = LazyLock::new(|| {
    serde_json::from_str(include_str!(
        "../../../../production/engine/NTE/launch_arguments.json"
    ))
    .expect("launch_arguments.json is missing or malformed!")
});

#[derive(Debug, Deserialize)]
struct Argument {
    #[serde(default)]
    fallback: Value,
    writes: Vec<WriteSpec>,
}

#[derive(Debug, Deserialize)]
struct WriteSpec {
    file: Option<String>,
    section: Option<String>,
    key: String,
    value: Value,
    #[serde(default)]
    default: Value,
}

#[derive(Debug)]
struct Target {
    file: IniFile,
    section: String,
    key: String,
}

impl Target {
    fn into_key(self) -> (IniFile, String, String) {
        (self.file, self.section, self.key)
    }
}

impl Argument {
    fn targets(&self, name: &str) -> Vec<(Target, &WriteSpec)> {
        let mut targets = Vec::with_capacity(self.writes.len());
        let mut file: Option<IniFile> = None;
        let mut section: Option<&str> = None;

        for spec in &self.writes {
            if let Some(named) = spec.file.as_deref() {
                let Some(resolved) = IniFile::from_name(named) else {
                    warn!("launch args: {name} writes to unknown file {named:?}, skipped");
                    continue;
                };
                file = Some(resolved);
            }

            if let Some(named) = spec.section.as_deref() {
                section = Some(named);
            }

            let (Some(file), Some(section)) = (file, section) else {
                warn!("launch args: the first write of {name} must name a file and a section");
                continue;
            };

            targets.push((
                Target {
                    file,
                    section: section.to_string(),
                    key: spec.key.clone(),
                },
                spec,
            ));
        }

        targets
    }
}

#[derive(Debug)]
struct Active {
    name: String,
    value: Option<String>,
}

impl Argument {
    fn is_parameterised(&self) -> bool {
        self.writes
            .iter()
            .any(|write| render(&write.value).is_some_and(|value| value.contains(PLACEHOLDER)))
    }

    fn substitution(&self, typed: Option<&str>) -> Option<String> {
        typed
            .map(ToString::to_string)
            .or_else(|| render(&self.fallback))
    }

    fn accepts(&self, value: &str) -> bool {
        if self.fallback.is_number() {
            value.parse::<f64>().is_ok()
        } else {
            !value.trim().is_empty()
        }
    }
}

fn render(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) if text.trim().is_empty() => None,
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn parse(raw: &str) -> Vec<Active> {
    let mut found: Vec<Active> = Vec::new();

    for token in raw.split_whitespace() {
        let (name, value) = token
            .split_once('=')
            .map_or((token, None), |(name, value)| (name, Some(value)));

        let name = format!("-{}", name.trim_start_matches('-').to_ascii_lowercase());

        let Some(argument) = CATALOG.get(&name) else {
            debug!("launch args: ignoring unsupported argument {token:?}");
            continue;
        };

        if let Some(value) = value
            && !argument.accepts(value)
        {
            warn!("launch args: {name} cannot take the value {value:?}, ignored");
            continue;
        }

        found.retain(|existing| existing.name != name);
        found.push(Active {
            name,
            value: value.map(ToString::to_string),
        });
    }

    found
}

pub fn apply(raw: &str) -> Result<()> {
    if !ini::SUPPORTED {
        debug!("launch args: unsupported platform, nothing to do");
        return Ok(());
    }

    if ini::config_dirs().is_empty() {
        return Err(anyhow!(
            "the game's configuration folder was not found, start the game once first"
        ));
    }

    let active = parse(raw);
    info!(
        "launch args: applying {raw:?} for {HARDCODE_GAME}, {} of {} arguments recognised",
        active.len(),
        raw.split_whitespace().count()
    );

    // "None" will delete the key
    let mut plan: BTreeMap<(IniFile, String, String), Option<String>> = BTreeMap::new();
    for (name, argument) in &*CATALOG {
        if active.iter().any(|typed| typed.name == *name) {
            continue;
        }

        for (target, spec) in argument.targets(name) {
            plan.insert(target.into_key(), render(&spec.default));
        }
    }

    for typed in &active {
        let name = &typed.name;
        let Some(argument) = CATALOG.get(name) else {
            continue;
        };

        let substitution = argument.substitution(typed.value.as_deref());
        if argument.is_parameterised() && substitution.is_none() {
            warn!("launch args: {name} needs a value, as in {name}=1, skipped");
            continue;
        }

        for (target, spec) in argument.targets(name) {
            let Some(value) = render(&spec.value) else {
                warn!(
                    "launch args: {name} has no value for {}, skipped",
                    target.key
                );
                continue;
            };

            let value = match &substitution {
                Some(substitution) => value.replace(PLACEHOLDER, substitution),
                None => value,
            };

            plan.insert(target.into_key(), Some(value));
        }
    }

    commit(&plan)
}

fn commit(plan: &BTreeMap<(IniFile, String, String), Option<String>>) -> Result<()> {
    for file in IniFile::ALL.iter().copied() {
        let mut batch = Ini::file(file);
        let mut edits = 0_usize;

        for ((target, section, key), value) in plan {
            if *target != file {
                continue;
            }

            batch = match value {
                Some(value) => batch.set(section, key, value.clone()),
                None => batch.remove(section, key),
            };
            edits += 1;
        }

        if edits == 0 {
            continue;
        }

        let report = batch.commit()?;
        debug!(
            "launch args: {file}, {edits} edits, {} of {} copies updated",
            report.written, report.found
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_is_valid() {
        for (name, argument) in &*CATALOG {
            assert!(name.starts_with('-'), "{name} should start with a dash");
            assert_eq!(
                *name,
                name.to_ascii_lowercase(),
                "{name} has to be lowercase to be matched"
            );
            assert!(!argument.writes.is_empty(), "{name} writes nothing");

            assert!(
                !argument.is_parameterised() || render(&argument.fallback).is_some(),
                "{name} takes a value, so it needs a fallback"
            );

            let targets = argument.targets(name);
            assert_eq!(
                targets.len(),
                argument.writes.len(),
                "{name} has a write that could not be resolved"
            );

            for (_, spec) in targets {
                assert!(render(&spec.value).is_some(), "{name} has an empty value");
            }
        }
    }

    fn names(raw: &str) -> Vec<String> {
        parse(raw).into_iter().map(|typed| typed.name).collect()
    }

    #[test]
    fn only_known_arguments_are_parsed() {
        assert_eq!(names("-dx11 -nonsense"), vec!["-dx11"]);
        assert!(names("").is_empty());
    }

    #[test]
    fn parsing_is_forgiving_about_dashes_and_case() {
        assert_eq!(names("   -NoFpsLimit   "), vec!["-nofpslimit"]);
        assert_eq!(names("--MaxAniso=8"), vec!["-maxaniso"]);
    }

    #[test]
    fn a_repeated_argument_keeps_its_last_position() {
        assert_eq!(names("-dx11 -sharpen -dx11"), vec!["-sharpen", "-dx11"]);
    }

    #[test]
    fn a_typed_value_is_kept() {
        let typed = parse("-sharpen=1.5");
        assert_eq!(typed[0].value.as_deref(), Some("1.5"));
    }

    #[test]
    fn a_numeric_argument_rejects_junk() {
        assert!(names("-maxaniso=high").is_empty());
        assert_eq!(names("-maxaniso=16"), vec!["-maxaniso"]);
    }

    #[test]
    fn a_bare_parameterised_argument_falls_back() {
        let argument = &CATALOG["-sharpen"];
        assert!(argument.is_parameterised());
        assert_eq!(argument.substitution(None), render(&argument.fallback));
        assert_eq!(argument.substitution(Some("3")), Some("3".to_string()));
    }
}
