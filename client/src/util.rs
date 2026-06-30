// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

pub(crate) mod errors {
    pub fn io_error_invalid_data<E: Into<Box<dyn std::error::Error + Send + Sync>>>(
        error: E,
    ) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidData, error)
    }
}

/// Builds a command String efficiently.
macro_rules! make_command {
    ($command:literal) => {{
        $crate::Command::from($command)
    }};

    ($format:literal $(, $arg:ident)* $(; text: $text:ident)? $(; options: $options:ident)?) => {{
        use std::fmt::Write as _;

        $(let $arg: &str = $arg.as_ref();)*
        $(let $text: &str = $text.as_ref();)?

        // NOTE: Since we can’t know if the argument will be quoted or not,
        //   this macro is a bit too generic and might waste a few bytes of
        //   capacity. Using `$format.len()` is a shortcut that’s good enough
        //   to account for spaces, quotes, etc. A single argument formatted
        //   as `{}` will reserve `len + 2`, although it will be printed as
        //   `len + 1` (preceding space). Same goes for `{:?}`, which will
        //   reserve 1 extraneous byte. It’s good enough. Options reserve 16
        //   bytes, as they should rarely be larger than this.
        let mut message: String = String::with_capacity(
            $format.len()
                $(+ $arg.len())*
                // NOTE: `*2` to account for escaping.
                $(+ ($text.len() * 2))?
                $(+ ($options.len() * 16))?
        );

        // SAFETY: `$arg`s are string slices, formatting cannot fail.
        write!(&mut message, $format$(, $arg)*).unwrap();

        #[allow(unused_mut, unused_assignments)]
        let (mut prefix_len, mut suffix_start) = (message.len(), message.len());

        $(
            message.push(' ');
            message.push('"');
            prefix_len = message.len();

            message.push_str($crate::util::escape_str($text).as_str());

            suffix_start = message.len();
            message.push('"');
        )?

        $(for option in $options {
            write!(&mut message, " {option}").map_err(std::io::Error::other)?;
        })?

        let suffix_len = message.len() - suffix_start;

        $crate::Command::new(message.into_boxed_str(), prefix_len, suffix_len)
    }};
}
pub(crate) use make_command;

/// Escapes special characters in strings.
pub(crate) fn escape_str(str: &str) -> String {
    // NOTE: Already allocate twice the space needed to avoid a re-allocation
    //   as soon as one character has to be escaped.
    let mut res = String::with_capacity(str.len() * 2);

    for c in str.chars() {
        match c {
            '"' => res.push_str("\\\""),
            '\r' => res.push_str("\\r"),
            '\n' => res.push_str("\\n"),
            '\\' => res.push_str("\\\\"),
            _ => res.push(c),
        }
    }

    res
}

