#[macro_export]
macro_rules! __saios_log {
    ($level:expr, $($arg:tt)*) => {{
        $crate::log::logger::log($level, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! __saios_trace {
    ($msg:literal) => {{
        $crate::log::logger::log_str($crate::log::level::LogLevel::Trace, $msg);
    }};
    ($($arg:tt)*) => {{
        $crate::log::logger::log($crate::log::level::LogLevel::Trace, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! __saios_debug {
    ($msg:literal) => {{
        $crate::log::logger::log_str($crate::log::level::LogLevel::Debug, $msg);
    }};
    ($($arg:tt)*) => {{
        $crate::log::logger::log($crate::log::level::LogLevel::Debug, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! __saios_info {
    ($msg:literal) => {{
        $crate::log::logger::log_str($crate::log::level::LogLevel::Info, $msg);
    }};
    ($($arg:tt)*) => {{
        $crate::log::logger::log($crate::log::level::LogLevel::Info, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! __saios_warn {
    ($msg:literal) => {{
        $crate::log::logger::log_str($crate::log::level::LogLevel::Warn, $msg);
    }};
    ($($arg:tt)*) => {{
        $crate::log::logger::log($crate::log::level::LogLevel::Warn, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! __saios_error {
    ($msg:literal) => {{
        $crate::log::logger::log_str($crate::log::level::LogLevel::Error, $msg);
    }};
    ($($arg:tt)*) => {{
        $crate::log::logger::log($crate::log::level::LogLevel::Error, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! __saios_fatal {
    ($msg:literal) => {{
        $crate::log::logger::log_str($crate::log::level::LogLevel::Fatal, $msg);
    }};
    ($($arg:tt)*) => {{
        $crate::log::logger::log($crate::log::level::LogLevel::Fatal, format_args!($($arg)*));
    }};
}

pub use crate::__saios_debug as debug;
pub use crate::__saios_error as error;
pub use crate::__saios_fatal as fatal;
pub use crate::__saios_info as info;
pub use crate::__saios_trace as trace;
pub use crate::__saios_warn as warn;
