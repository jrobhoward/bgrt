//! Executors under comparison and the per-executor run loop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bgrt::QosClass;
use bgrt::telemetry::{Aggregate, EnergyMeter};
use parking_lot::Mutex;

use crate::workload::{self, WorkloadConfig};

/// The executors the harness can compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Executor {
    /// Default-class bgrt runtime (no energy hint).
    Default,
    /// Utility-class bgrt runtime.
    Utility,
    /// Background-class bgrt runtime.
    Background,
    /// Background-class work on plain spawned OS threads.
    BackgroundThreads,
}

impl Executor {
    /// Stable lowercase label (also used as the JSON/table key).
    pub fn label(self) -> &'static str {
        match self {
            Executor::Default => "default",
            Executor::Utility => "utility",
            Executor::Background => "background",
            Executor::BackgroundThreads => "background-threads",
        }
    }

    fn qos(self) -> QosClass {
        match self {
            Executor::Default => QosClass::Default,
            Executor::Utility => QosClass::Utility,
            Executor::Background | Executor::BackgroundThreads => QosClass::Background,
        }
    }
}

/// Outcome of running the workload on one executor.
pub struct RunResult {
    /// The executor that produced this result.
    pub executor: Executor,
    /// Total wall-clock time for the run.
    pub wall: Duration,
    /// Folded self-samples (placement + frequency).
    pub aggregate: Aggregate,
    /// Energy consumed during the run, if measurable.
    pub energy_uj: Option<u64>,
}

/// Run the workload on `executor`, returning its measured result.
///
/// # Errors
///
/// Returns the [`bgrt::Error`] from building a runtime (for the runtime-backed
/// executors).
pub fn run(executor: Executor, cfg: WorkloadConfig, pin: bool) -> Result<RunResult, bgrt::Error> {
    match executor {
        Executor::BackgroundThreads => Ok(run_on_threads(executor, cfg, pin)),
        _ => run_on_runtime(executor, cfg, pin),
    }
}

fn run_on_runtime(
    executor: Executor,
    cfg: WorkloadConfig,
    pin: bool,
) -> Result<RunResult, bgrt::Error> {
    let rt = bgrt::Builder::new()
        .qos(executor.qos())
        .worker_threads(cfg.workers)
        .pin_efficiency_cores(pin)
        .build()?;

    let agg = Arc::new(Mutex::new(Aggregate::default()));
    let meter = EnergyMeter::start();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(cfg.workers);
    for _ in 0..cfg.workers {
        let agg = Arc::clone(&agg);
        handles.push(rt.spawn(async move { workload::run(cfg, &agg) }));
    }
    rt.block_on(async move {
        for h in handles {
            let _ = h.await;
        }
    });

    let wall = start.elapsed();
    let energy_uj = meter.stop_uj();
    Ok(RunResult {
        executor,
        wall,
        aggregate: take_aggregate(agg),
        energy_uj,
    })
}

fn run_on_threads(executor: Executor, cfg: WorkloadConfig, pin: bool) -> RunResult {
    let agg = Arc::new(Mutex::new(Aggregate::default()));
    let meter = EnergyMeter::start();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(cfg.workers);
    for _ in 0..cfg.workers {
        let agg = Arc::clone(&agg);
        let spawned = bgrt::ThreadBuilder::new()
            .qos(executor.qos())
            .pin_efficiency_cores(pin)
            .spawn(move || workload::run(cfg, &agg));
        match spawned {
            Ok(h) => handles.push(h),
            Err(e) => eprintln!("warning: failed to spawn worker thread: {e}"),
        }
    }
    for h in handles {
        let _ = h.join();
    }

    let wall = start.elapsed();
    let energy_uj = meter.stop_uj();
    RunResult {
        executor,
        wall,
        aggregate: take_aggregate(agg),
        energy_uj,
    }
}

/// Reclaim the aggregate after all workers have finished (the only `Arc` left).
fn take_aggregate(agg: Arc<Mutex<Aggregate>>) -> Aggregate {
    Arc::into_inner(agg)
        .map(Mutex::into_inner)
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;
