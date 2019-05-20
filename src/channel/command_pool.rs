use std::sync::Mutex;
use rayon::{ThreadPool, ThreadPoolBuilder};

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
                ThreadPoolBuilder::new().num_threads(8).build().unwrap()  // FIXME: should be manageable from config.cfg
            ),
        }
    }

    pub fn enqueue<'a, F>(&self, func: F)
    where
        F: FnOnce() + Send + 'a
    {
        self.thread_pool.lock().unwrap().scope(|s| {
            s.spawn(move |_| {
                func();
            });
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
        assert_eq!(COMMAND_POOL.thread_pool.lock().unwrap().current_num_threads(), 0);
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
        assert_eq!(COMMAND_POOL.thread_pool.lock().unwrap().current_num_threads(), 3);
    }
}
