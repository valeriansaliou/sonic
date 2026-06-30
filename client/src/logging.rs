// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#![allow(unused_macros)]
#![allow(unused_imports)]

macro_rules! log_trace {
    ($($t:tt)*) => {
        crate::logging::log!(trace, $($t)*)
    }
}
pub(crate) use log_trace;

macro_rules! log_debug {
    ($($t:tt)*) => {
        crate::logging::log!(debug, $($t)*)
    }
}
pub(crate) use log_debug;

macro_rules! log_info {
    ($($t:tt)*) => {
        crate::logging::log!(info, $($t)*)
    }
}
pub(crate) use log_info;

macro_rules! log_warn {
    ($($t:tt)*) => {
        crate::logging::log!(warn, $($t)*)
    }
}
pub(crate) use log_warn;

macro_rules! log_error {
    ($($t:tt)*) => {
        crate::logging::log!(error, $($t)*)
    }
}
pub(crate) use log_error;

macro_rules! log {
    ($level:ident, $($t:tt)*) => {{
        #[cfg(feature = "logging-std")]
        eprintln!("[{}] {}", stringify!($level), format!($($t)*));

        #[cfg(feature = "log")]
        log::$level!($($t)*);

        #[cfg(feature = "tracing")]
        tracing::$level!($($t)*);

        #[cfg(not(any(feature = "logging-std", feature = "log", feature = "tracing")))]
        if false { let _ = ( format!($($t)*) ); }
    }}
}
pub(crate) use log;
