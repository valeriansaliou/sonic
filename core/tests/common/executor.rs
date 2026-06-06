// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::sync::Arc;

use sonic::store::{fst::StoreFSTPool, kv::StoreKVPool};

use crate::common::LOG_LEVEL;

use super::config::*;

/// A guard that prints the executor state when the test fails for easier
/// debugging.
pub struct ExecutorGuard {
    id: String,
    pub executor: sonic::Executor,
}

impl ExecutorGuard {
    fn id(&self) -> String {
        if self.id.is_empty() {
            "Executor".to_owned()
        } else {
            format!("Executor#{}", self.id)
        }
    }

    pub fn log(&self, line: impl std::fmt::Display) {
        if *LOG_LEVEL >= tracing::Level::INFO {
            eprintln!("{}: {line}", self.id());
        }
    }
}

impl Drop for ExecutorGuard {
    fn drop(&mut self) {
        let test_failed = std::thread::panicking();

        // Print executor state on test failures for easier debugging.
        if test_failed {
            if *LOG_LEVEL > tracing::Level::INFO {
                self.log(format!("\nexecutor={:#?}", self.executor));
            }
        } else {
            self.log("[Drop]\n");
        }

        // TODO: Cleanup data directory on success.
    }
}

pub fn make_test_executor_with_id(id: impl ToString) -> ExecutorGuard {
    let app_conf = make_config(&defaults_toml());

    // Create connection pools (does not open any connection yet)
    let kv_pool = StoreKVPool::new(Arc::clone(&app_conf.store.kv));
    let fst_pool = StoreFSTPool::new(Arc::clone(&app_conf.store.fst), Default::default());

    ExecutorGuard {
        id: id.to_string(),
        executor: sonic::Executor {
            app_conf: Arc::new(app_conf),
            kv_pool,
            fst_pool,
        },
    }
}

pub fn make_test_executor() -> ExecutorGuard {
    make_test_executor_with_id("")
}

// MARK: - Boilerplate

impl std::ops::Deref for ExecutorGuard {
    type Target = sonic::Executor;

    fn deref(&self) -> &Self::Target {
        &self.executor
    }
}

impl std::ops::DerefMut for ExecutorGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.executor
    }
}
