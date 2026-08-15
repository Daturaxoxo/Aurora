#![allow(dead_code)]
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
};
pub const ARP_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Aurora";

pub enum Value {
    Str(String),
    Dword(u32),
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe { RegCloseKey(self.0) };
    }
}

impl Key {
    fn create(subkey: &str) -> Result<Self, String> {
        let subkey_w = wide(subkey);
        let mut handle: HKEY = std::ptr::null_mut();

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
            Ok(Self(handle))
        } else {
            Err(format!("could not create HKCU\\{subkey} (error {status})"))
        }
    }

    fn set(&self, name: &str, value: &Value) -> Result<(), String> {
        let name_w = wide(name);
        let (kind, data) = match value {
            Value::Str(text) => {
                let utf16 = wide(text);
                let bytes: Vec<u8> = utf16.iter().flat_map(|unit| unit.to_le_bytes()).collect();
                (REG_SZ, bytes)
            }
            Value::Dword(number) => (REG_DWORD, number.to_le_bytes().to_vec()),
        };

        let len = u32::try_from(data.len())
            .map_err(|_| format!("value `{name}` is too large for the registry"))?;

        let status =
            unsafe { RegSetValueExW(self.0, name_w.as_ptr(), 0, kind, data.as_ptr(), len) };

        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("could not write `{name}` (error {status})"))
        }
    }
}

pub fn write_entry(values: &[(&str, Value)]) -> Result<(), String> {
    let key = Key::create(ARP_SUBKEY)?;
    for (name, value) in values {
        key.set(name, value)?;
    }
    Ok(())
}

pub fn delete_entry() -> Result<(), String> {
    let subkey_w = wide(ARP_SUBKEY);
    let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, subkey_w.as_ptr()) };

    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!(
            "could not delete HKCU\\{ARP_SUBKEY} (error {status})"
        ))
    }
}
