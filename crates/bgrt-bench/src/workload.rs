//! The CPU-bound workload that each executor runs.
//!
//! The workload keeps a core busy for a fixed duration and self-samples
//! telemetry as it goes, so core placement / frequency are attributed to the
//! thread actually doing the work. It returns the number of work units
//! (fixed-size compute chunks) completed, which is the throughput signal: a
//! slower (efficiency-core, low-clock) executor completes fewer in the same wall
//! time.

use std::time::{Duration, Instant};

use bgrt::telemetry::{self, Aggregate};
use parking_lot::Mutex;

/// Iterations of inner work per counted unit.
const WORK_UNIT_ITERS: u64 = 200_000;

/// How long to run, how often to self-sample, and how many workers to use.
#[derive(Debug, Clone, Copy)]
pub struct WorkloadConfig {
    /// Wall-clock duration to keep each worker busy.
    pub duration: Duration,
    /// Interval between telemetry self-samples.
    pub sample_interval: Duration,
    /// Number of concurrent workers (tasks or threads).
    pub workers: usize,
}

/// Run a CPU-bound loop until `cfg.duration` elapses, folding self-samples into
/// `agg`, and return the number of work units completed. Takes at least one
/// sample even for very short runs.
pub fn run(cfg: WorkloadConfig, agg: &Mutex<Aggregate>) -> u64 {
    let start = Instant::now();
    // Bias the first sample to fire on the opening iteration.
    let mut last_sample = start.checked_sub(cfg.sample_interval).unwrap_or(start);
    let mut acc: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut units: u64 = 0;

    while start.elapsed() < cfg.duration {
        // A chunk of pure CPU work (an LCG mix); black_box defeats elision.
        for _ in 0..WORK_UNIT_ITERS {
            acc = acc
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
        }
        std::hint::black_box(acc);
        units = units.saturating_add(1);

        if last_sample.elapsed() >= cfg.sample_interval {
            agg.lock().record(telemetry::sample());
            last_sample = Instant::now();
        }
    }
    units
}

#[cfg(test)]
#[path = "workload_tests.rs"]
mod workload_tests;
