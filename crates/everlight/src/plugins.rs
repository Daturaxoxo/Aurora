use crate::paths;
use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleW, LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW,
};
const TAG: &str = "Plugins";
const WAIT_ENV: &str = "EVERLIGHT_WAIT_RENDERER";
const RENDERER_MODULES: [&str; 3] = ["d3d12.dll\0", "dxgi.dll\0", "d3d11.dll\0"];

fn renderer_loaded() -> bool {
    RENDERER_MODULES.iter().any(|name| {
        let wide: Vec<u16> = name.encode_utf16().collect();
        !unsafe { GetModuleHandleW(wide.as_ptr()) }.is_null()
    })
}

fn wait_for_renderer() {
    const MAX_WAIT: Duration = Duration::from_secs(10);
    const POLL: Duration = Duration::from_millis(50);
    const SETTLE: Duration = Duration::from_millis(500);

    let start = Instant::now();
    while start.elapsed() < MAX_WAIT {
        if renderer_loaded() {
            log_debug!(TAG, "Renderer up after {:?}; settling {SETTLE:?} before plugins.", start.elapsed());
            std::thread::sleep(SETTLE);
            return;
        }
        std::thread::sleep(POLL);
    }
    log_warn!(TAG, "Renderer not detected within {MAX_WAIT:?}; loading plugins anyway.");
}

pub fn load_all() {
    if std::env::var_os(WAIT_ENV).is_some() {
        wait_for_renderer();
    }

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
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let module = unsafe {
            LoadLibraryExW(
                wide.as_ptr(),
                std::ptr::null_mut(),
                LOAD_WITH_ALTERED_SEARCH_PATH,
            )
        };
        if module.is_null() {
            log_warn!(TAG, "Failed to load {name} (err={}).", unsafe {
                GetLastError()
            });
        } else {
            log_info!(TAG, "Loaded {name} (base 0x{:X}).", module as usize);
        }
    }
}
