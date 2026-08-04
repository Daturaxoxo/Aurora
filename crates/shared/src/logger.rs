use std::{
    collections::VecDeque,
    fs,
    hint::cold_path,
    io::Write,
    path::Path,
    sync::{mpsc::SyncSender, Mutex},
};

use env_filter::{Builder, Filter};

use log::*;
use termcolor::{Buffer, BufferWriter, Color, ColorChoice, ColorSpec, WriteColor};

use crate::telemetry::{spawn_error_worker, ErrorEvent};

const FILTER_ENV: &str = "AURORA_LOG";

#[cfg(windows)]
pub(crate) fn log_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
        .join("Logs")
}

#[cfg(target_os = "linux")]
pub(crate) fn log_dir() -> std::path::PathBuf {
    ipc::state_root().join("Logs")
}

/// Maximum amount of log entries allowed in the in-memory buffer
pub const LOG_BUFFER_CAPACITY: usize = 10_000;

/// Maximum amount of `aurora-*.log` files kept on disk
const MAX_LOG_FILES: usize = 20;

fn session_log_modified(entry: &fs::DirEntry) -> Option<std::time::SystemTime> {
    let name = entry.file_name();
    let name = name.to_str()?;
    if !name.starts_with("aurora-") || !name.ends_with(".log") {
        cold_path();
        return None;
    }

    let metadata = entry.metadata().ok()?;
    if !metadata.is_file() {
        cold_path();
        return None;
    }

    metadata.modified().ok()
}

fn prune_old_logs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    let mut logs: Vec<_> = entries
        .flatten()
        .filter_map(|entry| Some((session_log_modified(&entry)?, entry.path())))
        .collect();

    if logs.len() <= MAX_LOG_FILES {
        return;
    }

    logs.sort_unstable_by(|(a, _), (b, _)| b.cmp(a));

    for (_, path) in logs.drain(MAX_LOG_FILES..) {
        if let Err(e) = fs::remove_file(&path) {
            eprintln!("Failed to remove old log file {}: {e}", path.display());
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: String,
    pub level: Level,
    pub module: String,
    pub message: String,
}

struct LogBuffer {
    entries: VecDeque<LogEntry>,
    next_seq: u64,
}

impl LogBuffer {
    const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
            next_seq: 0,
        }
    }
}

static LOG_BUFFER: Mutex<LogBuffer> = Mutex::new(LogBuffer::new());

fn buffer_record(timestamp: &str, record: &Record) {
    let Ok(mut buffer) = LOG_BUFFER.lock() else {
        return;
    };

    if buffer.entries.len() >= LOG_BUFFER_CAPACITY {
        buffer.entries.pop_front();
    }

    let seq = buffer.next_seq;
    buffer.next_seq += 1;

    buffer.entries.push_back(LogEntry {
        id: seq,
        timestamp: timestamp.to_string(),
        level: record.level(),
        module: record.module_path().unwrap_or_default().to_string(),
        message: record.args().to_string(),
    });
}

pub fn for_each_log_since<F: FnMut(&LogEntry)>(cursor: u64, mut visit: F) -> u64 {
    let Ok(buffer) = LOG_BUFFER.lock() else {
        return cursor;
    };

    let Some(front) = buffer.entries.front() else {
        return buffer.next_seq;
    };

    let skip = usize::try_from(cursor.saturating_sub(front.id)).unwrap_or(usize::MAX);
    for entry in buffer.entries.iter().skip(skip) {
        visit(entry);
    }

    buffer.next_seq
}

struct Sink {
    console: Buffer,
    file: Option<fs::File>,
}

pub struct Logger {
    inner: Filter,
    writer: BufferWriter,
    sink: Mutex<Sink>,
    error_tx: Option<SyncSender<ErrorEvent>>,
}

impl Logger {
    fn new() -> Self {
        let mut builder = Builder::from_env(FILTER_ENV);
        builder.filter_module("mslnk", log::LevelFilter::Off);

        let startup_timestamp = chrono::Utc::now().format("%d-%m-%Y-%H-%M-%S").to_string();
        let log_file_path = log_dir()
            .join(format!("aurora-{startup_timestamp}.log"))
            .display()
            .to_string();

        let path = Path::new(&log_file_path);
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
            prune_old_logs(p);
        }

        let file = match fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file_path)
        {
            Ok(file) => Some(file),
            Err(e) => {
                eprintln!("Failed to open log file {log_file_path}: {e}");
                None
            }
        };

        let writer = BufferWriter::stdout(ColorChoice::Always);
        let console = writer.buffer();

        Self {
            inner: builder.build(),
            writer,
            sink: Mutex::new(Sink { console, file }),
            error_tx: Some(spawn_error_worker()),
        }
    }

    pub fn init() -> Result<(), SetLoggerError> {
        let logger = Self::new();

        #[cfg(debug_assertions)]
        log::set_max_level(log::LevelFilter::Trace);

        #[cfg(not(debug_assertions))]
        log::set_max_level(log::LevelFilter::Info);

        // if beta is enabled, enable trace anyways
        #[cfg(feature = "beta")]
        log::set_max_level(log::LevelFilter::Trace);

        log::set_boxed_logger(Box::new(logger))
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if record.module_path().is_some_and(|s| {
            s.contains("reqwest")
                || s.contains("rustls")
                || s.contains("calloop")
                || s.contains("smithay")
        }) {
            return;
        }
        let timestamp = chrono::Utc::now().format("%d-%m-%Y-%H-%M-%S").to_string();
        buffer_record(&timestamp, record);

        let level = record.level();
        let module = record.module_path().unwrap_or_default();

        if let Ok(mut sink) = self.sink.lock() {
            let sink = &mut *sink;

            macro_rules! set_color {
                ($r: expr, $g: expr, $b: expr) => {{
                    let _ = sink
                        .console
                        .set_color(ColorSpec::new().set_fg(Some(Color::Rgb($r, $g, $b))));
                }};
            }

            sink.console.clear();

            set_color!(131, 141, 140);
            let _ = write!(sink.console, "[");
            let _ = sink.console.reset();

            let _ = write!(sink.console, "{timestamp}");

            match level {
                Level::Error => set_color!(255, 0, 0),
                Level::Warn => set_color!(255, 255, 0),
                Level::Info => set_color!(79, 184, 150),
                Level::Debug => set_color!(0, 255, 255),
                Level::Trace => set_color!(0, 0, 255),
            }
            let _ = write!(sink.console, " {level} ");
            let _ = sink.console.reset();

            let _ = write!(sink.console, "{module}");

            set_color!(131, 141, 140);
            let _ = write!(sink.console, "] ");
            let _ = sink.console.reset();

            let _ = writeln!(sink.console, "{}", record.args());
            let _ = self.writer.print(&sink.console);

            if let Some(file) = sink.file.as_mut() {
                let _ = writeln!(file, "[{timestamp} {level} {module}] {}", record.args());
            }
        }

        if level == Level::Error {
            if let Some(tx) = &self.error_tx {
                let _ = tx.try_send(ErrorEvent {
                    timestamp,
                    module: module.to_string(),
                    message: record.args().to_string(),
                });
            }
        }
    }

    fn flush(&self) {}
}

pub fn logs_directory() -> std::path::PathBuf {
    log_dir()
}

pub fn get_latest_logs() -> Option<String> {
    let last_modified_file = fs::read_dir(logs_directory())
        .ok()?
        .flatten()
        .filter_map(|entry| Some((session_log_modified(&entry)?, entry)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, entry)| entry);

    last_modified_file.map(|file| fs::read_to_string(file.path()).unwrap_or_default())
}
