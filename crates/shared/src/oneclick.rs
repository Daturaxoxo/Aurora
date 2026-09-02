#[cfg(target_os = "windows")]
use log::*;
#[cfg(target_os = "windows")]
use std::path::Path;
pub use ipc::oneclick::{OneClick, SCHEME, parse};
#[cfg(target_os = "windows")]
pub fn handler_path(dir: &Path) -> std::path::PathBuf {
    let shim = dir.join(ipc::ONECLICK_EXE);
    if shim.is_file() {
        shim
    } else {
        warn!("1-Click: {} is missing; registering Aurora itself", shim.display());
        dir.join(ipc::AURORA_EXE)
    }
}

#[cfg(target_os = "windows")]
fn protocol_subkey() -> String {
    format!(r"Software\Classes\{SCHEME}")
}

#[cfg(target_os = "windows")]
pub fn register_protocol(exe: &Path) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn create(subkey: &str) -> Result<Key, String> {
        let subkey_w = wide(OsStr::new(subkey));
        let mut handle = std::ptr::null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                subkey_w.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &raw mut handle,
                std::ptr::null_mut(),
            )
        };
        if status == ERROR_SUCCESS {
            Ok(Key(handle))
        } else {
            Err(format!("could not create HKCU\\{subkey} (error {status})"))
        }
    }

    fn set(key: &Key, name: &str, value: &str) -> Result<(), String> {
        let name_w = wide(OsStr::new(name));
        let value_w = wide(OsStr::new(value));
        let byte_len = u32::try_from(value_w.len().saturating_mul(size_of::<u16>()))
            .map_err(|_| format!("registry value `{name}` is too large"))?;
        let status = unsafe {
            RegSetValueExW(
                key.0,
                name_w.as_ptr(),
                0,
                REG_SZ,
                value_w.as_ptr().cast(),
                byte_len,
            )
        };
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("could not write `{name}` (error {status})"))
        }
    }

    let exe = exe.display().to_string();
    let protocol_subkey = protocol_subkey();
    let root = create(&protocol_subkey)?;
    set(&root, "", "URL:Aurora Launcher Protocol")?;
    set(&root, "URL Protocol", "")?;

    let icon = create(&format!(r"{protocol_subkey}\DefaultIcon"))?;
    set(&icon, "", &format!("{exe},0"))?;

    let command = create(&format!(r"{protocol_subkey}\shell\open\command"))?;
    set(&command, "", &format!(r#""{exe}" "%1""#))?;

    info!("1-Click: registered protocol `{SCHEME}`");
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn unregister_protocol() -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};

    let protocol_subkey = protocol_subkey();
    let subkey: Vec<u16> = OsStr::new(&protocol_subkey)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, subkey.as_ptr()) };
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        info!("1-Click: unregistered protocol `{SCHEME}`");
        Ok(())
    } else {
        Err(format!(
            "could not delete HKCU\\{protocol_subkey} (error {status})"
        ))
    }
}
