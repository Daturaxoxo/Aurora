const MAGIC: &[u8; 4] = b"ELS1";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 4 + 1 + 4;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

pub fn unwrap(raw: &[u8]) -> Result<String, String> {
    let payload = if raw.starts_with(MAGIC) {
        if raw.len() < HEADER_LEN {
            return Err("envelope is truncated".into());
        }
        if raw[4] != VERSION {
            return Err(format!("envelope version {} is not supported", raw[4]));
        }
        let stored = u32::from_le_bytes([raw[5], raw[6], raw[7], raw[8]]);
        let payload = &raw[HEADER_LEN..];
        if crc32(payload) != stored {
            return Err("CRC mismatch (file is corrupted)".into());
        }
        payload
    } else {
        raw
    };
    let payload = payload.strip_prefix(UTF8_BOM).unwrap_or(payload);
    Ok(String::from_utf8_lossy(payload).into_owned())
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
