//! Logging utilities

use std::{
    fmt::{self, Display},
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use chrono::Local;

/// The logging level of a message
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Debug message that is only shown in debug builds.
    Debug = 0,
    /// Warning/important info message that does not signify a failure.
    WarnInfo = 1,
    /// Partially-recoverable failure, such as a leak.
    Error = 2,
}

/// The logging target
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sink {
    /// Logs only to stdout
    Stdout,
    /// Logs only to the provided file
    File(PathBuf),
    /// Logs to both stdout and the provided file
    Both(PathBuf),
}

impl Sink {
    fn file_path(&self) -> Option<&Path> {
        match self {
            Sink::Stdout => None,
            Sink::File(path) | Sink::Both(path) => Some(path),
        }
    }
}

struct Config {
    sink: Sink,
    level: Level,
    file: Mutex<Option<File>>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Initializes logging.
///
/// `sink` will be the future logging target.
///
/// `level` will be the minimum level to display.
pub fn init_logging(sink: Sink, level: Level) {
    let _ = CONFIG.set(Config {
        sink,
        level,
        file: Mutex::new(None),
    });
}

/// Internal log function.
pub fn log_impl(level: Level, file: &str, line: u32, column: u32, args: fmt::Arguments<'_>) {
    if !cfg!(debug_assertions) && level == Level::Debug {
        return;
    }

    let message = args.to_string();

    let Some(config) = CONFIG.get() else {
        let mut out = io::stdout().lock();
        let _ = writeln!(out, "{}{}{}", color(level), message, RESET);
        let _ = out.flush();
        return;
    };

    if (level as u8) < (config.level as u8) {
        return;
    }

    let line = file_line(timestamp(), level, file, line, column, &message);
    match &config.sink {
        Sink::Stdout => stdout_line(level, &message),
        Sink::File(_) => write_file(config, &line),
        Sink::Both(_) => {
            stdout_line(level, &message);
            write_file(config, &line);
        }
    }
}

fn stdout_line(level: Level, message: &str) {
    let mut out = io::stdout().lock();
    let _ = writeln!(
        out,
        "{}[{tag}] {message}{RESET}",
        color(level),
        tag = tag(level)
    );
    let _ = out.flush();
}

fn write_file(config: &Config, line: &str) {
    let Some(path) = config.sink.file_path() else {
        return;
    };
    let mut guard = config.file.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = OpenOptions::new().create(true).append(true).open(path).ok();
    }
    if let Some(file) = guard.as_mut() {
        let _ = writeln!(file, "{line}");
        let _ = file.flush();
    }
}

fn file_line(
    ts: impl Display,
    level: Level,
    file: &str,
    line: u32,
    column: u32,
    message: &str,
) -> String {
    format!(
        "{ts} [{tag}] {file}:{line}:{column} {message}",
        tag = tag(level)
    )
}

fn tag(level: Level) -> &'static str {
    match level {
        Level::Debug => "DEBUG",
        Level::WarnInfo => "WARN",
        Level::Error => "ERROR",
    }
}

fn color(level: Level) -> &'static str {
    match level {
        Level::Debug => "\x1b[90m",
        Level::WarnInfo => "\x1b[93m",
        Level::Error => "\x1b[91m",
    }
}

const RESET: &str = "\x1b[0m";

fn timestamp() -> impl Display {
    let date = Local::now();
    date.format("(%d|$H:%M:%S.%6f)")
}

/// Logs a message.
///
/// The syntax is identical to that of [`println!`], except with a prefix of
/// either `err:`, `wrn:`, or `dbg:`, describing the [`Level`] to use.
#[macro_export]
macro_rules! log {
    (err: $($rest:tt)*) => {
        $crate::logging::log_impl(
            $crate::logging::Level::Error,
            file!(),
            line!(),
            column!(),
            format_args!($($rest)*)
        )
    };

    (wrn: $($rest:tt)*) => {
        $crate::logging::log_impl(
            $crate::logging::Level::WarnInfo,
            file!(),
            line!(),
            column!(),
            format_args!($($rest)*)
        )
    };

    (dbg: $($rest:tt)*) => {
        if cfg!(debug_assertions) {
            $crate::logging::log_impl(
                $crate::logging::Level::Debug,
                file!(),
                line!(),
                column!(),
                format_args!($($rest)*)
            )
        }
    };
}
