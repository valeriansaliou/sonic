// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub use SonicError as Error;

pub struct SonicError(LeveledError);

impl_transparent_wrapper_utils!(Debug for Error(LeveledError));
impl_transparent_wrapper_utils!(Display for Error(LeveledError));
impl_transparent_wrapper_utils!(Deref for Error(LeveledError));

impl<E> From<E> for Error
where
    E: Into<anyhow::Error>,
{
    #[inline]
    fn from(inner: E) -> Self {
        Self(LeveledError {
            level: tracing::Level::ERROR,
            error: inner.into(),
        })
    }
}

pub trait Context {
    type Res;

    /// Wrap the error value with additional context.
    fn context<C>(self, context: C) -> Self::Res
    where
        C: std::fmt::Display + Send + Sync + 'static;

    /// Wrap the error value with additional context that is evaluated lazily
    /// only once an error does occur.
    fn with_context<C, F>(self, f: F) -> Self::Res
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C;
}

impl Context for Error {
    type Res = Self;

    #[inline]
    fn context<C>(self, context: C) -> Self
    where
        C: std::fmt::Display + Send + Sync + 'static,
    {
        Self(self.0.context(context))
    }

    #[inline]
    fn with_context<C, F>(self, f: F) -> Self
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        Self(self.0.with_context(f))
    }
}

impl Context for LeveledError<anyhow::Error> {
    type Res = Self;

    #[inline]
    fn context<C>(self, context: C) -> Self
    where
        C: std::fmt::Display + Send + Sync + 'static,
    {
        Self {
            level: self.level,
            error: self.error.context(context),
        }
    }

    #[inline]
    fn with_context<C, F>(self, f: F) -> Self
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        Self {
            level: self.level,
            error: self.error.context(f()),
        }
    }
}

impl<T, E: Into<Error>> Context for Result<T, E> {
    type Res = Result<T, Error>;

    #[inline]
    fn context<C>(self, context: C) -> Self::Res
    where
        C: std::fmt::Display + Send + Sync + 'static,
    {
        self.map_err(|err| err.into().context(context))
    }

    #[inline]
    fn with_context<C, F>(self, f: F) -> Self::Res
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        self.map_err(|err| err.into().context(f()))
    }
}

impl<T> Context for Option<T> {
    type Res = Result<T, Error>;

    #[inline]
    fn context<C>(self, context: C) -> Self::Res
    where
        C: std::fmt::Display + Send + Sync + 'static,
    {
        self.ok_or(LeveledError::from(anyhow::Error::msg(context.to_string())).into())
    }

    #[inline]
    fn with_context<C, F>(self, f: F) -> Self::Res
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        self.ok_or_else(|| LeveledError::from(anyhow::Error::msg(f().to_string())).into())
    }
}

#[derive(Debug)]
pub struct LeveledError<E = anyhow::Error> {
    level: tracing::Level,
    error: E,
}

impl<E> LeveledError<E> {
    #[inline]
    pub(crate) const fn new(level: tracing::Level, error: E) -> Self {
        Self { level, error }
    }

    #[inline]
    pub const fn level(&self) -> tracing::Level {
        self.level
    }
}

impl std::error::Error for LeveledError {
    #[inline]
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.error.source()
    }
}

impl std::fmt::Display for LeveledError {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.error, f)
    }
}

impl<E> From<E> for LeveledError<E> {
    #[inline]
    fn from(inner: E) -> Self {
        LeveledError {
            level: tracing::Level::ERROR,
            error: inner,
        }
    }
}

#[allow(clippy::wrong_self_convention)]
pub trait Leveled {
    type Res;

    /// Mark as warning (will be logged at level `WARN`).
    ///
    /// Use this for user input parsing errors, etc..
    fn as_warn(self) -> Self::Res;

    /// Mark as internal error (will be logged at level `ERROR`).
    ///
    /// Use this for logical or unexpected state errors.
    fn as_error(self) -> Self::Res;
}

impl<T, E: Into<anyhow::Error>> Leveled for Result<T, E> {
    type Res = Result<T, Error>;

    #[inline]
    fn as_warn(self) -> Self::Res {
        self.map_err(|error| {
            LeveledError {
                level: tracing::Level::WARN,
                error: error.into(),
            }
            .into()
        })
    }

    #[inline]
    fn as_error(self) -> Self::Res {
        self.map_err(|error| {
            LeveledError {
                level: tracing::Level::ERROR,
                error: error.into(),
            }
            .into()
        })
    }
}

// macro_rules! error {
//     ($($tt:tt)+) => {
//         crate::Error(crate::LeveledError { level: tracing::Level::ERROR, error: anyhow::anyhow!($($tt)+) })
//     };
// }
// pub(crate) use error;

macro_rules! warning {
    ($($tt:tt)+) => {
        crate::LeveledError::new(tracing::Level::WARN, anyhow::anyhow!($($tt)+))
    };
}
pub(crate) use warning;

// NOTE: This is a macro (and not a method on `SonicError`) so the callsite
//   is correct.
#[macro_export]
macro_rules! log {
    ($error:expr) => {{
        let error = $error;
        match error.level() {
            tracing::Level::ERROR => tracing::error!("{:#}", error),
            tracing::Level::WARN => tracing::warn!("{:#}", error),
            _ => unreachable!("Only ERROR and WARN are supported"),
        }
    }};
}
