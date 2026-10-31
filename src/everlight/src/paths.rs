use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};

pub fn host_exe() -> &'static Path {
    static EXE: OnceLock<PathBuf> = OnceLock::new();
    EXE.get_or_init(|| module_path(std::ptr::null_mut()).unwrap_or_else(|| PathBuf::from(".")))
}

pub fn host_exe_name() -> String {
    host_exe()
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn game_dir() -> &'static Path {
    host_exe().parent().unwrap_or(Path::new("."))
}

pub fn module_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut module: HMODULE = std::ptr::null_mut();
        let anchor = module_dir as *const () as *const u16;
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                anchor,
                &mut module,
            )
        } != 0;
        found
            .then(|| module_path(module))
            .flatten()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| game_dir().to_path_buf())
    })
}

fn module_path(module: HMODULE) -> Option<PathBuf> {
    let mut buf = vec![0u16; 260];
    loop {
        let len = unsafe { GetModuleFileNameW(module, buf.as_mut_ptr(), buf.len() as u32) } as usize;
        if len == 0 {
            return None;
        }
        if len < buf.len() {
            return Some(PathBuf::from(OsString::from_wide(&buf[..len])));
        }
        if buf.len() >= 32768 {
            return None;
        }
        buf.resize(buf.len() * 2, 0);
    }
}
