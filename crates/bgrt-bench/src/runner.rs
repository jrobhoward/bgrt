//! Executors under comparison and the per-executor run loop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bgrt::QosClass;
use bgrt::telemetry::{Aggregate, EnergyMeter};
use parking_lot::Mutex;

use crate::power::PowerStats;
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
    /// Total work units completed across all workers (throughput numerator).
    pub work_units: u64,
    /// Folded self-samples (placement + frequency), where available.
    pub aggregate: Aggregate,
    /// Energy consumed during the run (Linux RAPL), if measurable.
    pub energy_uj: Option<u64>,
    /// macOS `powermetrics` stats for the run, if `--mac-power` and root.
    pub power: Option<PowerStats>,
}

/// Run the workload on `executor`, returning its measured result.
///
/// # Errors
///
/// Returns the [`bgrt::Error`] from building a runtime (for the runtime-backed
/// executors).
pub fn run(
    executor: Executor,
    cfg: WorkloadConfig,
    pin: bool,
    mac_power: bool,
) -> Result<RunResult, bgrt::Error> {
    match executor {
        Executor::BackgroundThreads => Ok(run_on_threads(executor, cfg, pin, mac_power)),
        _ => run_on_runtime(executor, cfg, pin, mac_power),
    }
}

fn run_on_runtime(
    executor: Executor,
    cfg: WorkloadConfig,
    pin: bool,
    mac_power: bool,
) -> Result<RunResult, bgrt::Error> {
    let rt = bgrt::Builder::new()
        .qos(executor.qos())
        .worker_threads(cfg.workers)
        .pin_efficiency_cores(pin)
        .build()?;

    let agg = Arc::new(Mutex::new(Aggregate::default()));
    let sampler = power_start(cfg, mac_power);
    let meter = EnergyMeter::start();
    let start = Instant::now();

    let mut handles = Vec::with_capacity(cfg.workers);
    for _ in 0..cfg.workers {
        let agg = Arc::clone(&agg);
        handles.push(rt.spawn(async move { workload::run(cfg, &agg) }));
    }
    let work_units = rt.block_on(async move {
        let mut total = 0u64;
        for h in handles {
            if let Ok(units) = h.await {
                total = total.saturating_add(units);
            }
        }
        total
    });

    let wall = start.elapsed();
    let energy_uj = meter.stop_uj();
    let power = power_finish(sampler);
    Ok(RunResult {
        executor,
        wall,
        work_units,
        aggregate: take_aggregate(agg),
        energy_uj,
        power,
    })
}

fn run_on_threads(
    executor: Executor,
    cfg: WorkloadConfig,
    pin: bool,
    mac_power: bool,
) -> RunResult {
    let agg = Arc::new(Mutex::new(Aggregate::default()));
    let sampler = power_start(cfg, mac_power);
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
    // On macOS, a higher-QoS thread that synchronously `join`s a background
    // thread promotes it off the efficiency cores (priority-inversion
    // avoidance). Match the waiter's QoS to the workers so the measurement
    // reflects the executor, not the join. (macOS lets us raise QoS back;
    // Linux/Windows don't promote on join, so this is macOS-only.)
    #[cfg(target_os = "macos")]
    let _ = bgrt::apply(executor.qos());

    let mut work_units = 0u64;
    for h in handles {
        if let Ok(units) = h.join() {
            work_units = work_units.saturating_add(units);
        }
    }

    #[cfg(target_os = "macos")]
    let _ = bgrt::apply(QosClass::Default);

    let wall = start.elapsed();
    let energy_uj = meter.stop_uj();
    let power = power_finish(sampler);
    RunResult {
        executor,
        wall,
        work_units,
        aggregate: take_aggregate(agg),
        energy_uj,
        power,
    }
}

/// Reclaim the aggregate after all workers have finished (the only `Arc` left).
fn take_aggregate(agg: Arc<Mutex<Aggregate>>) -> Aggregate {
    Arc::into_inner(agg)
        .map(Mutex::into_inner)
        .unwrap_or_default()
}

// --- powermetrics sampling (macOS only; no-op elsewhere) -------------------

#[cfg(target_os = "macos")]
fn power_start(cfg: WorkloadConfig, mac_power: bool) -> Option<crate::power_macos::Sampler> {
    if mac_power {
        crate::power_macos::Sampler::start(cfg.duration)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn power_finish(sampler: Option<crate::power_macos::Sampler>) -> Option<PowerStats> {
    sampler.and_then(crate::power_macos::Sampler::finish)
}

#[cfg(not(target_os = "macos"))]
fn power_start(_cfg: WorkloadConfig, _mac_power: bool) -> Option<()> {
    None
}

#[cfg(not(target_os = "macos"))]
fn power_finish(_sampler: Option<()>) -> Option<PowerStats> {
    None
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod runner_tests;
