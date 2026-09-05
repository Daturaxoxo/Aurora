use anyhow::{Result, anyhow};
pub use ipc::oneclick::{OneClick, SCHEME, parse};
use log::*;
use std::path::Path;

#[cfg(target_os = "linux")]
fn protocol_entry(exe: &Path) -> Result<String> {
    let path = exe
        .to_str()
        .ok_or_else(|| anyhow!("the protocol handler path is not UTF-8"))?;
    if !exe.is_absolute() || path.chars().any(char::is_control) || path.contains('=') {
        return Err(anyhow!(
            "the protocol handler needs an absolute desktop-compatible path"
        ));
    }

    let mut escaped = String::new();
    for ch in path.chars() {
        match ch {
            '\\' => escaped.push_str(r"\\\\"),
            '"' | '`' | '$' => {
                escaped.push_str(r"\\");
                escaped.push(ch);
            }
            '%' => escaped.push_str("%%"),
            _ => escaped.push(ch),
        }
    }

    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Aurora 1-Click\n\
         Exec=\"{escaped}\" %u\n\
         Terminal=false\n\
         NoDisplay=true\n\
         MimeType=x-scheme-handler/{SCHEME};\n"
    ))
}

#[cfg(target_os = "linux")]
pub fn register_protocol(exe: &Path) -> Result<()> {
    use std::process::Command;

    const DESKTOP_FILE: &str = "aurora-oneclick.desktop";
    let contents = protocol_entry(exe)?;
    let applications = dirs::data_dir()
        .ok_or_else(|| anyhow!("could not resolve the data directory"))?
        .join("applications");
    crate::utils::write_if_changed(&applications.join(DESKTOP_FILE), contents.as_bytes())
        .map_err(|e| anyhow!("could not write the one-click desktop entry: {e}"))?;

    match Command::new("update-desktop-database")
        .arg(&applications)
        .status()
    {
        Ok(status) if !status.success() => {
            warn!("1-Click: update-desktop-database exited with {status}");
        }
        Err(e) => warn!("1-Click: update-desktop-database is unavailable: {e}"),
        Ok(_) => {}
    }

    let output = Command::new("xdg-mime")
        .args([
            "default",
            DESKTOP_FILE,
            &format!("x-scheme-handler/{SCHEME}"),
        ])
        .output()
        .map_err(|e| anyhow!("could not run xdg-mime: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "xdg-mime exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    info!(
        "1-Click: registered protocol `{SCHEME}` for {}",
        exe.display()
    );

    Ok(())
}

#[cfg(target_os = "windows")]
pub fn handler_path(dir: &Path) -> std::path::PathBuf {
    let shim = dir.join(ipc::ONECLICK_EXE);
    if shim.is_file() {
        shim
    } else {
        warn!(
            "1-Click: {} is missing; registering Aurora itself",
            shim.display()
        );
        dir.join(ipc::AURORA_EXE)
    }
}

#[cfg(target_os = "windows")]
fn protocol_subkey() -> String {
    format!(r"Software\Classes\{SCHEME}")
}

#[cfg(target_os = "windows")]
pub fn register_protocol(exe: &Path) -> Result<()> {
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

    fn create(subkey: &str) -> Result<Key> {
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
            Err(anyhow!("could not create HKCU\\{subkey} (error {status})"))
        }
    }

    fn set(key: &Key, name: &str, value: &str) -> Result<()> {
        let name_w = wide(OsStr::new(name));
        let value_w = wide(OsStr::new(value));
        let byte_len = u32::try_from(value_w.len().saturating_mul(size_of::<u16>()))
            .map_err(|_| anyhow!("registry value `{name}` is too large"))?;
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
            Err(anyhow!("could not write `{name}` (error {status})"))
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
pub fn unregister_protocol() -> Result<()> {
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
        Err(anyhow!(
            "could not delete HKCU\\{protocol_subkey} (error {status})"
        ))
    }
}
