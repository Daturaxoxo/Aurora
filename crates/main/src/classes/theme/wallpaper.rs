use super::model::ImageRef;
use anyhow::{Context, Result, anyhow};
use image::imageops::FilterType;
use log::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;

include!(concat!(env!("OUT_DIR"), "/theme_backgrounds.rs"));
// constants
const MAX_TARGET_EDGE: u32 = 3840;
const MIN_TARGET_EDGE: u32 = 1280;
const FALLBACK_TARGET_EDGE: u32 = 1920;
const LOOKAHEAD_FRAMES: usize = 2;
const MIN_DELAY: Duration = Duration::from_millis(20);
const DEFAULT_DELAY: Duration = Duration::from_millis(100);
fn target_edge() -> u32 {
    static TARGET_EDGE: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

    *TARGET_EDGE.get_or_init(|| {
        let edge = match shared::display::get_monitor_size() {
            Ok(display) => display
                .width
                .max(display.height)
                .clamp(MIN_TARGET_EDGE, MAX_TARGET_EDGE),
            Err(e) => {
                warn!(
                    "[Theme] could not measure the display ({e}); assuming {FALLBACK_TARGET_EDGE}px"
                );
                FALLBACK_TARGET_EDGE
            }
        };

        info!("[Theme] wallpapers decode to at most {edge}px");
        edge
    })
}

fn target_size(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= target_edge() || longest == 0 {return (width, height)}

    let scale = f64::from(target_edge()) / f64::from(longest);
    (scaled(width, scale), scaled(height, scale))
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn scaled(value: u32, scale: f64) -> u32 {
    (f64::from(value) * scale).round().max(1.0) as u32
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub delay: Duration,
}

pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub enum Wallpaper {
    Still(Decoded),
    Animated {
        width: u32,
        height: u32,
        frames: std::sync::mpsc::Receiver<Frame>,
    },
}

pub fn builtin_bytes(stem: &str) -> Option<&'static [u8]> {
    THEME_BACKGROUNDS
        .iter()
        .find(|(name, _)| *name == stem)
        .map(|(_, bytes)| *bytes)
}

fn cache_path(url: &str) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);

    shared::utils::get_cache_dir()
        .join("Themes")
        .join(format!("{:016x}", hasher.finish()))
}

fn download(url: &str) -> Result<Vec<u8>> {
    let path = cache_path(url);
    match std::fs::read(&path) {
        Ok(bytes) if !bytes.is_empty() => return Ok(bytes),
        _ => debug!("[Theme] {url} is not cached yet"),
    }

    info!("[Theme] downloading wallpaper {url}");
    let bytes = shared::api::download_bytes(url)?;

    if bytes.is_empty() {
        return Err(anyhow!("'{url}' returned an empty body"));
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, &bytes) {
        warn!("[Theme] could not cache '{url}': {e}");
    }

    Ok(bytes)
}

pub fn bytes_for(image: &ImageRef) -> Result<Vec<u8>> {
    match image {
        ImageRef::Builtin(stem) => builtin_bytes(stem)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| anyhow!("no wallpaper named '{stem}' ships with Aurora")),
        ImageRef::Remote(url) => download(url),
        ImageRef::Local(path) => {
            std::fs::read(path).with_context(|| format!("could not read '{}'", path.display()))
        }
    }
}

fn is_gif(bytes: &[u8]) -> bool {
    bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")
}

pub fn open(bytes: Vec<u8>) -> Result<Wallpaper> {
    if is_gif(&bytes) {
        match open_animated(bytes) {
            Ok(wallpaper) => return Ok(wallpaper),
            Err((bytes, e)) => {
                warn!("[Theme] could not read the GIF as an animation ({e:#}); using one frame");
                return Ok(Wallpaper::Still(decode_still(&bytes)?));
            }
        }
    }

    Ok(Wallpaper::Still(decode_still(&bytes)?))
}

