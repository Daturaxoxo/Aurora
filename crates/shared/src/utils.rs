const VERSION: &[u8] = include_bytes!("../../../production/VERSION");

pub fn get_local_version() -> String {
    String::from_utf8_lossy(VERSION).to_string()
}
