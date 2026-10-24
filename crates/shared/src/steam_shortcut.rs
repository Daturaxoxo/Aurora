use anyhow::{Context, Result, anyhow, bail};
use log::*;
use std::path::{Path, PathBuf};

const APP_NAME: &str = "Aurora";

const TYPE_MAP: u8 = 0x00;
const TYPE_STRING: u8 = 0x01;
const TYPE_INT: u8 = 0x02;
const TYPE_END: u8 = 0x08;

enum Value {
    Map(Vec<(Vec<u8>, Self)>),
    Str(Vec<u8>),
    Int(u32),
}

pub fn add() -> Result<bool> {
    let exe = launcher_exe()?;
    let start_dir = exe
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", exe.display()))?;
    let icon = icon_path();

    let root = steam_root().ok_or_else(|| anyhow!("could not find a Steam installation"))?;
    let accounts = accounts(&root.join("userdata"));
    if accounts.is_empty() {
        bail!("no Steam account has signed in on this computer");
    }

    let exe = quote(&exe);
    let start_dir = quote(start_dir);
    let mut changed = false;
    for account in accounts {
        let path = account.join("config").join("shortcuts.vdf");
        changed |= add_to(&path, &exe, &start_dir, icon.as_deref())
            .with_context(|| format!("could not update {}", path.display()))?;
    }
    Ok(changed)
}

fn add_to(path: &Path, exe: &str, start_dir: &str, icon: Option<&Path>) -> Result<bool> {
    let mut shortcuts = match std::fs::read(path) {
        Ok(bytes) => parse(&bytes)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.into()),
    };

    let icon = icon.map_or_else(String::new, |icon| icon.display().to_string());
    let existing = shortcuts.iter_mut().find_map(|(_, entry)| match entry {
        Value::Map(fields) if is_aurora(fields, exe) => Some(fields),
        _ => None,
    });

    if let Some(fields) = existing {
        if string_field(fields, "Exe").is_some_and(|e| e.eq_ignore_ascii_case(exe.as_bytes())) {
            debug!("{} already has Aurora", path.display());
            return Ok(false);
        }
        info!("updating the moved Aurora shortcut in {}", path.display());
        set_string(fields, "Exe", exe);
        set_string(fields, "StartDir", start_dir);
        set_string(fields, "icon", &icon);
    } else {
        info!("adding Aurora to {}", path.display());
        let index = shortcuts
            .iter()
            .filter_map(|(key, _)| std::str::from_utf8(key).ok()?.parse::<usize>().ok())
            .max()
            .map_or(0, |i| i + 1);
        shortcuts.push((index.to_string().into_bytes(), entry(exe, start_dir, &icon)));
    }

    let mut out = Vec::new();
    write_map(&mut out, b"shortcuts", &shortcuts);
    out.push(TYPE_END);

    let tmp = path.with_extension("vdf.tmp");
    std::fs::create_dir_all(path.parent().unwrap_or(path))?;
    std::fs::write(&tmp, &out)?;
    std::fs::rename(&tmp, path)?;
    Ok(true)
}

fn is_aurora(fields: &[(Vec<u8>, Value)], exe: &str) -> bool {
    string_field(fields, "Exe").is_some_and(|e| e.eq_ignore_ascii_case(exe.as_bytes()))
        || string_field(fields, "AppName") == Some(APP_NAME.as_bytes())
}

fn entry(exe: &str, start_dir: &str, icon: &str) -> Value {
    let s = |key: &str, value: &str| (key.as_bytes().to_vec(), Value::Str(value.into()));
    let i = |key: &str, value: u32| (key.as_bytes().to_vec(), Value::Int(value));

    let app_id = crc32fast::hash(format!("{exe}{APP_NAME}").as_bytes()) | 0x8000_0000;
    Value::Map(vec![
        i("appid", app_id),
        s("AppName", APP_NAME),
        s("Exe", exe),
        s("StartDir", start_dir),
        s("icon", icon),
        s("ShortcutPath", ""),
        s("LaunchOptions", ""),
        i("IsHidden", 0),
        i("AllowDesktopConfig", 1),
        i("AllowOverlay", 1),
        i("OpenVR", 0),
        i("Devkit", 0),
        s("DevkitGameID", ""),
        i("DevkitOverrideAppID", 0),
        i("LastPlayTime", 0),
        s("FlatpakAppID", ""),
        (b"tags".to_vec(), Value::Map(Vec::new())),
    ])
}

