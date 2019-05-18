use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;
use threadpool::ThreadPool;

use crate::THREAD_NAME_COMMAND_THREAD_POOL;

lazy_static! {
    pub static ref COMMAND_POOL: CommandPool = CommandPool::new();
}

pub struct CommandPool {
    pub thread_pool: Mutex<ThreadPool>,
}

impl CommandPool {
    pub fn new() -> Self {
        Self {
            thread_pool: Mutex::new(
                threadpool::Builder::new()
                    .num_threads(10) // FIXME: should be manageable from config.cfg
                    .thread_name(THREAD_NAME_COMMAND_THREAD_POOL.into())
                    .build(),
            ),
        }
    }

    pub fn enqueue<F>(&self, func: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.thread_pool.lock().unwrap().execute(func);
    }
}

// tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_if_enqueued_functions_working_async() {
        assert_eq!(COMMAND_POOL.thread_pool.lock().unwrap().active_count(), 0);
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
        assert_eq!(COMMAND_POOL.thread_pool.lock().unwrap().active_count(), 3);
    }
}
