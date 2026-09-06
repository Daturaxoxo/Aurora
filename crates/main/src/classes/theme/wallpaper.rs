use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;
use anyhow::{Context, Result, anyhow};
use image::imageops::FilterType;
use log::*;
use super::model::ImageRef;

include!(concat!(env!("OUT_DIR"), "/theme_backgrounds.rs"));

const MAX_DIMENSION: u32 = 1920; // for performance reasons, if someone on a 4K monitor cries that this shit makes it have less quality, plz change this -datura
const FRAME_BUDGET_BYTES: usize = 192 * 1024 * 1024;
const MAX_FRAMES: usize = 240;
const MIN_DIMENSION: u32 = 320;
const MIN_DELAY: Duration = Duration::from_millis(20);
const DEFAULT_DELAY: Duration = Duration::from_millis(100);

pub struct Decoded {
    pub width: u32,
    pub height: u32,
    pub frames: Vec<Vec<u8>>,
    pub delays: Vec<Duration>,
}

impl Decoded {
    pub const fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }
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

pub fn decode(bytes: &[u8]) -> Result<Decoded> {
    if is_gif(bytes) {
        match decode_animated(bytes) {
            Ok(decoded) => return Ok(decoded),
            Err(e) => {
                warn!("[Theme] could not read the GIF as an animation ({e}); using one frame");
            }
        }
    }

    decode_still(bytes)
}

fn decode_still(bytes: &[u8]) -> Result<Decoded> {
    let decoded = image::load_from_memory(bytes).context("could not decode the wallpaper")?;
    let (width, height) = (decoded.width(), decoded.height());

    let scaled = if width.max(height) > MAX_DIMENSION {
        decoded.resize(MAX_DIMENSION, MAX_DIMENSION, FilterType::Lanczos3)
    } else {
        decoded
    };
    let rgba = scaled.into_rgba8();

    Ok(Decoded {
        width: rgba.width(),
        height: rgba.height(),
        frames: vec![rgba.into_raw()],
        delays: Vec::new(),
    })
}

fn decode_animated(bytes: &[u8]) -> Result<Decoded> {
    use image::AnimationDecoder as _;
    use image::codecs::gif::GifDecoder;

    let decoder = GifDecoder::new(std::io::Cursor::new(bytes)).context("could not open the GIF")?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .context("could not decode the GIF frames")?;

    if frames.is_empty() {
        return Err(anyhow!("the GIF holds no frames"));
    }

    let mut buffers = Vec::with_capacity(frames.len());
    let mut delays = Vec::with_capacity(frames.len());
    for frame in frames {
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let millis = if denominator == 0 {
            0
        } else {
            u64::from(numerator) / u64::from(denominator).max(1)
        };
        let delay = Duration::from_millis(millis);

        delays.push(if delay < MIN_DELAY {
            DEFAULT_DELAY
        } else {
            delay
        });
        buffers.push(frame.into_buffer());
    }

    let (width, height) = (buffers[0].width(), buffers[0].height());
    if width == 0 || height == 0 {
        return Err(anyhow!("the GIF has a zero-sized canvas"));
    }

    drop_frames_to_fit(&mut buffers, &mut delays);
    let scale = scale_to_fit(width, height, buffers.len());

    let (width, height) = if scale < 1.0 {
        let target_width = scaled(width, scale);
        let target_height = scaled(height, scale);
        info!(
            "[Theme] scaling the animation from {width}x{height} to {target_width}x{target_height}"
        );

        for buffer in &mut buffers {
            *buffer =
                image::imageops::resize(buffer, target_width, target_height, FilterType::Triangle);
        }
        (target_width, target_height)
    } else {
        (width, height)
    };

    Ok(Decoded {
        width,
        height,
        frames: buffers
            .into_iter()
            .map(image::ImageBuffer::into_raw)
            .collect(),
        delays,
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn scaled(value: u32, scale: f64) -> u32 {
    (f64::from(value) * scale).round().max(1.0) as u32
}

fn drop_frames_to_fit(frames: &mut Vec<image::RgbaImage>, delays: &mut Vec<Duration>) {
    if frames.len() <= MAX_FRAMES {return}

    let step = frames.len().div_ceil(MAX_FRAMES);
    info!(
        "[Theme] the animation has {} frames {step}",
        frames.len()
    );

    let mut kept_frames = Vec::with_capacity(frames.len().div_ceil(step));
    let mut kept_delays = Vec::with_capacity(kept_frames.capacity());
    let mut carried = Duration::ZERO;

    for (index, (frame, delay)) in std::mem::take(frames)
        .into_iter()
        .zip(std::mem::take(delays))
        .enumerate()
    {
        carried += delay;
        if index % step == 0 {
            kept_frames.push(frame);
            kept_delays.push(carried);
            carried = Duration::ZERO;
        }
    }

    if let Some(last) = kept_delays.last_mut() {
        *last += carried;
    }

    *frames = kept_frames;
    *delays = kept_delays;
}

fn scale_to_fit(width: u32, height: u32, frame_count: usize) -> f64 {
    let longest = f64::from(width.max(height));
    let dimension_scale = f64::from(MAX_DIMENSION) / longest;
    let frame_bytes = width as usize * height as usize * 4;
    let total = frame_bytes.saturating_mul(frame_count);
    let budget_scale = if total > FRAME_BUDGET_BYTES {
        (FRAME_BUDGET_BYTES as f64 / total as f64).sqrt()
    } else {1.0};

    let floor = (f64::from(MIN_DIMENSION) / longest).min(1.0);
    dimension_scale.min(budget_scale).clamp(floor, 1.0)
}

pub fn thumbnail(bytes: &[u8], max_edge: u32) -> Result<Decoded> {
    let decoded = image::load_from_memory(bytes).context("could not decode the wallpaper")?;
    let rgba = decoded.thumbnail(max_edge, max_edge).into_rgba8();
    Ok(Decoded {
        width: rgba.width(),
        height: rgba.height(),
        frames: vec![rgba.into_raw()],
        delays: Vec::new(),
    })
}