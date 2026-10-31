use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
use crate::paths;
pub const FILE_NAME: &str = "Everlight.log";

#[derive(Clone, Copy)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

static DEBUG: AtomicBool = AtomicBool::new(false);
static FILE_LOCK: Mutex<()> = Mutex::new(());

pub fn set_debug(enabled: bool) {
    DEBUG.store(enabled, Ordering::Relaxed);
}

pub fn debug_enabled() -> bool {
    DEBUG.load(Ordering::Relaxed)
}

pub fn write(level: Level, tag: &str, line: u32, args: fmt::Arguments) {
    let (t, nanos) = local_time();
    let text = format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:09} [{}] <{}:{}>: {}\n",
        t.wYear,
        t.wMonth,
        t.wDay,
        t.wHour,
        t.wMinute,
        t.wSecond,
        nanos,
        level.as_str(),
        tag,
        line,
        args
    );

    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let path = paths::game_dir().join(FILE_NAME);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

fn local_time() -> (SYSTEMTIME, u64) {
    let mut ft = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        GetSystemTimePreciseAsFileTime(&mut ft);
        FileTimeToSystemTime(&ft, &mut utc);
        if SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0 {
            local = utc;
        }
    }
    let ticks = (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime);
    (local, (ticks % 10_000_000) * 100)
}

macro_rules! log_debug {
    ($tag:expr, $($arg:tt)+) => {
        if $crate::log::debug_enabled() {
            $crate::log::write($crate::log::Level::Debug, $tag, line!(), format_args!($($arg)+))
        }
    };
}

macro_rules! log_info {
    ($tag:expr, $($arg:tt)+) => {
        $crate::log::write($crate::log::Level::Info, $tag, line!(), format_args!($($arg)+))
    };
}

macro_rules! log_warn {
    ($tag:expr, $($arg:tt)+) => {
        $crate::log::write($crate::log::Level::Warn, $tag, line!(), format_args!($($arg)+))
    };
}

macro_rules! log_error {
    ($tag:expr, $($arg:tt)+) => {
        $crate::log::write($crate::log::Level::Error, $tag, line!(), format_args!($($arg)+))
    };
}
