// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Crisp IM SAS
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Default)]
pub struct Shutdown {
    requested: Arc<AtomicBool>,
}

impl Shutdown {
    pub fn request(&self) {
        self.requested.store(true, Ordering::Release);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }
}

#[cfg(unix)]
mod platform {
    use nix::sys::signal::{SIGINT, SIGQUIT, SIGTERM, SigSet};

    pub struct ShutdownSignal(SigSet);

    impl Default for ShutdownSignal {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ShutdownSignal {
        pub fn new() -> Self {
            let mut mask = SigSet::empty();
            mask.add(SIGINT);
            mask.add(SIGQUIT);
            mask.add(SIGTERM);
            mask.thread_block().expect("cannot block shutdown signals");
            Self(mask)
        }

        pub fn wait(&self) -> usize {
            self.0.wait().expect("cannot wait for shutdown signal") as usize
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
    use std::sync::{LazyLock, Mutex};

    use windows_sys::Win32::Foundation::{BOOL, TRUE};
    use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;

    static CHANNEL: LazyLock<(SyncSender<u32>, Mutex<Receiver<u32>>)> = LazyLock::new(|| {
        let channel = sync_channel(1);
        (channel.0, Mutex::new(channel.1))
    });

    unsafe extern "system" fn handler(event: u32) -> BOOL {
        let _ = CHANNEL.0.try_send(event);
        TRUE
    }

    pub struct ShutdownSignal;

    impl Default for ShutdownSignal {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ShutdownSignal {
        pub fn new() -> Self {
            unsafe {
                SetConsoleCtrlHandler(Some(handler), TRUE);
            }
            Self
        }

        pub fn wait(&self) -> usize {
            CHANNEL.1.lock().unwrap().recv().unwrap() as usize
        }
    }
}

pub use platform::ShutdownSignal;
