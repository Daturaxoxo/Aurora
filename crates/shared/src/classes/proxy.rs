pub fn registry_proxy_url() -> Option<String> {
    #[cfg(windows)]
    {
        if win::read_dword("ProxyEnable")? != 1 {
            return None;
        }
        parse_proxy_server(&win::read_string("ProxyServer")?)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn parse_proxy_server(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if !raw.contains('=') {
        return Some(with_scheme(raw, "http"));
    }

    for (key, scheme) in [("https=", "http"), ("http=", "http"), ("socks=", "socks5")] {
        let found = raw
            .split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix(key))
            .filter(|addr| !addr.is_empty());
        if let Some(addr) = found {
            return Some(with_scheme(addr, scheme));
        }
    }
    None
}

fn with_scheme(addr: &str, scheme: &str) -> String {
    if addr.contains("://") {
        addr.to_string()
    } else {
        format!("{scheme}://{addr}")
    }
}

#[cfg(windows)]
mod win {
    use std::mem::size_of;
    use std::ptr::{null_mut};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
    };

    const SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn read_dword(name: &str) -> Option<u32> {
        let (sub, val) = (wide(SUBKEY), wide(name));
        let mut data: u32 = 0;
        let mut size = size_of::<u32>() as u32;
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                sub.as_ptr(),
                val.as_ptr(),
                RRF_RT_REG_DWORD,
                null_mut(),
                &mut data as *mut u32 as *mut _,
                &mut size,
            )
        };
        (rc == 0).then_some(data)
    }

    pub fn read_string(name: &str) -> Option<String> {
        let (sub, val) = (wide(SUBKEY), wide(name));
        let mut size: u32 = 0;
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                sub.as_ptr(),
                val.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                null_mut(),
                &mut size,
            )
        };
        if rc != 0 || size == 0 {
            return None;
        }

        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                sub.as_ptr(),
                val.as_ptr(),
                RRF_RT_REG_SZ,
                null_mut(),
                buf.as_mut_ptr() as *mut _,
                &mut size,
            )
        };
        if rc != 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}