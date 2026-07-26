//! Tests for the disk workload's aggregation and percentiles.
#![allow(non_snake_case)]

use std::time::{Duration, Instant};

use rstest::rstest;

use super::{IoConfig, PhaseStats, WorkerStats, percentile_us, run_reader};
use crate::io_file::{CacheBypass, ScratchFile};

fn worker(bytes: u64, reads: u64, elapsed_ms: u64, bypass: CacheBypass) -> WorkerStats {
    WorkerStats {
        bytes,
        reads,
        errors: 0,
        elapsed: Duration::from_millis(elapsed_ms),
        latencies_us: vec![10; reads as usize],
        bypass,
    }
}

#[rstest]
#[case(0.50, Some(5))]
#[case(0.95, Some(10))]
#[case(0.99, Some(10))]
#[case(0.0, Some(1))]
fn percentile_us____ascending_samples____nearest_rank(
    #[case] p: f64,
    #[case] expected: Option<u32>,
) {
    let sorted: Vec<u32> = (1..=10).collect();
    assert_eq!(percentile_us(&sorted, p), expected);
}

#[test]
fn percentile_us____no_samples____is_none() {
    assert_eq!(percentile_us(&[], 0.95), None);
}

#[test]
fn merge____several_workers____sums_bytes_and_takes_the_longest_window() {
    let phase = PhaseStats::merge(vec![
        worker(3 * 1024 * 1024, 3, 1000, CacheBypass::Direct),
        worker(1024 * 1024, 1, 1500, CacheBypass::Direct),
    ]);
    assert_eq!(phase.bytes, 4 * 1024 * 1024);
    assert_eq!(phase.reads, 4);
    assert_eq!(phase.elapsed, Duration::from_millis(1500));
    assert_eq!(phase.bypass, CacheBypass::Direct);
    // 4 MiB over the longest window (1.5 s).
    assert!((phase.mib_per_s() - 4.0 / 1.5).abs() < 1e-6);
    assert!((phase.iops() - 4.0 / 1.5).abs() < 1e-6);
}

#[test]
fn merge____one_buffered_worker____taints_the_phase() {
    let phase = PhaseStats::merge(vec![
        worker(1024, 1, 100, CacheBypass::Direct),
        worker(1024, 1, 100, CacheBypass::Buffered),
    ]);
    assert_eq!(phase.bypass, CacheBypass::Buffered);
}

#[test]
fn merge____no_workers____is_zero_and_buffered() {
    let phase = PhaseStats::merge(Vec::new());
    assert_eq!(phase.bytes, 0);
    assert_eq!(phase.bypass, CacheBypass::Buffered);
    assert_eq!(phase.mib_per_s(), 0.0);
    assert_eq!(phase.iops(), 0.0);
}

#[test]
fn run_reader____real_scratch_file____reads_whole_blocks_without_errors() {
    let dir = tempfile::tempdir().unwrap();
    let block = 64 * 1024;
    let scratch = ScratchFile::create(dir.path(), 4 * 1024 * 1024, false).unwrap();
    let cfg = IoConfig {
        duration: Duration::from_millis(60),
        block,
        blocks: crate::io_file::blocks_in(scratch.size(), block),
    };

    let stats = run_reader(scratch.path(), cfg, 12_345, Instant::now());
    assert_eq!(stats.errors, 0, "reads failed on the scratch file");
    assert!(stats.reads >= 1, "expected at least one read");
    assert_eq!(
        stats.bytes,
        stats.reads * block as u64,
        "short read: every read should return a whole block"
    );
    assert_eq!(stats.latencies_us.len() as u64, stats.reads);
}

#[test]
fn run_reader____missing_file____reports_an_error_rather_than_panicking() {
    let cfg = IoConfig {
        duration: Duration::from_millis(10),
        block: 4096,
        blocks: 1,
    };
    let stats = run_reader(
        std::path::Path::new("/nonexistent/bgrt-bench-io.dat"),
        cfg,
        1,
        Instant::now(),
    );
    assert_eq!(stats.reads, 0);
    assert_eq!(stats.errors, 1);
}

#[test]
fn run_reader____window_already_closed____still_records_one_read() {
    // The Windows CI failure this guards: a slow read straddles a short window,
    // the deadline passes during it, and the worker would otherwise report zero
    // reads over zero elapsed — indistinguishable from a broken executor. A
    // zero-length window reproduces that deterministically, with no timing race.
    let dir = tempfile::tempdir().unwrap();
    let block = 64 * 1024;
    let scratch = ScratchFile::create(dir.path(), 4 * 1024 * 1024, false).unwrap();
    let cfg = IoConfig {
        duration: Duration::ZERO,
        block,
        blocks: crate::io_file::blocks_in(scratch.size(), block),
    };

    let stats = run_reader(scratch.path(), cfg, 99, Instant::now());
    assert_eq!(
        stats.reads, 1,
        "expected exactly one read for a closed window"
    );
    assert_eq!(stats.errors, 0);
    assert!(
        stats.elapsed > Duration::ZERO,
        "elapsed must be non-zero so throughput is finite"
    );
    // ...and that read is a real one, so the phase reports throughput, not 0.0.
    let phase = PhaseStats::merge(vec![stats]);
    assert!(phase.mib_per_s() > 0.0);
}
