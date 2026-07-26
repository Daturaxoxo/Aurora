use std::{fs, io::Write, path::Path, sync::mpsc::SyncSender};

use env_filter::{Builder, Filter};

use log::*;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use crate::telemetry::{spawn_error_worker, ErrorEvent};

const FILTER_ENV: &str = "AURORA_LOG";
const LOG_FILE: &str = "Logs/aurora";

pub struct Logger {
    inner: Filter,
    log_file_path: String,
    error_tx: Option<SyncSender<ErrorEvent>>,
}

impl Logger {
    fn new() -> Self {
        let mut builder = Builder::from_env(FILTER_ENV);
        builder.filter_module("mslnk", log::LevelFilter::Off);

        let startup_timestamp = chrono::Utc::now().format("%d-%m-%Y-%H-%M-%S").to_string();
        let log_file_path = format!("{LOG_FILE}-{startup_timestamp}.log");

        let path = Path::new(&log_file_path);
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }

        Self {
            inner: builder.build(),
            log_file_path,
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
        if record
            .module_path()
            .is_some_and(|s| s.contains("reqwest") || s.contains("rustls"))
        {
            return;
        }
        macro_rules! set_stdout_color {
            ($r: expr, $g: expr, $b: expr, $stdout: ident) => {
                $stdout
                    .set_color(ColorSpec::new().set_fg(Some(Color::Rgb($r, $g, $b))))
                    .unwrap()
            };
        }

        let timestamp = chrono::Utc::now().format("%d-%m-%Y-%H-%M-%S").to_string();
        let mut stdout = StandardStream::stdout(ColorChoice::Always);
        set_stdout_color!(131, 141, 140, stdout);
        write!(&mut stdout, "[").unwrap();

        stdout.reset().expect("Failed to reset stdout");

        write!(&mut stdout, "{timestamp}").unwrap();

        let str = format!(
            "[{timestamp} {} {}] {}",
            record.level(),
            record.module_path().unwrap_or_default(),
            record.args()
        );

        match record.level() {
            log::Level::Error => set_stdout_color!(255, 0, 0, stdout),
            log::Level::Warn => set_stdout_color!(255, 255, 0, stdout),
            log::Level::Info => set_stdout_color!(79, 184, 150, stdout),
            log::Level::Debug => set_stdout_color!(0, 255, 255, stdout),
            log::Level::Trace => set_stdout_color!(0, 0, 255, stdout),
        }
        write!(&mut stdout, " {} ", record.level()).unwrap();

        stdout.reset().expect("Failed to reset stdout");
        write!(&mut stdout, "{}", record.module_path().unwrap_or_default()).unwrap();

        set_stdout_color!(131, 141, 140, stdout);
        write!(&mut stdout, "] ").unwrap();

        stdout.reset().expect("Failed to reset stdout");
        write!(&mut stdout, "{}", record.args()).unwrap();
        println!();

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file_path)
            .expect("Failed to open log file");

        writeln!(file, "{str}").unwrap();

        if record.level() == log::Level::Error {
            if let Some(tx) = &self.error_tx {
                let _ = tx.try_send(ErrorEvent {
                    timestamp,
                    module: record.module_path().unwrap_or_default().to_string(),
                    message: record.args().to_string(),
                });
            }
        }
    }

    fn flush(&self) {}
}

pub fn get_latest_logs() -> Option<String> {
    let log_dir_path = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
        .join("Logs");

    let last_modified_file = std::fs::read_dir(log_dir_path)
        .expect("Couldn't access local directory")
        .flatten()
        .filter(|f| f.metadata().unwrap().is_file())
        .max_by_key(|x| x.metadata().unwrap().modified().unwrap());

    if let Some(file) = last_modified_file {
        Some(std::fs::read_to_string(file.path()).unwrap_or_default())
    } else {
        None
    }
}
