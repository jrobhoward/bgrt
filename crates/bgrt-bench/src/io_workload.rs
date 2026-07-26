//! The random-read workload each executor runs against the scratch file, and
//! the aggregation of one phase's per-worker results.
//!
//! Reads only. Writes would not measure what the class controls: on Linux the
//! flusher thread issues buffered writeback, so the *writer's* I/O priority is
//! not what reaches the device, and macOS and Windows buffer them similarly.
//! Random reads are issued by, and accounted to, the classified thread.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::io_file::{self, AlignedBuf, CacheBypass, Reader};

/// Upper bound on retained latency samples per worker (~400 KiB).
const MAX_LATENCIES: usize = 100_000;

/// How long to read for, in what size chunks, over how much of the file.
#[derive(Debug, Clone, Copy)]
pub struct IoConfig {
    /// Wall-clock duration to keep each reader busy.
    pub duration: Duration,
    /// Read size in bytes (a multiple of [`io_file::ALIGN`]).
    pub block: usize,
    /// Number of block-sized slots in the scratch file.
    pub blocks: u64,
}

/// One reader's contribution to a phase.
#[derive(Debug, Default)]
pub struct WorkerStats {
    /// Bytes successfully read.
    pub bytes: u64,
    /// Completed reads.
    pub reads: u64,
    /// Failed reads (a handle that could not be opened counts as one).
    pub errors: u64,
    /// This worker's measured window.
    pub elapsed: Duration,
    /// Per-read latencies in microseconds, capped at [`MAX_LATENCIES`].
    pub latencies_us: Vec<u32>,
    /// Whether this worker's handle bypassed the cache.
    pub bypass: CacheBypass,
}

/// Read random blocks from `path` for `cfg.duration`, starting at `gate`.
///
/// Every reader in a phase is handed the same `gate`, so setup (spawning the
/// runtime, opening handles) lands *before* the measured window and a contended
/// phase is contended for all of it rather than only its tail. A shared instant
/// rather than a `Barrier`: readers run as tokio tasks, and a barrier would
/// deadlock if the scheduler didn't place all of them at once.
pub fn run_reader(path: &Path, cfg: IoConfig, seed: u64, gate: Instant) -> WorkerStats {
    match Reader::open(path) {
        Ok(reader) => read_until(reader, cfg, seed, gate),
        Err(_) => WorkerStats {
            errors: 1,
            ..WorkerStats::default()
        },
    }
}

/// The measured loop, once a handle is open.
///
/// The wait for `gate` is spent **reading**, not sleeping. A sleeping thread has
/// to be woken, and macOS defers timers for `QOS_CLASS_BACKGROUND` threads — a
/// background reader handed a sleep would wake after its window had already
/// closed and record nothing. Reading through the warm-up keeps every worker
/// runnable, and matches what a real background job looks like at the moment
/// foreground work shows up.
fn read_until(reader: Reader, cfg: IoConfig, seed: u64, gate: Instant) -> WorkerStats {
    let mut buf = AlignedBuf::new(cfg.block);
    let mut stats = WorkerStats {
        bypass: reader.bypass(),
        latencies_us: Vec::new(),
        ..WorkerStats::default()
    };
    let mut rand = seed | 1;
    let next_offset = |rand: &mut u64| {
        *rand = rand
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        io_file::block_offset(*rand >> 16, cfg.blocks, cfg.block)
    };

    // Warm-up: uncounted reads until the phase's window opens.
    while Instant::now() < gate {
        let offset = next_offset(&mut rand);
        let _ = reader.read_at(buf.as_mut_slice(), offset);
    }

    // Always at least one counted read, even if the window has already closed by
    // the time we get here — hence the loop-with-break rather than a `while`.
    // A synchronous read can't be cancelled, so the last warm-up read can start
    // just under the gate and finish past the deadline; on a contended
    // virtualized disk that single read can outlast a short window on its own.
    // Recording nothing there would report 0 MiB/s over a zero-length elapsed —
    // an executor that looks broken rather than slow, which is the failure mode
    // this harness exists to avoid. `workload::run` makes the same guarantee for
    // the CPU side ("at least one sample even for very short runs").
    let deadline = gate + cfg.duration;
    let start = Instant::now();
    loop {
        let offset = next_offset(&mut rand);
        let issued = Instant::now();
        match reader.read_at(buf.as_mut_slice(), offset) {
            Ok(n) => {
                stats.bytes = stats.bytes.saturating_add(n as u64);
                stats.reads = stats.reads.saturating_add(1);
                if stats.latencies_us.len() < MAX_LATENCIES {
                    let us = u32::try_from(issued.elapsed().as_micros()).unwrap_or(u32::MAX);
                    stats.latencies_us.push(us);
                }
            }
            Err(_) => stats.errors = stats.errors.saturating_add(1),
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    stats.elapsed = start.elapsed();
    stats
}

/// One phase's merged result: a set of readers running over the same window.
#[derive(Debug, Clone, Default)]
pub struct PhaseStats {
    /// Total bytes read across the phase's workers.
    pub bytes: u64,
    /// Total completed reads.
    pub reads: u64,
    /// Total failed reads.
    pub errors: u64,
    /// Longest worker window — the phase's wall clock.
    pub elapsed: Duration,
    /// Median read latency (µs).
    pub p50_us: Option<u32>,
    /// 95th-percentile read latency (µs).
    pub p95_us: Option<u32>,
    /// 99th-percentile read latency (µs).
    pub p99_us: Option<u32>,
    /// `Direct` only if every worker bypassed the cache.
    pub bypass: CacheBypass,
}

impl PhaseStats {
    /// Merge per-worker results into one phase.
    pub fn merge(workers: Vec<WorkerStats>) -> Self {
        let mut latencies = Vec::new();
        let mut phase = Self {
            bypass: CacheBypass::Direct,
            ..Self::default()
        };
        if workers.is_empty() {
            phase.bypass = CacheBypass::Buffered;
            return phase;
        }
        for w in workers {
            phase.bytes = phase.bytes.saturating_add(w.bytes);
            phase.reads = phase.reads.saturating_add(w.reads);
            phase.errors = phase.errors.saturating_add(w.errors);
            phase.elapsed = phase.elapsed.max(w.elapsed);
            phase.bypass = phase.bypass.merge(w.bypass);
            latencies.extend(w.latencies_us);
        }
        latencies.sort_unstable();
        phase.p50_us = percentile_us(&latencies, 0.50);
        phase.p95_us = percentile_us(&latencies, 0.95);
        phase.p99_us = percentile_us(&latencies, 0.99);
        phase
    }

    /// Read throughput in MiB/s, or 0.0 for an empty window.
    pub fn mib_per_s(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs <= 0.0 {
            return 0.0;
        }
        (self.bytes as f64 / (1024.0 * 1024.0)) / secs
    }

    /// Completed reads per second, or 0.0 for an empty window.
    pub fn iops(&self) -> f64 {
        let secs = self.elapsed.as_secs_f64();
        if secs <= 0.0 {
            return 0.0;
        }
        self.reads as f64 / secs
    }
}

/// Nearest-rank percentile over an ascending slice of latencies.
pub fn percentile_us(sorted: &[u32], p: f64) -> Option<u32> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted.get(idx).copied()
}

#[cfg(test)]
#[path = "io_workload_tests.rs"]
mod io_workload_tests;
