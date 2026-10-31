use super::config::{Config, LoadMethod, PatchType, Settings, Target};
use crate::process::scan::Pattern;
pub const SCHEMA: u32 = 4;

#[derive(Debug, Default)]
pub struct Parsed {
    pub config: Config,
    pub warnings: Vec<String>,
    pub ignored: Vec<String>,
}

#[derive(Default)]
struct RawTarget {
    line: usize,
    label: String,
    exe: Option<String>,
    method: Option<String>,
    kind: Option<String>,
    pattern: Option<String>,
    offset: Option<String>,
    bytes: Option<String>,
}

enum Section {
    None,
    Settings,
    Target,
    Unknown,
}

pub fn parse(text: &str) -> Parsed {
    let mut out = Parsed::default();
    let mut settings = Settings::default();
    let mut schema = None;
    let mut raw_targets: Vec<RawTarget> = Vec::new();
    let mut section = Section::None;

    for (index, line) in text.lines().enumerate() {
        let n = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = name.trim();
            section = if name.eq_ignore_ascii_case("everlight") {
                Section::Settings
            } else if let Some(label) = target_label(name) {
                let label = if label.is_empty() { format!("#{}", raw_targets.len() + 1) } else { label.to_string() };
                raw_targets.push(RawTarget { line: n, label, ..Default::default() });
                Section::Target
            } else {
                out.ignored.push(format!("line {n}: unknown section [{name}]"));
                Section::Unknown
            };
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            out.ignored.push(format!("line {n}: not a key = value line"));
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();

        match section {
            Section::Settings => match key.as_str() {
                "schema" => match parse_int(value).and_then(|v| u32::try_from(v).ok()) {
                    Some(v) => schema = Some(v),
                    None => out.warnings.push(format!("line {n}: Schema '{value}' is not a number")),
                },
                "folder" => settings.folder = value.to_string(),
                "debug" => match parse_bool(value) {
                    Some(v) => settings.debug = v,
                    None => out.warnings.push(format!("line {n}: Debug '{value}' is not true/false")),
                },
                "method" => match LoadMethod::parse(value) {
                    Some(v) => settings.method = v,
                    None => out.warnings.push(format!("line {n}: unknown Method '{value}'")),
                },
                _ => out.ignored.push(format!("line {n}: unknown key '{key}' in [Everlight]")),
            },
            Section::Target => {
                let target = raw_targets.last_mut().expect("in a target section");
                let slot = match key.as_str() {
                    "exe" => &mut target.exe,
                    "method" => &mut target.method,
                    "type" => &mut target.kind,
                    "pattern" => &mut target.pattern,
                    "offset" => &mut target.offset,
                    "bytes" => &mut target.bytes,
                    _ => {
                        out.ignored.push(format!("line {n}: unknown key '{key}' in target '{}'", target.label));
                        continue;
                    }
                };
                *slot = Some(value.to_string());
            }
            Section::None => out.ignored.push(format!("line {n}: key '{key}' is outside any section")),
            Section::Unknown => {}
        }
    }

    out.config.schema = match schema {
        Some(s) if s != SCHEMA => {
            out.warnings.push(format!("Schema is {s}, expected {SCHEMA}; reading it anyway"));
            s
        }
        Some(s) => s,
        None => SCHEMA,
    };

    for raw in raw_targets {
        let (label, line) = (raw.label.clone(), raw.line);
        match build_target(raw, &settings) {
            Ok(target) => out.config.targets.push(target),
            Err(why) => out.warnings.push(format!("target '{label}' (line {line}) skipped: {why}")),
        }
    }
    out.config.settings = settings;
    out
}

fn target_label(section: &str) -> Option<&str> {
    let head = section.get(..6)?;
    if !head.eq_ignore_ascii_case("target") {
        return None;
    }
    let rest = &section[6..];
    if rest.is_empty() {
        return Some("");
    }
    rest.strip_prefix(':').map(str::trim)
}

fn build_target(raw: RawTarget, settings: &Settings) -> Result<Target, String> {
    let exe = raw.exe.filter(|s| !s.is_empty()).ok_or("no Exe")?;
    let method = match raw.method {
        Some(m) => LoadMethod::parse(&m).ok_or_else(|| format!("unknown Method '{m}'"))?,
        None => settings.method,
    };
    let kind = match raw.kind {
        Some(k) => PatchType::parse(&k).ok_or_else(|| format!("unknown Type '{k}'"))?,
        None => PatchType::FollowJump,
    };
    let pattern = raw.pattern.ok_or("no Pattern")?;
    let pattern = Pattern::parse(&pattern).ok_or("Pattern is not valid hex")?;
    let offset = match raw.offset {
        Some(o) => parse_int(&o)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| format!("Offset '{o}' is not a valid number"))?,
        None => 0,
    };
    let bytes = match raw.bytes {
        Some(b) => parse_hex_bytes(&b).ok_or("Bytes is not valid hex")?,
        None => Vec::new(),
    };
    if method == LoadMethod::SigBypass && kind != PatchType::ResolveCall && bytes.is_empty() {
        return Err("no Bytes".into());
    }
    Ok(Target { label: raw.label, exe, method, kind, pattern, offset, bytes })
}

fn parse_int(s: &str) -> Option<u64> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_hex_bytes(s: &str) -> Option<Vec<u8>> {
    s.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .map(|t| {
            let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
            u8::from_str_radix(t, 16).ok()
        })
        .collect()
}