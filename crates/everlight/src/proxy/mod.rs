#[cfg(feature = "dbghelp")]
pub mod dbghelp;
#[cfg(feature = "dwmapi")]
pub mod dwmapi;
#[cfg(feature = "version")]
pub mod version;
#[cfg(feature = "xinput1_4")]
pub mod xinput1_4;
#[cfg(feature = "winhttp")]
pub mod winhttp;
#[cfg(feature = "winmm")]
pub mod winmm;
use crate::paths;
#[cfg(feature = "dbghelp")]
use dbghelp as active;
#[cfg(feature = "dwmapi")]
use dwmapi as active;
use std::os::windows::ffi::OsStrExt;
#[cfg(feature = "version")]
use version as active;
use windows_sys::Win32::System::LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW};
#[cfg(feature = "winhttp")]
use winhttp as active;
#[cfg(feature = "winmm")]
use winmm as active;
#[cfg(feature = "xinput1_4")]
use xinput1_4 as active;
const TAG: &str = "Proxy";
const ENGINE_DLL: &str = "Everlight.asi";
pub fn start() {
    resolve_exports();
    #[cfg(feature = "dwmapi")]
    unsafe {
        std::env::set_var("EVERLIGHT_WAIT_RENDERER", "1")
    };
    load_engine();
}

fn resolve_exports() {
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    let dll = active::DLL_NAME;
    unsafe {
        let mut dir = [0u16; 260];
        let len = GetSystemDirectoryW(dir.as_mut_ptr(), dir.len() as u32) as usize;
        if len == 0 || len > dir.len() {
            log_error!(
                TAG,
                "GetSystemDirectory failed - cannot locate the real {dll}."
            );
            return;
        }
        let mut path: Vec<u16> = dir[..len].to_vec();
        path.extend(
            std::path::Path::new("\\")
                .join(dll)
                .as_os_str()
                .encode_wide(),
        );
        path.push(0);
        let real = LoadLibraryW(path.as_ptr());
        if real.is_null() {
            log_error!(TAG, "Could not load the real {dll} from System32.");
            return;
        }
        let filled = active::fill(real);
        log_debug!(
            TAG,
            "Forwarding {dll}: {filled}/{} exports resolved.",
            active::COUNT
        );
    }
}

fn load_engine() {
    let spawned = std::thread::Builder::new()
        .name("everlight-proxy".into())
        .spawn(|| {
            let path = paths::module_dir().join(ENGINE_DLL);
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let module = unsafe {
                LoadLibraryExW(
                    wide.as_ptr(),
                    std::ptr::null_mut(),
                    LOAD_WITH_ALTERED_SEARCH_PATH,
                )
            };
            if module.is_null() {
                log_error!(TAG, "Could not load {ENGINE_DLL} from the game folder.");
            } else {
                log_info!(
                    TAG,
                    "Loaded engine {ENGINE_DLL} (base 0x{:X}).",
                    module as usize
                );
            }
        });
    if let Err(e) = spawned {
        log_error!(TAG, "Could not start the engine-loader thread: {e}");
    }
}
