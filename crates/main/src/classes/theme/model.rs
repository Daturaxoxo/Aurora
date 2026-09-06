use std::path::{Path, PathBuf};
use anyhow::{Result, anyhow};
use log::*;
use slint::Color;

pub const EXTENSION: &str = "autheme";
pub const DEFAULT_ID: &str = "aurora";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageRef {
    Builtin(String),
    Remote(String),
    Local(PathBuf),
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub author: String,
    pub builtin: bool,
    pub path: Option<PathBuf>,
    pub image: Option<ImageRef>,
    pub background: (Color, Color),
    pub border: (Color, Color),
    pub shadow: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub surface: Color,
    pub backdrop: Color,
    pub scrim: (Color, Color),
    pub accent: Color,
    pub accent_strong: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    pub favorite: Color,
}

mod fallback {
    pub const BACKGROUND: (u32, u32) = (0xdd11_1115, 0xdd16_161a);
    pub const BORDER: (u32, u32) = (0x25ff_ffff, 0x0cff_ffff);
    pub const SHADOW: u32 = 0xaa00_0000;
    pub const TEXT_PRIMARY: u32 = 0xffff_ffff;
    pub const TEXT_SECONDARY: u32 = 0xffaa_aaaa;
    pub const ACCENT: u32 = 0xff6d_b3f2;
    pub const ACCENT_STRONG: u32 = 0xff3b_82f6;
    pub const DANGER: u32 = 0xffee_4937;
    pub const WARNING: u32 = 0xffe8_a020;
    pub const SUCCESS: u32 = 0xff22_c55e;
    pub const FAVORITE: u32 = 0xffff_d75e;
}

const fn argb(value: u32) -> Color {
    Color::from_argb_encoded(value)
}

impl Default for Theme {
    fn default() -> Self {
        let background = (argb(fallback::BACKGROUND.0), argb(fallback::BACKGROUND.1));
        let shadow = argb(fallback::SHADOW);
        let text_primary = argb(fallback::TEXT_PRIMARY);

        Self {
            id: DEFAULT_ID.to_string(),
            name: "Aurora".to_string(),
            author: "Aurora Team".to_string(),
            builtin: true,
            path: None,
            image: Some(ImageRef::Builtin("background".to_string())),

            background,
            border: (argb(fallback::BORDER.0), argb(fallback::BORDER.1)),
            shadow,
            text_primary,
            text_secondary: argb(fallback::TEXT_SECONDARY),
            surface: derive_surface(background.1, text_primary),
            backdrop: derive_backdrop(background.1),
            scrim: derive_scrim(shadow),
            accent: argb(fallback::ACCENT),
            accent_strong: argb(fallback::ACCENT_STRONG),
            danger: argb(fallback::DANGER),
            warning: argb(fallback::WARNING),
            success: argb(fallback::SUCCESS),
            favorite: argb(fallback::FAVORITE),
        }
    }
}

fn derive_surface(background_bottom: Color, text_primary: Color) -> Color {
    opaque(background_bottom).mix(&text_primary, 0.95)
}

fn derive_backdrop(background_bottom: Color) -> Color {
    opaque(background_bottom).darker(0.35)
}

fn derive_scrim(shadow: Color) -> (Color, Color) {
    (shadow.with_alpha(0.267), shadow.with_alpha(0.533))
}

fn opaque(color: Color) -> Color {
    color.with_alpha(1.0)
}

#[derive(Default)]
pub struct Optionals {
    pub surface: Option<Color>,
    pub backdrop: Option<Color>,
    pub scrim: Option<(Color, Color)>,
    pub accent: Option<Color>,
    pub accent_strong: Option<Color>,
    pub danger: Option<Color>,
    pub warning: Option<Color>,
    pub success: Option<Color>,
    pub favorite: Option<Color>,
}

impl Theme {
    fn apply_optionals(&mut self, optional: &Optionals) {
        self.surface = optional
            .surface
            .unwrap_or_else(|| derive_surface(self.background.1, self.text_primary));
        self.backdrop = optional
            .backdrop
            .unwrap_or_else(|| derive_backdrop(self.background.1));
        self.scrim = optional.scrim.unwrap_or_else(|| derive_scrim(self.shadow));
        self.accent = optional.accent.unwrap_or_else(|| argb(fallback::ACCENT));
        self.accent_strong = optional
            .accent_strong
            .or(optional.accent)
            .unwrap_or_else(|| argb(fallback::ACCENT_STRONG));
        self.danger = optional.danger.unwrap_or_else(|| argb(fallback::DANGER));
        self.warning = optional.warning.unwrap_or_else(|| argb(fallback::WARNING));
        self.success = optional.success.unwrap_or_else(|| argb(fallback::SUCCESS));
        self.favorite = optional
            .favorite
            .or_else(|| {
                optional
                    .warning
                    .map(|w| w.mix(&Color::from_rgb_u8(255, 255, 255), 0.55))
            })
            .unwrap_or_else(|| argb(fallback::FAVORITE));
    }
}

