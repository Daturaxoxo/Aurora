const VERSION: &[u8] = include_bytes!("../../../production/VERSION");

pub fn get_local_version() -> String {
    String::from_utf8_lossy(VERSION).to_string()
}

pub fn get_current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .cast_signed()
}
