use std::io;
use std::time::Duration;
// oh my god i love consts i love consts i love consts -d
pub const PIPE_NAME: &str = "aurora-ingame-v1";
const VERSION: u8 = 1;
const KIND_TOAST: u8 = 0;
const ACK_SHOWN: u8 = 0;
const MAX_FRAME: usize = 4096;
const MAX_TITLE_UNITS: usize = 159;
const MAX_SUBTITLE_UNITS: usize = 255;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accent {
    Aurora,
    Success,
    Warn,
    Error,
}

impl Accent {
    const fn rgb(self) -> u32 {
        match self {
            Self::Aurora => 0x7A_C0FF,
            Self::Success => 0x3E_CF6B,
            Self::Warn => 0xF5_BA3C,
            Self::Error => 0xFF_6B6B,
        }
    }
}

fn push_text(buf: &mut Vec<u8>, text: &str, max_units: usize) {
    let units: Vec<u16> = text.encode_utf16().take(max_units).collect();
    #[allow(clippy::cast_possible_truncation)]
    buf.extend_from_slice(&(units.len() as u16).to_le_bytes());
    for unit in units {
        buf.extend_from_slice(&unit.to_le_bytes());
    }
}

pub fn encode_toast(title: &str, subtitle: &str, hold: Duration, accent: Accent) -> Vec<u8> {
    let hold_ms = u32::try_from(hold.as_millis()).unwrap_or(u32::MAX);
    let mut payload = Vec::with_capacity(64);
    payload.push(VERSION);
    payload.push(KIND_TOAST);
    payload.extend_from_slice(&accent.rgb().to_le_bytes());
    payload.extend_from_slice(&hold_ms.to_le_bytes());
    push_text(&mut payload, title, MAX_TITLE_UNITS);
    push_text(&mut payload, subtitle, MAX_SUBTITLE_UNITS);
    debug_assert!(payload.len() <= MAX_FRAME);

    #[allow(clippy::cast_possible_truncation)]
    let mut frame = (payload.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(&payload);
    frame
}

#[cfg(windows)]
pub fn send_toast(title: &str, subtitle: &str, hold: Duration, accent: Accent) -> io::Result<()> {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    const OPEN_ATTEMPTS: u32 = 4;
    const OPEN_RETRY_DELAY: Duration = Duration::from_millis(100);
    const ERROR_PIPE_BUSY: i32 = 231;

    let path = format!(r"\\.\pipe\{PIPE_NAME}");
    let mut attempt = 1;
    let mut pipe = loop {
        match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(pipe) => break pipe,
            Err(e)
                if attempt < OPEN_ATTEMPTS
                    && (e.kind() == io::ErrorKind::NotFound
                        || e.raw_os_error() == Some(ERROR_PIPE_BUSY)) =>
            {
                attempt += 1;
                std::thread::sleep(OPEN_RETRY_DELAY);
            }
            Err(e) => return Err(e),
        }
    };
    pipe.write_all(&encode_toast(title, subtitle, hold, accent))?;

    let mut ack = [0u8; 1];
    pipe.read_exact(&mut ack)?;
    if ack[0] == ACK_SHOWN {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the in-game plugin rejected the toast",
        ))
    }
}

#[cfg(not(windows))]
pub fn send_toast(
    _title: &str,
    _subtitle: &str,
    _hold: Duration,
    _accent: Accent,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "in-game toasts are Windows-only",
    ))
}