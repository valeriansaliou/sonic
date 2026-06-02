// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::fmt;

#[allow(dead_code)]
#[derive(Debug)]
enum PrettyLock<T: fmt::Debug> {
    Locked,
    Unlocked { data: T, poisoned: bool },
    Poisoned { data: T },
}

/// A type that formats `RwLock`s as “locked“/“unlocked”.
#[repr(transparent)]
pub struct AsPrettyRwLock<'this, T>(pub &'this std::sync::RwLock<T>);

impl<'this, T: fmt::Debug> fmt::Debug for AsPrettyRwLock<'this, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let val = match self.0.try_read() {
            Ok(guard) => PrettyLock::Unlocked {
                data: guard,
                poisoned: false,
            },
            Err(std::sync::TryLockError::Poisoned(err)) => PrettyLock::Poisoned {
                data: err.into_inner(),
            },
            Err(std::sync::TryLockError::WouldBlock) => PrettyLock::Locked,
        };

        #[allow(unused_qualifications)]
        if std::mem::size_of::<T>() > 1 {
            fmt::Debug::fmt(&val, f)
        } else {
            // Print inline if data holds no meaningful state.
            write!(f, "{val:?}")
        }
    }
}

/// A type that formats `Mutex`es in a more readable fashion.
#[repr(transparent)]
pub struct AsPrettyMutex<'this, T>(pub &'this std::sync::Mutex<T>);

impl<'this, T: fmt::Debug> fmt::Debug for AsPrettyMutex<'this, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let val = match self.0.try_lock() {
            Ok(guard) => PrettyLock::Unlocked {
                data: guard,
                poisoned: false,
            },
            Err(std::sync::TryLockError::Poisoned(err)) => PrettyLock::Poisoned {
                data: err.into_inner(),
            },
            Err(std::sync::TryLockError::WouldBlock) => PrettyLock::Locked,
        };

        #[allow(unused_qualifications)]
        if std::mem::size_of::<T>() > 1 {
            fmt::Debug::fmt(&val, f)
        } else {
            // Print inline if data holds no meaningful state.
            write!(f, "{val:?}")
        }
    }
}
