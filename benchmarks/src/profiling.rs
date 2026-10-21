// Sonic
//
// Fast, lightweight and schema-less search backend
// Copyright: 2026, Rémi Bardon <remi@remibardon.name>
// License: Mozilla Public License v2.0 (MPL v2.0)

use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;

use crate::RunContext;

pub enum ProfilingMode {
    Time,
    ThreadState,
}

pub static PROFILING_MODE: LazyLock<Option<ProfilingMode>> =
    LazyLock::new(|| match std::env::var("PROFILING_MODE") {
        Ok(val) => match val.as_str() {
            "time" => Some(ProfilingMode::Time),
            "thread_state" => Some(ProfilingMode::ThreadState),
            val => panic!(
                "Unknown profiling mode: {val:?}. Check your `PROFILING_MODE` environment variable."
            ),
        },
        Err(std::env::VarError::NotPresent) => None,
        Err(err @ std::env::VarError::NotUnicode(_)) => {
            panic!("Invalid `PROFILING_MODE` value: {err:?}")
        }
    });

impl RunContext {
    pub fn start_profiling(&mut self, profiling_mode: &ProfilingMode, output: PathBuf) {
        let sonic = self.sonic.as_ref().unwrap();

        let (mut xctrace_record, xctrace_output) = {
            let mut command = Command::new("xctrace");

            command.arg("record").arg("--output").arg(&output);

            (command, output)
        };

        match profiling_mode {
            ProfilingMode::Time => {
                xctrace_record
                    .args(["--instrument", "Time Profiler"])
                    // .args(["--instrument", "CPU Counters"])
                    .args(["--instrument", "CPU Profiler"])
                    .args(["--attach", &sonic.id().to_string()]);
            }
            ProfilingMode::ThreadState => {
                xctrace_record
                    .args(["--instrument", "Time Profiler"])
                    .args(["--instrument", "CPU Profiler"])
                    .args(["--instrument", "Thread State Trace"])
                    .args(["--attach", &sonic.id().to_string()]);
            }
        }

        let xctrace = xctrace_record.spawn().unwrap();

        // Give a bit of time for `xctrace` to startup.
        tracing::info!("Waiting for `xctrace` to start… (2s)");
        std::thread::sleep(std::time::Duration::from_millis(2000));

        self.xctrace = Some((xctrace, xctrace_output));
    }
}
