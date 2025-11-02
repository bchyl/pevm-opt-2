use std::fmt::Display;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
    Debug,
}

impl Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Debug => write!(f, "DEBUG"),
        }
    }
}

pub fn log(level: LogLevel, message: impl Display) {
    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f");
    eprintln!("{} [{}] {}", timestamp, level, message);
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::logger::log($crate::log::logger::LogLevel::Info, format!($($arg)*))
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::logger::log($crate::log::logger::LogLevel::Warn, format!($($arg)*))
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::logger::log($crate::log::logger::LogLevel::Error, format!($($arg)*))
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        $crate::log::logger::log($crate::log::logger::LogLevel::Debug, format!($($arg)*))
    };
}