macro_rules! impl_channel_structs {
    ($mode:ident($mode_lowercase:literal): $low_level_ty:ident / $blocking_ty:ident / $async_ty:ident) => {
        type LowLevelChannel = $low_level_ty;

        #[doc = concat!("A synchronous but non-blocking way to interact with a Sonic Channel in ", stringify!($mode), " mode.")]
        #[doc = concat!("\n\nShared logic for [`", stringify!($blocking_ty), "`] and [`", stringify!($async_ty),"`], which you should use instead.")]
        #[repr(transparent)]
        pub struct $low_level_ty {
            inner: SonicChannel<self::Mode>,
        }

        impl $low_level_ty {
            #[doc(alias = "new")]
            pub fn connect(
                addr: impl Into<std::net::SocketAddr>,
                pass: impl AsRef<str>,
                multiplexer: &$crate::SonicMultiplexer,
            ) -> std::io::Result<Self> {
                SonicChannel::<self::Mode>::connect::<crate::transport::SonicStream>(addr, pass, multiplexer)
                    .map(|inner| Self { inner })
            }

            /// Same as [`connect`][Self::connect] but allows choosing a
            /// different transport layer.
            ///
            /// This is useful in tests for example, to debug what’s going on
            /// by wrapping the TCP stream in a logging layer.
            pub fn connect_custom<T: crate::transport::Transport + 'static>(
                addr: impl Into<std::net::SocketAddr>,
                pass: impl AsRef<str>,
                multiplexer: &$crate::SonicMultiplexer,
            ) -> std::io::Result<Self> {
                SonicChannel::<self::Mode>::connect::<T>(addr, pass, multiplexer)
                    .map(|inner| Self { inner })
            }

            pub fn server_info(&self) -> &crate::events::ServerInfo {
                &self.inner.server_info
            }

            pub fn channel_info(&self) -> &crate::events::ChannelInfo {
                &self.inner.channel_info
            }

            fn quit_blocking_(&mut self) -> std::io::Result<()> {
                self.quit()?
                    .recv_timeout($crate::RECV_TIMEOUT)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::BrokenPipe, error))?
            }
        }

        impl Drop for $low_level_ty {
            #[inline]
            fn drop(&mut self) {
                if !self.inner.is_closed() {
                    $crate::logging::log_trace!(concat!("[Drop] Quitting ", stringify!($low_level_ty)));
                    self.quit_blocking_().unwrap_or_else(|error| crate::logging::log_error!("{error:?}"));
                }
            }
        }

        #[cfg(feature = "sync")]
        type BlockingChannel = $blocking_ty;

        #[doc = concat!("A blocking way to interact with a Sonic Channel in ", stringify!($mode), " mode.")]
        #[doc = concat!("\n\nWhen in an asynchronous context (e.g. using `tokio`), use [`", stringify!($async_ty), "`] instead.")]
        #[cfg(feature = "sync")]
        #[repr(transparent)]
        pub struct $blocking_ty {
            inner: $low_level_ty,
        }

        #[cfg(feature = "sync")]
        impl $blocking_ty {
            #[doc(alias = "new")]
            pub fn connect(
                addr: impl Into<std::net::SocketAddr>,
                pass: impl AsRef<str>,
                multiplexer: &$crate::SonicMultiplexer,
            ) -> std::io::Result<Self> {
                $low_level_ty::connect(addr, pass, multiplexer).map(|inner| Self { inner })
            }

            /// Same as [`connect`][Self::connect] but allows choosing a
            /// different transport layer.
            ///
            /// This is useful in tests for example, to debug what’s going on
            /// by wrapping the TCP stream in a logging layer.
            pub fn connect_custom<T: crate::transport::Transport + 'static>(
                addr: impl Into<std::net::SocketAddr>,
                pass: impl AsRef<str>,
                multiplexer: &$crate::SonicMultiplexer,
            ) -> std::io::Result<Self> {
                $low_level_ty::connect_custom::<T>(addr, pass, multiplexer).map(|inner| Self { inner })
            }

            pub fn server_info(&self) -> &crate::events::ServerInfo {
                self.inner.server_info()
            }

            pub fn channel_info(&self) -> &crate::events::ChannelInfo {
                self.inner.channel_info()
            }
        }

        #[cfg(feature = "async")]
        type AsyncChannel = $async_ty;

        #[doc = concat!("An asynchronous way to interact with a Sonic Channel in ", stringify!($mode), " mode.")]
        #[doc = concat!("\n\nIf you can’t be in an asynchronous context, use [`", stringify!($blocking_ty), "`] instead.")]
        #[cfg(feature = "async")]
        #[repr(transparent)]
        pub struct $async_ty {
            inner: $low_level_ty,
        }

        #[cfg(feature = "async")]
        impl $async_ty {
            #[doc(alias = "new")]
            pub fn connect(
                addr: impl Into<std::net::SocketAddr>,
                pass: impl AsRef<str>,
                multiplexer: &$crate::SonicMultiplexer,
            ) -> std::io::Result<Self> {
                $low_level_ty::connect(addr, pass, multiplexer).map(|inner| Self { inner })
            }

            /// Same as [`connect`][Self::connect] but allows choosing a
            /// different transport layer.
            ///
            /// This is useful in tests for example, to debug what’s going on
            /// by wrapping the TCP stream in a logging layer.
            pub fn connect_custom<T: crate::transport::Transport + 'static>(
                addr: impl Into<std::net::SocketAddr>,
                pass: impl AsRef<str>,
                multiplexer: &$crate::SonicMultiplexer,
            ) -> std::io::Result<Self> {
                $low_level_ty::connect_custom::<T>(addr, pass, multiplexer).map(|inner| Self { inner })
            }

            pub fn server_info(&self) -> &crate::events::ServerInfo {
                self.inner.server_info()
            }

            pub fn channel_info(&self) -> &crate::events::ChannelInfo {
                self.inner.channel_info()
            }
        }
    };
}
pub(crate) use impl_channel_structs;