fn decode_still(bytes: &[u8]) -> Result<Decoded> {
    let decoded = image::load_from_memory(bytes).context("could not decode the wallpaper")?;
    let (width, height) = (decoded.width(), decoded.height());
    let (target_width, target_height) = target_size(width, height);

    // A still costs one buffer, so it is only ever shrunk to fit the display.
    let scaled = if (target_width, target_height) == (width, height) {
        decoded
    } else {
        info!("[Theme] shrinking the wallpaper from {width}x{height} to fit the display");
        decoded.resize(target_width, target_height, FilterType::Lanczos3)
    };
    let rgba = scaled.into_rgba8();

    Ok(Decoded {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}

/// Reads the GIF's header, then hands the bytes to a worker that decodes frames
/// on demand for as long as anyone is listening.
///
/// Returns the bytes back on failure so the caller can still try them as a still
/// image without re-reading the file.
fn open_animated(bytes: Vec<u8>) -> std::result::Result<Wallpaper, (Vec<u8>, anyhow::Error)> {
    use image::ImageDecoder as _;
    use image::codecs::gif::GifDecoder;

    let probe = |bytes: &[u8]| -> Result<(u32, u32)> {
        let decoder =
            GifDecoder::new(std::io::Cursor::new(bytes)).context("could not open the GIF")?;
        let (width, height) = decoder.dimensions();
        if width == 0 || height == 0 {
            return Err(anyhow!("the GIF has a zero-sized canvas"));
        }
        Ok((width, height))
    };

    let (width, height) = match probe(&bytes) {
        Ok(size) => size,
        Err(e) => return Err((bytes, e)),
    };

    let (target_width, target_height) = target_size(width, height);
    if (target_width, target_height) == (width, height) {
        info!("[Theme] streaming an animated wallpaper at {width}x{height}");
    } else {
        info!(
            "[Theme] streaming an animated wallpaper; it is {width}x{height} and the display              needs {target_width}x{target_height}"
        );
    }

    let (sender, frames) = std::sync::mpsc::sync_channel(LOOKAHEAD_FRAMES);
    std::thread::spawn(move || stream_frames(&bytes, (target_width, target_height), &sender));

    Ok(Wallpaper::Animated {
        width: target_width,
        height: target_height,
        frames,
    })
}

/// Decodes frames in a loop until the receiver goes away.
///
/// The channel is bounded, so this parks on a full queue rather than racing
/// ahead: at rest it wakes once per frame delay and does one frame's work.
fn stream_frames(
    bytes: &[u8],
    (target_width, target_height): (u32, u32),
    sender: &std::sync::mpsc::SyncSender<Frame>,
) {
    use image::AnimationDecoder as _;
    use image::codecs::gif::GifDecoder;

    let resizing = true;
    loop {
        let decoder = match GifDecoder::new(std::io::Cursor::new(bytes)) {
            Ok(decoder) => decoder,
            Err(e) => {
                warn!("[Theme] the animation stopped: {e}");
                return;
            }
        };

        let mut sent = 0_usize;
        for frame in decoder.into_frames() {
            let frame = match frame {
                Ok(frame) => frame,
                Err(e) => {
                    warn!("[Theme] the animation stopped at frame {sent}: {e}");
                    return;
                }
            };

            let (numerator, denominator) = frame.delay().numer_denom_ms();
            let millis = if denominator == 0 {
                0
            } else {
                u64::from(numerator) / u64::from(denominator).max(1)
            };
            let delay = Duration::from_millis(millis);
            // A GIF asking for 0ms means "as fast as sensible", not "spin the CPU".
            let delay = if delay < MIN_DELAY {
                DEFAULT_DELAY
            } else {
                delay
            };

            let buffer = frame.into_buffer();
            let buffer = if resizing
                && (buffer.width(), buffer.height()) != (target_width, target_height)
            {
                image::imageops::resize(&buffer, target_width, target_height, FilterType::Triangle)
            } else {
                buffer
            };

            // A failed send means the theme changed or Aurora is closing.
            if sender
                .send(Frame {
                    width: buffer.width(),
                    height: buffer.height(),
                    pixels: buffer.into_raw(),
                    delay,
                })
                .is_err()
            {
                return;
            }
            sent += 1;
        }

        if sent <= 1 {
            // Nothing to animate, and re-reading would spin. The one frame that
            // was sent stays on screen.
            return;
        }
    }
}

pub fn thumbnail(bytes: &[u8], max_edge: u32) -> Result<Decoded> {
    let decoded = image::load_from_memory(bytes).context("could not decode the wallpaper")?;
    let rgba = decoded.thumbnail(max_edge, max_edge).into_rgba8();
    Ok(Decoded {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}
