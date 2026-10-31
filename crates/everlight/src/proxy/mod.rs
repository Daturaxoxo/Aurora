#[cfg(feature = "version")]
pub mod version;
#[cfg(feature = "version")]
pub use version as active;
#[cfg(feature = "version")]
pub const DLL_NAME: &str = "version.dll";
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::System::LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW};
use crate::paths;
const TAG: &str = "Proxy";
const ENGINE_DLL: &str = "Everlight.asi";

pub fn start() {
    #[cfg(feature = "version")]
    resolve_exports();
    load_engine();
}

#[cfg(feature = "version")]
fn resolve_exports() {
    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    unsafe {
        let mut dir = [0u16; 260];
        let len = GetSystemDirectoryW(dir.as_mut_ptr(), dir.len() as u32) as usize;
        if len == 0 || len > dir.len() {
            log_error!(TAG, "GetSystemDirectory failed - cannot locate the real {DLL_NAME}.");
            return;
        }
        let mut path: Vec<u16> = dir[..len].to_vec();
        path.extend(std::path::Path::new("\\").join(DLL_NAME).as_os_str().encode_wide());
        path.push(0);
        let real = LoadLibraryW(path.as_ptr());
        if real.is_null() {
            log_error!(TAG, "Could not load the real {DLL_NAME} from System32.");
            return;
        }
        let filled = active::fill(real);
        log_debug!(TAG, "Forwarding {DLL_NAME}: {filled}/{} exports resolved.", active::COUNT);
    }
}

fn load_engine() {
    let spawned = std::thread::Builder::new().name("everlight-proxy".into()).spawn(|| {
        let path = paths::module_dir().join(ENGINE_DLL);
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
        if module.is_null() {
            log_error!(TAG, "Could not load {ENGINE_DLL} from the game folder.");
        } else {
            log_info!(TAG, "Loaded engine {ENGINE_DLL} (base 0x{:X}).", module as usize);
        }
    });
    if let Err(e) = spawned {
        log_error!(TAG, "Could not start the engine-loader thread: {e}");
    }
}
