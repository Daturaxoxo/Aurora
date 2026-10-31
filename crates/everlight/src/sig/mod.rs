mod config;
mod envelope;
mod parse;
use std::path::Path;
pub use config::{LoadMethod, PatchType, Target};
pub use parse::{Parsed, parse};
pub const FILE_NAME: &str = "everlight.sig";
const MAX_SIZE: u64 = 16 * 1024 * 1024; // 24-bits
pub fn load(path: &Path) -> Result<Parsed, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("not found or unreadable ({e})"))?.len();
    if size > MAX_SIZE {
        return Err(format!("file is too large ({size} bytes)"));
    }
    let raw = std::fs::read(path).map_err(|e| format!("read failed ({e})"))?;
    let text = envelope::unwrap(&raw)?;
    Ok(parse(&text))
}
