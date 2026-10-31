use std::ffi::c_void;
use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READWRITE, VirtualProtect, VirtualQuery,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;
const TAG: &str = "Memory";
const NULL_THRESHOLD: usize = 0x10000; // address must be bigger than this, otherwise it will count as null
const KERNEL_THRESHOLD: usize = 0xFFFF_8000_0000_0000; // address must not be higher than this

fn query(addr: usize) -> Option<MEMORY_BASIC_INFORMATION> {
    let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
    let got = unsafe { VirtualQuery(addr as *const c_void, &mut mbi, size_of::<MEMORY_BASIC_INFORMATION>()) };
    (got != 0).then_some(mbi)
}

fn region_end(mbi: &MEMORY_BASIC_INFORMATION) -> usize {
    mbi.BaseAddress as usize + mbi.RegionSize
}

fn is_readable_region(mbi: &MEMORY_BASIC_INFORMATION) -> bool {
    mbi.State == MEM_COMMIT && mbi.Protect != 0 && mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) == 0
}

fn is_executable(protect: u32) -> bool {
    protect & (PAGE_EXECUTE | PAGE_EXECUTE_READ | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY) != 0
}

pub fn validate_address(addr: usize, context: &str) -> bool {
    if addr < NULL_THRESHOLD {
        log_warn!(TAG, "{context}: address is NULL or close to NULL (0x{addr:X}).");
        return false;
    }
    if addr >= KERNEL_THRESHOLD {
        log_warn!(TAG, "{context}: address looks negative or kernel-space (0x{addr:X}).");
        return false;
    }
    true
}

pub fn is_readable(addr: usize, len: usize) -> bool {
    if addr == 0 || len == 0 {
        return false;
    }
    query(addr).is_some_and(|mbi| is_readable_region(&mbi) && addr.saturating_add(len) <= region_end(&mbi))
}

pub fn readable_ranges(base: usize, size: usize) -> Vec<(usize, usize)> {
    let end = base.saturating_add(size);
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut addr = base;
    while addr < end {
        let Some(mbi) = query(addr) else { break };
        let next = region_end(&mbi).min(end);
        if next <= addr {
            break;
        }
        if is_readable_region(&mbi) {
            match ranges.last_mut() {
                Some((start, len)) if *start + *len == addr => *len += next - addr,
                _ => ranges.push((addr, next - addr)),
            }
        }
        addr = next;
    }
    ranges
}

pub fn follow_jump(addr: usize) -> Option<usize> {
    if !validate_address(addr, "follow_jump") {
        return None;
    }
    if !is_readable(addr, 5) {
        log_warn!(TAG, "follow_jump: 5 bytes at 0x{addr:X} are not readable, aborting.");
        return None;
    }
    let opcode = unsafe { *(addr as *const u8) };
    if opcode != 0xE9 && opcode != 0xE8 {
        log_warn!(TAG, "No jmp rel32 at 0x{addr:X} (found 0x{opcode:02X}).");
        return None;
    }
    let rel = unsafe { ((addr + 1) as *const i32).read_unaligned() };
    let target = addr.wrapping_add(5).wrapping_add_signed(rel as isize);
    validate_address(target, "follow_jump target").then_some(target)
}

pub fn mem_equals(addr: usize, expect: &[u8]) -> bool {
    is_readable(addr, expect.len())
        && unsafe { std::slice::from_raw_parts(addr as *const u8, expect.len()) } == expect
}

pub fn write_bytes(addr: usize, patch: &[u8]) -> bool {
    if patch.is_empty() || !validate_address(addr, "write_bytes") {
        return false;
    }
    let Some(mbi) = query(addr) else {
        log_warn!(TAG, "VirtualQuery check failed for 0x{addr:X}: page state unknown");
        return false;
    };
    if mbi.State != MEM_COMMIT {
        log_warn!(TAG, "Target page at 0x{addr:X} is not committed (State=0x{:X})", mbi.State);
        return false;
    }
    if !is_executable(mbi.Protect) {
        log_warn!(
            TAG,
            "Target page at 0x{addr:X} is not executable (Protect=0x{:X}); refusing to patch non-code.",
            mbi.Protect
        );
        return false;
    }
    if addr.saturating_add(patch.len()) > region_end(&mbi) {
        log_warn!(TAG, "Patch at 0x{addr:X} ({} bytes) crosses its memory region", patch.len());
        return false;
    }

    let mut old = 0u32;
    if unsafe { VirtualProtect(addr as *const c_void, patch.len(), PAGE_READWRITE, &mut old) } == 0 {
        log_error!(TAG, "Failed to change protection at 0x{addr:X}.");
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(patch.as_ptr(), addr as *mut u8, patch.len());
        VirtualProtect(addr as *const c_void, patch.len(), old, &mut old);
        FlushInstructionCache(GetCurrentProcess(), addr as *const c_void, patch.len());
    }

    if !mem_equals(addr, patch) {
        log_error!(TAG, "Integrity check failed at 0x{addr:X}: bytes do not match after write.");
        return false;
    }
    true
}
