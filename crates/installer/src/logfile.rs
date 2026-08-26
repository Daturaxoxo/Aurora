#![allow(dead_code)]

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};

const MAX_LEVEL: LevelFilter = if cfg!(debug_assertions) {
    LevelFilter::Trace
} else {
    LevelFilter::Debug
};

const TRACE_CAPPED: [&str; 4] = ["rustls", "ureq", "ureq_proto", "hyper"];

const NOISY_MODULES: [&str; 7] = [
    "winit", "slint", "i_slint", "femtovg", "calloop", "sctk", "mslnk",
];

struct Output {
    file: File,
    path: PathBuf,
}

struct State {
    output: Option<Output>,
    armed: bool,
    failed: bool,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

struct Sink;

static SINK: Sink = Sink;

pub fn init(append: bool) {
    let output = default_path().and_then(|path| open(&path, append));

    let _ = STATE.set(Mutex::new(State {
        output,
        armed: true,
        failed: false,
    }));

    let _ = log::set_logger(&SINK);
    log::set_max_level(MAX_LEVEL);
    set_panic_hook();

    write_header();
}

pub fn finish() {
    let Some(state) = STATE.get() else {
        return;
    };
    let Ok(mut state) = state.lock() else {
        return;
    };

    let keep = state.failed || !state.armed;
    let Some(output) = state.output.take() else {
        return;
    };

    drop(output.file);

    if !keep {
        let _ = fs::remove_file(&output.path);
    }
}

pub fn keep() {
    with_state(|state| state.armed = false);
}

pub fn path() -> Option<PathBuf> {
    let state = STATE.get()?.lock().ok()?;
    state.output.as_ref().map(|output| output.path.clone())
}

pub fn fatal(message: &str) {
    log::error!("{message}");

    let text = match path() {
        Some(path) => format!("{message}\n\nDetails were written to:\n{}", path.display()),
        None => message.to_owned(),
    };
    message_box("Aurora", &text);
}

fn with_state<F: FnOnce(&mut State)>(visit: F) {
    if let Some(state) = STATE.get()
        && let Ok(mut state) = state.lock()
    {
        visit(&mut state);
    }
}

fn default_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_owned();
    Some(exe.with_file_name(stem).with_extension("log"))
}

fn open(path: &Path, append: bool) -> Option<Output> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .ok()
        .map(|file| Output {
            file,
            path: path.to_path_buf(),
        })
}

fn write_header() {
    use sysinfo::System;

    log::info!(
        "Aurora {} ({})",
        env!("CARGO_PKG_VERSION"),
        std::env::current_exe()
            .map(|exe| exe.display().to_string())
            .unwrap_or_else(|_| "unknown location".to_owned())
    );
    log::info!(
        "{} (kernel {}) on {}",
        System::long_os_version().unwrap_or_else(|| "unknown OS".to_owned()),
        System::kernel_version().unwrap_or_else(|| "unknown".to_owned()),
        System::cpu_arch()
    );
    log::info!(
        "Started {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
}

fn set_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map_or_else(|| "unknown location".to_owned(), ToString::to_string);

        log::error!("PANIC at {location}: {}", panic_message(info));

        let text = match path() {
            Some(path) => format!(
                "Aurora hit an unexpected error and has to close.\n\nDetails were written to:\n{}",
                path.display()
            ),
            None => "Aurora hit an unexpected error and has to close.".to_owned(),
        };
        message_box("Aurora", &text);
    }));
}

fn panic_message(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "unknown payload".to_owned()
    }
}

#[cfg(windows)]
fn message_box(title: &str, text: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MessageBoxW,
    };

    fn wide(value: &str) -> Vec<u16> {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let text = wide(text);
    let title = wide(title);

    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND | MB_TOPMOST,
        );
    }
}

#[cfg(not(windows))]
fn message_box(title: &str, text: &str) {
    eprintln!("{title}: {text}");
}

impl Log for Sink {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= MAX_LEVEL
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let module = record.module_path().unwrap_or_default();
        if NOISY_MODULES.iter().any(|noisy| module.starts_with(noisy)) {
            return;
        }

        let level = record.level();
        if level == Level::Trace && TRACE_CAPPED.iter().any(|capped| module.starts_with(capped)) {
            return;
        }
        let line = format!(
            "[{} {level:<5} {module}] {}\n",
            chrono::Local::now().format("%H:%M:%S%.3f"),
            record.args()
        );

        #[cfg(debug_assertions)]
        eprint!("{line}");

        with_state(|state| {
            if level == Level::Error {
                state.failed = true;
            }
            if let Some(output) = state.output.as_mut() {
                let _ = output.file.write_all(line.as_bytes());
                let _ = output.file.flush();
            }
        });
    }

    fn flush(&self) {
        with_state(|state| {
            if let Some(output) = state.output.as_mut() {
                let _ = output.file.flush();
            }
        });
    }
}
