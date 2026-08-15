use std::path::Path;

#[cfg(not(target_os = "windows"))]
pub(super) const fn holders(_path: &Path) -> Vec<String> {
    Vec::new()
}

#[cfg(target_os = "windows")]
pub(super) fn holders(path: &Path) -> Vec<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::RestartManager::{
        RmEndSession, RmRegisterResources, RmStartSession,
    };

    // CCH_RM_SESSION_KEY + 1
    let mut key = [0u16; 33];
    let mut session = 0u32;

    if unsafe { RmStartSession(&raw mut session, 0, key.as_mut_ptr()) } != ERROR_SUCCESS {
        return Vec::new();
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let files = [wide.as_ptr()];

    let registered = unsafe {
        RmRegisterResources(
            session,
            1,
            files.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        )
    };

    let names = if registered == ERROR_SUCCESS {
        list_processes(session)
    } else {
        Vec::new()
    };

    unsafe { RmEndSession(session) };

    names
}

#[cfg(target_os = "windows")]
fn list_processes(session: u32) -> Vec<String> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::RestartManager::{RmGetList, RM_PROCESS_INFO};

    const MAX_TRIES: usize = 3;

    let mut capacity = 8u32;

    for _ in 0..MAX_TRIES {
        let len = usize::try_from(capacity).unwrap_or_default();
        let mut info = vec![unsafe { std::mem::zeroed::<RM_PROCESS_INFO>() }; len];
        let mut needed = 0u32;
        let mut count = capacity;
        let mut reason = 0u32;

        let result = unsafe {
            RmGetList(
                session,
                &raw mut needed,
                &raw mut count,
                info.as_mut_ptr(),
                &raw mut reason,
            )
        };

        if result == ERROR_MORE_DATA {
            capacity = needed.max(capacity.saturating_add(1));
            continue;
        }

        if result != ERROR_SUCCESS {
            return Vec::new();
        }

        info.truncate(usize::try_from(count).unwrap_or_default());
        return info.iter().map(describe).collect();
    }

    Vec::new()
}

#[cfg(target_os = "windows")]
fn describe(info: &windows_sys::Win32::System::RestartManager::RM_PROCESS_INFO) -> String {
    let name = &info.strAppName;
    let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
    let name = String::from_utf16_lossy(&name[..end]);
    let pid = info.Process.dwProcessId;

    if name.is_empty() {
        format!("pid {pid}")
    } else {
        format!("{name} (pid {pid})")
    }
}
