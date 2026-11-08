// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use crate::util::impl_transparent_wrapper_utils;

pub use SonicError as Error;

pub struct SonicError(LeveledError);

impl_transparent_wrapper_utils!(Debug for Error(LeveledError));
impl_transparent_wrapper_utils!(Display for Error(LeveledError));
impl_transparent_wrapper_utils!(Deref for Error(LeveledError));

impl<E> From<E> for Error
where
    E: Attachable,
    TracedError<E>: Into<anyhow::Error>,
{
    #[inline]
    #[track_caller]
    fn from(inner: E) -> Self {
        Self(LeveledError {
            level: tracing::Level::ERROR,
            error: inner
                // NOTE: The leading space is there so terminals detect paths
                //   better for easier navigation during debugging.
                .attach(crate::fields!(callsite = %format!(" {callsite}", callsite = std::panic::Location::caller())))
                .into(),
        })
    }
}

impl<E> From<TracedError<E>> for Error
where
    TracedError<E>: Into<anyhow::Error>,
{
    #[inline]
    fn from(inner: TracedError<E>) -> Self {
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

/// A String wrapper that implements [`std::error::Error`] so we can convert
/// [`TracedError<StringError>`] to [`anyhow::Error`].
///
/// This is useful for the [`Option<T>`] implementation of [`Context`].
struct StringError(String);

impl_transparent_wrapper_utils!(Debug for StringError(String));
impl_transparent_wrapper_utils!(Display for StringError(String));

impl std::error::Error for StringError {}

impl<T> Context for Option<T> {
    type Res = Result<T, Error>;

    #[inline]
    fn context<C>(self, context: C) -> Self::Res
    where
        C: std::fmt::Display + Send + Sync + 'static,
    {
        self.ok_or(Error::from(
            StringError(context.to_string()).attach_current_span(),
        ))
    }

    #[inline]
    fn with_context<C, F>(self, f: F) -> Self::Res
    where
        C: std::fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> C,
    {
        self.ok_or_else(|| Error::from(StringError(f().to_string()).attach_current_span()))
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

// NOTE: This type is necessary to avoid some conflicting implementations errors,
//   and said implementations are necessary to avoid .
#[repr(transparent)]
pub struct TracedError<E>(tracing_error::TracedError<E>);

impl<E: Into<tracing_error::TracedError<E>>> From<E> for TracedError<E> {
    #[inline]
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl<E> From<TracedError<E>> for anyhow::Error
where
    tracing_error::TracedError<E>: Into<anyhow::Error>,
{
    #[inline]
    fn from(value: TracedError<E>) -> Self {
        value.0.into()
    }
}

pub trait Attachable: Sized {
    fn attach(self, span: tracing::Span) -> TracedError<Self>;

    fn attach_current_span(self) -> TracedError<Self>;
}

impl<E> Attachable for E
where
    E: std::error::Error + Send + Sync + 'static,
{
    #[inline]
    fn attach(self, span: tracing::Span) -> TracedError<E> {
        span.in_scope(|| TracedError(tracing_error::TracedError::from(self)))
    }

    #[inline]
    fn attach_current_span(self) -> TracedError<E> {
        TracedError(tracing_error::TracedError::from(self))
    }
}

pub trait AttachResult<T, E> {
    fn attach(self, span: tracing::Span) -> Result<T, TracedError<E>>;
}

impl<T, E: Attachable> AttachResult<T, E> for Result<T, E> {
    #[inline]
    fn attach(self, span: tracing::Span) -> Result<T, TracedError<E>> {
        self.map_err(|e| e.attach(span))
    }
}

#[macro_export]
macro_rules! fields {
    ($($tt:tt)+) => {
        tracing::span!(tracing::Level::ERROR, "_context", $($tt)+)
    };
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