/// Implements the given functions for all supported contexts (low-level, sync,
/// async), depending on enabled features.
///
/// NOTE: We must have two branches for `&self`, `&mut self`…
///   it’s annoying but I(@RemiBardon) found no way around it.
macro_rules! impl_fns {
    (
        $(#[$meta:meta])*
        fn $fn:ident $(<$($lifetime:lifetime),+>)? (&mut $self:ident $(, $arg_name:ident: $arg_ty:ty)* $(,)?) -> $ret_ty:ty $main:block
    ) => {
        impl self::LowLevelChannel {
            $(#[$meta])*
            pub fn $fn$(<$($lifetime),+>)?(
                &mut $self,
                $($arg_name: $arg_ty,)*
            ) -> std::io::Result<oneshot::Receiver<$ret_ty>> {
                $main
            }
        }

        #[cfg(feature = "sync")]
        impl self::BlockingChannel {
            $(#[$meta])*
            pub fn $fn$(<$($lifetime),+>)?(
                &mut $self,
                $($arg_name: $arg_ty,)*
            ) -> $ret_ty {
                $self.inner.$fn($($arg_name),*)?
                    .recv_timeout($crate::RECV_TIMEOUT)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::BrokenPipe, error))?
            }
        }

        #[cfg(feature = "async")]
        impl self::AsyncChannel {
            pub async fn $fn$(<$($lifetime),+>)?(
                &mut $self,
                $($arg_name: $arg_ty,)*
            ) -> $ret_ty {
                $self.inner.$fn($($arg_name),*)?
                    .await
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string()))?
            }
        }
    };

    (
        $(#[$meta:meta])*
        fn $fn:ident $(<$($lifetime:lifetime),+>)? (&$self:ident $(, $arg_name:ident: $arg_ty:ty)* $(,)?) -> $ret_ty:ty $main:block
    ) => {
        impl self::LowLevelChannel {
            $(#[$meta])*
            pub fn $fn$(<$($lifetime),+>)?(
                &$self,
                $($arg_name: $arg_ty,)*
            ) -> std::io::Result<oneshot::Receiver<$ret_ty>> {
                $main
            }
        }

        #[cfg(feature = "sync")]
        impl self::BlockingChannel {
            $(#[$meta])*
            pub fn $fn$(<$($lifetime),+>)?(
                &$self,
                $($arg_name: $arg_ty,)*
            ) -> $ret_ty {
                $self.inner.$fn($($arg_name),*)?
                    .recv_timeout($crate::RECV_TIMEOUT)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::BrokenPipe, error))?
            }
        }

        #[cfg(feature = "async")]
        impl self::AsyncChannel {
            pub async fn $fn$(<$($lifetime),+>)?(
                &$self,
                $($arg_name: $arg_ty,)*
            ) -> $ret_ty {
                $self.inner.$fn($($arg_name),*)?
                    .await
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::BrokenPipe, error.to_string()))?
            }
        }
    };
}
pub(crate) use impl_fns;
