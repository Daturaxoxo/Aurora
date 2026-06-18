use std::{fs, io::Write, path};

use env_filter::{Builder, Filter};

use log::{Log, Metadata, Record, SetLoggerError};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

const FILTER_ENV: &str = "AURORA_LOG";
const LOG_FILE: &str = "Logs/aurora";

pub struct Logger {
    inner: Filter,
}

impl Logger {
    fn new() -> Self {
        let mut builder = Builder::from_env(FILTER_ENV);

        Self {
            inner: builder.build(),
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
        macro_rules! set_stdout_color {
            ($r: expr, $g: expr, $b: expr, $stdout: ident) => {
                $stdout
                    .set_color(ColorSpec::new().set_fg(Some(Color::Rgb($r, $g, $b))))
                    .unwrap()
            };
        }

        let timestamp = chrono::Utc::now().format("%d-%m-%Y %H:%M:%S").to_string();
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
        };
        write!(&mut stdout, " {} ", record.level()).unwrap();

        stdout.reset().expect("Failed to reset stdout");
        write!(&mut stdout, "{}", record.module_path().unwrap_or_default()).unwrap();

        set_stdout_color!(131, 141, 140, stdout);
        write!(&mut stdout, "] ").unwrap();

        stdout.reset().expect("Failed to reset stdout");
        write!(&mut stdout, "{}", record.args()).unwrap();

        let name = format!("{}-{}.log", LOG_FILE, timestamp.replace(":", "-").replace(" ", "-"));
        let path = path::Path::new(&name);
        let _ = fs::create_dir_all(path.parent().unwrap());
        if fs::metadata(&name).is_err() && fs::File::create(&name).is_err() {
                println!("Failed to create log file");
                return;
        }

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&name)
            .expect("Failed to open log file");
        writeln!(file, "{str}").unwrap();
        println!();
    }

    fn flush(&self) {}
}