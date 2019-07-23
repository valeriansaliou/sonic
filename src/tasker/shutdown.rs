// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2019, Valerian Saliou <valerian@valeriansaliou.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

#[cfg(windows)]
mod platform {
    // Notice: the following module is inspired from a fork of `graceful`, which implements \
    //   Windows support upon the original `graceful` crate; find the fork at: \
    //   https://github.com/Git0Shuai/graceful

    use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
    use std::sync::Mutex;

    use winapi::shared::minwindef::{BOOL, DWORD, TRUE};
    use winapi::um::consoleapi::SetConsoleCtrlHandler;

    lazy_static! {
        static ref CHANNEL: (SyncSender<DWORD>, Mutex<Receiver<DWORD>>) = {
            let channel = sync_channel(0);

            (channel.0, Mutex::new(channel.1))
        };
    }

    unsafe extern "system" fn handler(event: DWORD) -> BOOL {
        CHANNEL.0.send(event).unwrap();
        CHANNEL.0.send(0).unwrap();

        TRUE
    }

    pub struct ShutdownSignal;

    impl ShutdownSignal {
        pub fn new() -> ShutdownSignal {
            unsafe { SetConsoleCtrlHandler(Some(handler), TRUE) };

            ShutdownSignal
        }

        pub fn at_exit<F: FnOnce(usize)>(&self, handler: F) {
            let event = {
                let receiver = CHANNEL.1.lock().unwrap();

                receiver.recv().unwrap()
            };

            handler(event as usize);

            CHANNEL.1.lock().unwrap().recv().unwrap();
        }
    }
}

#[cfg(unix)]
mod platform {
    // Notice: the following module is inspired from `graceful`, which can be found at: \
    //   https://github.com/0x1997/graceful

    use nix::sys::signal::{SigSet, SIGINT, SIGQUIT, SIGTERM};

    pub struct ShutdownSignal(SigSet);

    impl ShutdownSignal {
        pub fn new() -> ShutdownSignal {
            let mut mask = SigSet::empty();

            ShutdownSignal::init(&mut mask).unwrap();
            ShutdownSignal(mask)
        }

        fn init(mask: &mut SigSet) -> nix::Result<()> {
            mask.add(SIGINT);
            mask.add(SIGQUIT);
            mask.add(SIGTERM);

            mask.thread_block()
        }

        pub fn at_exit<F: FnOnce(usize)>(&self, handler: F) {
            let signal = self.0.wait().unwrap();

            handler(signal as usize);
        }
    }
}

pub use platform::ShutdownSignal;
