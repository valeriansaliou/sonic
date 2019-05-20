use crate::APP_CONF;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::sync::Mutex;

lazy_static! {
    pub static ref COMMAND_POOL: CommandPool = CommandPool::new();
}

pub struct CommandPool {
    pub thread_pool: Mutex<ThreadPool>,
}

impl CommandPool {
    pub fn new() -> Self {
        let num_threads = APP_CONF.channel.command_pool_num_threads;
        debug!("initializing command pool, num threads: {}", num_threads);
        Self {
            thread_pool: Mutex::new(
                ThreadPoolBuilder::new()
                    .num_threads(num_threads)
                    .build()
                    .unwrap(),
            ),
        }
    }

    pub fn enqueue<'a, F>(&self, func: F)
    where
        F: FnOnce() + Send + 'a,
    {
        self.thread_pool.lock().unwrap().scope(|s| {
            s.spawn(move |_| func());
        });
    }
}

// tests

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn checks_if_enqueued_functions_working_async() {
        assert_eq!(
            COMMAND_POOL
                .thread_pool
                .lock()
                .unwrap()
                .current_num_threads(),
            0
        );
        COMMAND_POOL.enqueue(|| {
            sleep(Duration::from_millis(500));
        });
        COMMAND_POOL.enqueue(|| {
            sleep(Duration::from_millis(500));
        });
        COMMAND_POOL.enqueue(|| {
            sleep(Duration::from_millis(500));
        });
        sleep(Duration::from_millis(100));
        assert_eq!(
            COMMAND_POOL
                .thread_pool
                .lock()
                .unwrap()
                .current_num_threads(),
            3
        );
    }
}