pub fn parse_color(raw: &str) -> Result<Color> {
    let hex = raw.trim().trim_start_matches('#');
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(anyhow!("'{raw}' is not a hex colour"));
    }

    let widen = |c: char| {
        let d = u8::try_from(c.to_digit(16).unwrap_or(0)).unwrap_or(0);
        d << 4 | d
    };
    let chars: Vec<char> = hex.chars().collect();

    let (r, g, b, a) = match chars.len() {
        3 => (widen(chars[0]), widen(chars[1]), widen(chars[2]), 0xff),
        4 => (
            widen(chars[0]),
            widen(chars[1]),
            widen(chars[2]),
            widen(chars[3]),
        ),
        6 | 8 => {
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
            let alpha = if chars.len() == 8 { byte(6) } else { 0xff };
            (byte(0), byte(2), byte(4), alpha)
        }
        _ => return Err(anyhow!("'{raw}' is not a 3, 4, 6 or 8 digit hex colour")),
    };

    Ok(Color::from_argb_u8(a, r, g, b))
}

fn parse_pair(raw: &str) -> Result<(Color, Color)> {
    let mut parts = raw.split(',');
    let first = parse_color(parts.next().unwrap_or_default())?;

    match parts.next() {
        Some(second) => Ok((first, parse_color(second)?)),
        None => Ok((first, first)),
    }
}

fn resolve_image(raw: &str, folder: Option<&Path>) -> Option<ImageRef> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("none") {return None}
    if raw.starts_with("http://") || raw.starts_with("https://") {return Some(ImageRef::Remote(raw.to_string()))}

    let path = Path::new(raw);
    let local = if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        folder.map(|folder| folder.join(path))
    };
    if let Some(local) = &local
        && local.is_file()
    {
        return Some(ImageRef::Local(local.clone()));
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(raw)
        .to_lowercase();
    if path.parent() == Some(Path::new("")) && super::wallpaper::builtin_bytes(&stem).is_some() {
        return Some(ImageRef::Builtin(stem));
    }
    local.map(ImageRef::Local)
}

pub fn parse(path: &Path) -> Result<Theme> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("could not read '{}': {e}", path.display()))?;

    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("'{}' has no usable file name", path.display()))?
        .to_string();

    let folder = path.parent();
    let mut theme = Theme {
        id: id.clone(),
        name: id,
        author: String::new(),
        builtin: false,
        path: Some(path.to_path_buf()),
        image: None,
        ..Theme::default()
    };
    let mut optional = Optionals::default();
    let mut required_seen = 0_u32;

    for (number, line) in contents.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }

        let Some((key, value)) = line.split_once('|') else {
            warn!(
                "[Theme] {}:{}: no '|' on this line, skipping it",
                path.display(),
                number + 1
            );
            continue;
        };
        let (key, value) = (key.trim().to_ascii_uppercase(), value.trim());
        let color = |target: &mut Color| match parse_color(value) {
            Ok(parsed) => {
                *target = parsed;
                true
            }
            Err(e) => {
                warn!("[Theme] {}:{}: {e}", path.display(), number + 1);
                false
            }
        };

        match key.as_str() {
            "NAME" => theme.name = value.to_string(),
            "AUTHOR" => theme.author = value.to_string(),
            "IMAGE" => theme.image = resolve_image(value, folder),

            "BACKGROUND" | "BORDER" | "OVERLAY" => match parse_pair(value) {
                Ok(pair) => match key.as_str() {
                    "BACKGROUND" => {
                        theme.background = pair;
                        required_seen += 1;
                    }
                    "BORDER" => {
                        theme.border = pair;
                        required_seen += 1;
                    }
                    _ => optional.scrim = Some(pair),
                },
                Err(e) => warn!("[Theme] {}:{}: {e}", path.display(), number + 1),
            },

            "SHADOW" => {
                if color(&mut theme.shadow) {
                    required_seen += 1;
                }
            }
            "PRIMARY_TEXT" => {
                if color(&mut theme.text_primary) {
                    required_seen += 1;
                }
            }
            "SECONDARY_TEXT" => {
                if color(&mut theme.text_secondary) {
                    required_seen += 1;
                }
            }

            "SURFACE" => optional.surface = parse_color(value).ok(),
            "BACKDROP" => optional.backdrop = parse_color(value).ok(),
            "ACCENT" => optional.accent = parse_color(value).ok(),
            "ACCENT_STRONG" => optional.accent_strong = parse_color(value).ok(),
            "DANGER" => optional.danger = parse_color(value).ok(),
            "WARNING" => optional.warning = parse_color(value).ok(),
            "SUCCESS" => optional.success = parse_color(value).ok(),
            "FAVORITE" | "FAVOURITE" => optional.favorite = parse_color(value).ok(),

            other => debug!(
                "[Theme] {}:{}: ignoring unknown key '{other}'",
                path.display(),
                number + 1
            ),
        }
    }

    if required_seen == 0 {
        return Err(anyhow!(
            "'{}' does not set a single theme colour; is it really a theme?",
            path.display()
        ));
    }

    theme.apply_optionals(&optional);
    Ok(theme)
}

