#[macro_use]
mod log;
mod paths;
#[cfg(not(feature = "proxy"))]
mod boot;
#[cfg(not(feature = "proxy"))]
mod loaders;
#[cfg(not(feature = "proxy"))]
mod plugins;
#[cfg(not(feature = "proxy"))]
mod process;
#[cfg(not(feature = "proxy"))]
mod sig;
#[cfg(feature = "proxy")]
mod proxy;
use std::ffi::c_void;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::core::BOOL;
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        unsafe { DisableThreadLibraryCalls(module) };
        #[cfg(feature = "proxy")]
        proxy::start();
        #[cfg(not(feature = "proxy"))]
        boot::start();
    }
    1
}
