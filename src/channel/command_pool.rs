use crate::APP_CONF;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::sync::{Arc, Mutex};

lazy_static! {
    pub static ref COMMAND_POOL: CommandPool = CommandPool::new();
}

pub struct CommandPool {
    pub thread_pool: Arc<Mutex<ThreadPool>>,
}

impl CommandPool {
    pub fn new() -> Self {
        let num_threads = APP_CONF.channel.command_pool_num_threads;
        debug!("initializing command pool, num threads: {}", num_threads);
        Self {
            thread_pool: Arc::new(Mutex::new(
                ThreadPoolBuilder::new()
                    .num_threads(num_threads)
                    .build()
                    .unwrap(),
            )),
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
    // use std::thread::sleep;
    // use std::time::Duration;

    #[test]
    // FIXME: threadpool is not asyn so it'f disabled.
    // fn checks_if_enqueued_functions_working_async() {
    //     let index: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    //     COMMAND_POOL.enqueue(|| {
    //         let mut index = index.lock().unwrap();
    //         sleep(Duration::from_millis(10000));
    //         *index += 1_usize;
    //         sleep(Duration::from_millis(50));
    //         *index += 1_usize;
    //     });
    //     COMMAND_POOL.enqueue(|| {
    //         let mut index = index.lock().unwrap();
    //         sleep(Duration::from_millis(100));
    //         *index += 1_usize;
    //         sleep(Duration::from_millis(70));
    //         *index += 1_usize;
    //     });
    //     COMMAND_POOL.enqueue(|| {
    //         let mut index = index.lock().unwrap();
    //         sleep(Duration::from_millis(100));
    //         *index += 1_usize;
    //         sleep(Duration::from_millis(90));
    //         // *index += 1_usize;
    //     });
    //     assert_eq!(*index.lock().unwrap(), 0_usize);
    //     // sleep(Duration::from_millis(20));
    //     // assert_eq!(*index.lock().unwrap(), 3_usize);
    //     // sleep(Duration::from_millis(60));
    //     // assert_eq!(*index.lock().unwrap(), 4_usize);
    //     // sleep(Duration::from_millis(80));
    //     // assert_eq!(*index.lock().unwrap(), 5_usize);
    //     // sleep(Duration::from_millis(100));
    //     // assert_eq!(*index.lock().unwrap(), 6_usize);
    // }
    #[test]
    fn num_thread_should_be_default() {
        assert_eq!(
            COMMAND_POOL
                .thread_pool
                .lock()
                .unwrap()
                .current_num_threads(),
            APP_CONF.channel.command_pool_num_threads
        );
    }
}