#[derive(serde::Deserialize)]
struct BuiltinEntry {
    id: String,
    name: String,
    #[serde(default)]
    author: String,
    background: (String, String),
    border: (String, String),
    shadow: String,
    primary_text: String,
    secondary_text: String,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    surface: Option<String>,
    #[serde(default)]
    backdrop: Option<String>,
    #[serde(default)]
    overlay: Option<(String, String)>,
    #[serde(default)]
    accent: Option<String>,
    #[serde(default)]
    accent_strong: Option<String>,
    #[serde(default)]
    danger: Option<String>,
    #[serde(default)]
    warning: Option<String>,
    #[serde(default)]
    success: Option<String>,
    #[serde(default)]
    favorite: Option<String>,
}

impl BuiltinEntry {
    fn into_theme(self) -> Result<Theme> {
        let optional_color = |raw: Option<String>| raw.as_deref().and_then(|c| parse_color(c).ok());
        let pair = |(a, b): (String, String)| -> Result<(Color, Color)> {
            Ok((parse_color(&a)?, parse_color(&b)?))
        };

        let mut theme = Theme {
            id: self.id,
            name: self.name,
            author: self.author,
            builtin: true,
            path: None,
            image: self
                .image
                .as_deref()
                .and_then(|raw| resolve_image(raw, None)),
            background: pair(self.background)?,
            border: pair(self.border)?,
            shadow: parse_color(&self.shadow)?,
            text_primary: parse_color(&self.primary_text)?,
            text_secondary: parse_color(&self.secondary_text)?,
            ..Theme::default()
        };

        theme.apply_optionals(&Optionals {
            surface: optional_color(self.surface),
            backdrop: optional_color(self.backdrop),
            scrim: self
                .overlay
                .and_then(|(a, b)| Some((parse_color(&a).ok()?, parse_color(&b).ok()?))),
            accent: optional_color(self.accent),
            accent_strong: optional_color(self.accent_strong),
            danger: optional_color(self.danger),
            warning: optional_color(self.warning),
            success: optional_color(self.success),
            favorite: optional_color(self.favorite),
        });

        Ok(theme)
    }
}

const BUILTIN_JSON: &str = include_str!("../../../../../production/assets/themes.json");

pub fn builtins() -> Vec<Theme> {
    let entries: Vec<BuiltinEntry> = match serde_json::from_str(BUILTIN_JSON) {
        Ok(entries) => entries,
        Err(e) => {
            error!("[Theme] themes.json is malformed ({e}); falling back to the stock theme");
            return vec![Theme::default()];
        }
    };

    let mut themes: Vec<Theme> = entries
        .into_iter()
        .filter_map(|entry| {
            let id = entry.id.clone();
            entry
                .into_theme()
                .map_err(|e| error!("[Theme] built-in theme '{id}' is malformed: {e}"))
                .ok()
        })
        .collect();

    if themes.is_empty() {
        themes.push(Theme::default());
    }
    themes
}

pub fn user_dir() -> PathBuf {shared::config::get_userdata_path().join("themes")}

pub fn list() -> Vec<Theme> {
    let mut themes = builtins();

    let dir = user_dir();
    let mut imported: Vec<Theme> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case(EXTENSION))
        })
        .filter_map(|path| {
            parse(&path)
                .map_err(|e| warn!("[Theme] skipping '{}': {e}", path.display()))
                .ok()
        })
        .filter(|theme| !themes.iter().any(|builtin| builtin.id == theme.id))
        .collect();

    imported.sort_by_key(|theme| theme.name.to_lowercase());
    themes.append(&mut imported);
    themes
}

pub fn find(themes: &[Theme], id: &str) -> Theme {
    themes
        .iter()
        .find(|theme| theme.id == id)
        .or_else(|| themes.first())
        .cloned()
        .unwrap_or_default()
}