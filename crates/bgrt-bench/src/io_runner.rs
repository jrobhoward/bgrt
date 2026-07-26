//! Running the disk workload on each executor, solo and under contention.
//!
//! I/O priority is a *contention* mechanism on all three platforms — on an idle
//! device, low-priority reads run at close to full speed everywhere (macOS only
//! inserts throttle delays while higher-tier I/O is in flight; Linux's BFQ and
//! `mq-deadline` only differentiate when requests queue). Measuring an executor
//! alone therefore cannot show the class working. Each executor runs twice:
//!
//! 1. **solo** — the executor's readers alone, for its own throughput, and
//! 2. **contended** — the same readers alongside plain, unclassified OS threads
//!    standing in for an ordinary foreground app.
//!
//! The signal is what happens to the *foreground* side in phase 2, compared with
//! a baseline of those same threads running alone: a `Background` executor should
//! give way, leaving the foreground near its solo throughput.

use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::io_workload::{self, IoConfig, PhaseStats, WorkerStats};
use crate::runner::Executor;

/// Setup headroom before a phase's measured window opens: enough to build a
/// runtime, spawn threads, and open handles.
const WARMUP: Duration = Duration::from_millis(250);

/// Seeds, kept distinct so foreground and background read different sequences.
const FG_SEED: u64 = 0x5170_1b3a_9c2d_11e5;
const BG_SEED: u64 = 0xa1b2_c3d4_e5f6_0718;

/// Everything a disk run needs, shared by every executor.
#[derive(Debug, Clone)]
pub struct IoPlan {
    /// Duration / block size / file geometry.
    pub cfg: IoConfig,
    /// Path of the scratch file.
    pub path: PathBuf,
    /// Readers on the executor under test.
    pub workers: usize,
    /// Plain OS threads standing in for a foreground app.
    pub foreground: usize,
    /// Pin to efficiency cores (Linux only).
    pub pin: bool,
    /// Clamp frequency via `uclamp` (Linux only).
    pub clamp: bool,
}

/// One executor's disk result.
pub struct IoRunResult {
    /// The executor that produced this result.
    pub executor: Executor,
    /// The executor's readers running alone.
    pub solo: PhaseStats,
    /// The executor's readers running against the foreground threads.
    pub contended: PhaseStats,
    /// The foreground threads during that same contended window.
    pub foreground: PhaseStats,
}

/// Measure the foreground threads running alone — the denominator for every
/// executor's "did it get out of the way?" figure. Run once per invocation.
pub fn foreground_baseline(plan: &IoPlan) -> PhaseStats {
    let gate = Instant::now() + WARMUP;
    PhaseStats::merge(join(spawn_foreground(plan, gate)))
}

/// Run both phases for one executor.
///
/// # Errors
///
/// Returns the [`bgrt::Error`] from building a runtime, for the runtime-backed
/// executors.
pub fn run(executor: Executor, plan: &IoPlan) -> Result<IoRunResult, bgrt::Error> {
    let solo = PhaseStats::merge(on_executor(executor, plan, Instant::now() + WARMUP)?);

    let gate = Instant::now() + WARMUP;
    let fg = spawn_foreground(plan, gate);
    let contended = PhaseStats::merge(on_executor(executor, plan, gate)?);
    let foreground = PhaseStats::merge(join(fg));

    Ok(IoRunResult {
        executor,
        solo,
        contended,
        foreground,
    })
}

/// Spawn the foreground readers on *plain* OS threads: unclassified on purpose,
/// since they represent whatever else the machine is doing.
fn spawn_foreground(plan: &IoPlan, gate: Instant) -> Vec<JoinHandle<WorkerStats>> {
    (0..plan.foreground)
        .map(|i| {
            let (path, cfg, seed) = (plan.path.clone(), plan.cfg, seed(FG_SEED, i));
            std::thread::spawn(move || io_workload::run_reader(&path, cfg, seed, gate))
        })
        .collect()
}

/// Run the workload on the executor under test.
fn on_executor(
    executor: Executor,
    plan: &IoPlan,
    gate: Instant,
) -> Result<Vec<WorkerStats>, bgrt::Error> {
    match executor {
        Executor::BackgroundThreads => Ok(on_threads(executor, plan, gate)),
        _ => on_runtime(executor, plan, gate),
    }
}

fn on_runtime(
    executor: Executor,
    plan: &IoPlan,
    gate: Instant,
) -> Result<Vec<WorkerStats>, bgrt::Error> {
    let rt = bgrt::RuntimeBuilder::new()
        .qos(executor.qos())
        .worker_threads(plan.workers)
        .pin_efficiency_cores(plan.pin)
        .clamp_frequency(plan.clamp)
        .build()?;

    let mut handles = Vec::with_capacity(plan.workers);
    for i in 0..plan.workers {
        let (path, cfg, seed) = (plan.path.clone(), plan.cfg, seed(BG_SEED, i));
        handles.push(rt.spawn(async move { io_workload::run_reader(&path, cfg, seed, gate) }));
    }
    Ok(rt.block_on(async move {
        let mut stats = Vec::new();
        for h in handles {
            if let Ok(s) = h.await {
                stats.push(s);
            }
        }
        stats
    }))
}

fn on_threads(executor: Executor, plan: &IoPlan, gate: Instant) -> Vec<WorkerStats> {
    let mut handles = Vec::with_capacity(plan.workers);
    for i in 0..plan.workers {
        let (path, cfg, seed) = (plan.path.clone(), plan.cfg, seed(BG_SEED, i));
        let spawned = bgrt::ThreadBuilder::new()
            .qos(executor.qos())
            .pin_efficiency_cores(plan.pin)
            .clamp_frequency(plan.clamp)
            .spawn(move || io_workload::run_reader(&path, cfg, seed, gate));
        match spawned {
            Ok(h) => handles.push(h),
            Err(e) => eprintln!("warning: failed to spawn reader thread: {e}"),
        }
    }
    // Same reason as the CPU runner: on macOS a higher-QoS thread joining a
    // background one promotes it, which would undo the class being measured.
    #[cfg(target_os = "macos")]
    let _ = bgrt::apply(executor.qos());

    let stats = join(handles);

    #[cfg(target_os = "macos")]
    let _ = bgrt::apply(bgrt::QosClass::Default);

    stats
}

/// Collect worker results, dropping any that panicked.
fn join(handles: Vec<JoinHandle<WorkerStats>>) -> Vec<WorkerStats> {
    handles.into_iter().filter_map(|h| h.join().ok()).collect()
}

/// Per-worker seed: distinct streams, deterministic across runs.
fn seed(base: u64, worker: usize) -> u64 {
    base ^ (worker as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

#[cfg(test)]
#[path = "io_runner_tests.rs"]
mod io_runner_tests;
