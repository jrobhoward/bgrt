//! Tests for the per-executor runners.
#![allow(non_snake_case)]

use std::time::Duration;

use super::{Executor, run};
use crate::workload::WorkloadConfig;

fn short_cfg() -> WorkloadConfig {
    WorkloadConfig {
        duration: Duration::from_millis(80),
        sample_interval: Duration::from_millis(10),
        workers: 1,
    }
}

#[test]
fn run____background_runtime____produces_result_with_work_and_samples() {
    let r = run(Executor::Background, short_cfg(), false, false).unwrap();
    assert!(r.work_units >= 1);
    assert!(r.aggregate.samples() >= 1);
    assert!(r.wall >= Duration::from_millis(60));
}

#[test]
fn run____background_threads____produces_result_with_work() {
    let r = run(Executor::BackgroundThreads, short_cfg(), false, false).unwrap();
    assert!(r.work_units >= 1);
    assert!(r.wall >= Duration::from_millis(60));
}
