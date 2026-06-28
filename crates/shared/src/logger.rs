use std::{fs, io::Write, path::Path};

use env_filter::{Builder, Filter};

use log::{Log, Metadata, Record, SetLoggerError};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

const FILTER_ENV: &str = "AURORA_LOG";
const LOG_FILE: &str = "Logs/aurora";

pub struct Logger {
    inner: Filter,
    log_file_path: String,
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
        }
    }

    pub fn init() -> Result<(), SetLoggerError> {
        let logger = Self::new();

        #[cfg(debug_assertions)]
        log::set_max_level(log::LevelFilter::Trace);

        #[cfg(not(debug_assertions))]
        log::set_max_level(log::LevelFilter::Info);
        log::set_boxed_logger(Box::new(logger))
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        // if !self.inner.matches(record) {
        //     return;
        // }

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
    }

    fn flush(&self) {}
}
