use super::memory;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern(Vec<Option<u8>>);

impl Pattern {
    pub fn parse(src: &str) -> Option<Self> {
        let bytes = src
            .split_whitespace()
            .map(|t| match t {
                "?" | "??" => Some(None),
                _ => u8::from_str_radix(t, 16).ok().map(Some),
            })
            .collect::<Option<Vec<_>>>()?;
        bytes.iter().any(Option::is_some).then_some(Self(bytes))
    }

    fn matches(&self, window: &[u8]) -> bool {
        self.0
            .iter()
            .zip(window)
            .all(|(p, b)| p.is_none_or(|p| p == *b))
    }

    pub fn find_in(&self, haystack: &[u8]) -> Option<usize> {
        let len = self.0.len();
        let last = haystack.len().checked_sub(len)?;
        let (anchor, first) = self
            .0
            .iter()
            .enumerate()
            .find_map(|(i, b)| b.map(|b| (i, b)))?;
        let mut start = 0;
        while start <= last {
            start += haystack[start + anchor..=last + anchor]
                .iter()
                .position(|&b| b == first)?;
            if self.matches(&haystack[start..start + len]) {
                return Some(start);
            }
            start += 1;
        }
        None
    }
}

pub fn find_in_main_module(pattern: &Pattern) -> Option<usize> {
    let (base, size) = main_module()?;
    for (start, len) in memory::readable_ranges(base, size) {
        let haystack = unsafe { std::slice::from_raw_parts(start as *const u8, len) };
        if let Some(offset) = pattern.find_in(haystack) {
            return Some(start + offset);
        }
    }
    None
}

fn main_module() -> Option<(usize, usize)> {
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        if module.is_null() {
            return None;
        }
        let mut info: MODULEINFO = std::mem::zeroed();
        if GetModuleInformation(
            GetCurrentProcess(),
            module,
            &mut info,
            size_of::<MODULEINFO>() as u32,
        ) == 0
        {
            return None;
        }
        Some((info.lpBaseOfDll as usize, info.SizeOfImage as usize))
    }
}
