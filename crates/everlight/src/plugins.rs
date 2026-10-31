use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW};
use crate::paths;
const TAG: &str = "Plugins";
pub fn load_all() {
    let dir = paths::module_dir().join("Plugins");
    let read = match std::fs::read_dir(&dir) {
        Ok(read) => read,
        Err(_) => {
            log_debug!(TAG, "No Plugins folder at {}.", dir.display());
            return;
        }
    };
    let mut asi: Vec<_> = read
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("asi")))
        .collect();
    asi.sort();
    for path in asi {
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
        if module.is_null() {
            log_warn!(TAG, "Failed to load {name} (err={}).", unsafe { GetLastError() });
        } else {
            log_info!(TAG, "Loaded {name} (base 0x{:X}).", module as usize);
        }
    }
}