fn string_field<'a>(fields: &'a [(Vec<u8>, Value)], name: &str) -> Option<&'a [u8]> {
    fields.iter().find_map(|(key, value)| match value {
        Value::Str(s) if key.eq_ignore_ascii_case(name.as_bytes()) => Some(s.as_slice()),
        _ => None,
    })
}

fn set_string(fields: &mut Vec<(Vec<u8>, Value)>, name: &str, text: &str) {
    let slot = fields
        .iter_mut()
        .find(|(key, _)| key.eq_ignore_ascii_case(name.as_bytes()));
    match slot {
        Some((_, value)) => *value = Value::Str(text.into()),
        None => fields.push((name.as_bytes().to_vec(), Value::Str(text.into()))),
    }
}

fn parse(bytes: &[u8]) -> Result<Vec<(Vec<u8>, Value)>> {
    let mut pos = 0;
    if bytes.first() != Some(&TYPE_MAP) {
        bail!("not a binary VDF file");
    }
    pos += 1;
    let _name = read_cstr(bytes, &mut pos)?;
    read_map(bytes, &mut pos)
}

fn read_map(bytes: &[u8], pos: &mut usize) -> Result<Vec<(Vec<u8>, Value)>> {
    let mut fields = Vec::new();
    loop {
        let kind = *bytes
            .get(*pos)
            .ok_or_else(|| anyhow!("truncated VDF file"))?;
        *pos += 1;
        if kind == TYPE_END {
            return Ok(fields);
        }
        let key = read_cstr(bytes, pos)?;
        let value = match kind {
            TYPE_MAP => Value::Map(read_map(bytes, pos)?),
            TYPE_STRING => Value::Str(read_cstr(bytes, pos)?),
            TYPE_INT => {
                let raw = bytes
                    .get(*pos..*pos + 4)
                    .ok_or_else(|| anyhow!("truncated VDF file"))?;
                *pos += 4;
                Value::Int(u32::from_le_bytes(raw.try_into()?))
            }
            other => bail!("unsupported VDF field type {other:#04x}"),
        };
        fields.push((key, value));
    }
}

fn read_cstr(bytes: &[u8], pos: &mut usize) -> Result<Vec<u8>> {
    let rest = bytes.get(*pos..).unwrap_or_default();
    let len = rest
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(|| anyhow!("truncated VDF file"))?;
    *pos += len + 1;
    Ok(rest[..len].to_vec())
}

fn write_map(out: &mut Vec<u8>, name: &[u8], fields: &[(Vec<u8>, Value)]) {
    out.push(TYPE_MAP);
    write_cstr(out, name);
    for (key, value) in fields {
        match value {
            Value::Map(children) => write_map(out, key, children),
            Value::Str(text) => {
                out.push(TYPE_STRING);
                write_cstr(out, key);
                write_cstr(out, text);
            }
            Value::Int(number) => {
                out.push(TYPE_INT);
                write_cstr(out, key);
                out.extend_from_slice(&number.to_le_bytes());
            }
        }
    }
    out.push(TYPE_END);
}

fn write_cstr(out: &mut Vec<u8>, text: &[u8]) {
    out.extend_from_slice(text);
    out.push(0);
}

fn quote(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

fn accounts(userdata: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(userdata) else {
        warn!("could not read {}", userdata.display());
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name != "0" && name.bytes().all(|b| b.is_ascii_digit())
        })
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

fn launcher_exe() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(appimage) = ipc::appimage_path() {
        return Ok(appimage);
    }
    std::env::current_exe().context("could not resolve the Aurora executable")
}

#[cfg(target_os = "linux")]
fn icon_path() -> Option<PathBuf> {
    crate::desktop_entry::install_icon()
        .inspect_err(|e| warn!("could not write the Steam shortcut icon: {e}"))
        .ok()
}

#[cfg(not(target_os = "linux"))]
const fn icon_path() -> Option<PathBuf> {
    None
}

#[cfg(target_os = "linux")]
fn steam_root() -> Option<PathBuf> {
    crate::classes::steam::find_steam_root().or_else(|| {
        let flatpak = crate::classes::steam::real_home()?
            .join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        flatpak.join("userdata").is_dir().then_some(flatpak)
    })
}

#[cfg(windows)]
fn steam_root() -> Option<PathBuf> {
    const HKEY_CURRENT_USER: isize = -2_147_483_647;
    crate::pathfind::registry_string(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath")
        .into_iter()
        .chain([PathBuf::from(r"C:\Program Files (x86)\Steam")])
        .find(|root| root.join("userdata").is_dir())
}
