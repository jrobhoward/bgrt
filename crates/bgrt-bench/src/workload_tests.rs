//! Tests for the workload runner.
#![allow(non_snake_case)]

use std::time::Duration;

use bgrt::telemetry::Aggregate;
use parking_lot::Mutex;

use super::{WorkloadConfig, run};

#[test]
fn run____short_duration____records_samples_and_returns_work() {
    let cfg = WorkloadConfig {
        duration: Duration::from_millis(40),
        sample_interval: Duration::from_millis(5),
        workers: 1,
    };
    let agg = Mutex::new(Aggregate::default());
    let units = run(cfg, &agg);
    assert!(units >= 1, "expected at least one work unit");
    assert!(agg.lock().samples() >= 1);
}
