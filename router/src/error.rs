// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

#[derive(Debug)]
pub enum RouterError {
    Code(&'static str),
    Context { code: &'static str, detail: String },
    Io(io::Error),
    Config(config::ConfigError),
    Database(rusqlite::Error),
    Json(serde_json::Error),
}

pub type RouterResult<T> = Result<T, RouterError>;

impl RouterError {
    pub const fn code(code: &'static str) -> Self {
        Self::Code(code)
    }

    pub fn context(code: &'static str, detail: impl Display) -> Self {
        Self::Context {
            code,
            detail: detail.to_string(),
        }
    }
}

impl Display for RouterError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code(code) => formatter.write_str(code),
            Self::Context { code, detail } => write!(formatter, "{code}:{detail}"),
            Self::Io(error) => Display::fmt(error, formatter),
            Self::Config(error) => Display::fmt(error, formatter),
            Self::Database(error) => Display::fmt(error, formatter),
            Self::Json(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for RouterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Code(_) | Self::Context { .. } => None,
        }
    }
}

impl From<io::Error> for RouterError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<config::ConfigError> for RouterError {
    fn from(error: config::ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<rusqlite::Error> for RouterError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<serde_json::Error> for RouterError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
