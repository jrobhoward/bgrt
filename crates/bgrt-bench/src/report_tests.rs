//! Tests for report formatting and the comparison check.
#![allow(non_snake_case)]

use std::time::Duration;

use bgrt::telemetry::Aggregate;

use super::{
    IoReport, IoSummary, Summary, background_not_hotter, background_yields_disk, device_saturated,
    io_json, io_table, json, table,
};
use crate::io_file::CacheBypass;
use crate::io_runner::IoRunResult;
use crate::io_workload::PhaseStats;
use crate::power::PowerStats;
use crate::runner::{Executor, RunResult};

fn summ(executor: &str, max_mhz: Option<u32>) -> Summary {
    Summary {
        executor: executor.to_owned(),
        wall_ms: 1000,
        work_units: 100,
        throughput_per_s: 100.0,
        distinct_cpus: 1,
        efficiency_pct: None,
        mean_mhz: None,
        max_mhz,
        energy_j: None,
        cpu_power_mw: None,
        samples: 10,
    }
}

#[test]
fn from_result____work_over_wall____is_throughput() {
    let r = RunResult {
        executor: Executor::Background,
        wall: Duration::from_secs(2),
        work_units: 500,
        aggregate: Aggregate::default(),
        energy_uj: None,
        power: None,
    };
    let s = Summary::from_result(&r);
    assert_eq!(s.work_units, 500);
    assert!((s.throughput_per_s - 250.0).abs() < 1e-6);
}

#[test]
fn from_result____powermetrics_present____fills_placement_and_freq() {
    // On macOS the self-sampled aggregate is empty; powermetrics should populate.
    let power = PowerStats::parse(
        "E-Cluster HW active frequency: 1000 MHz\n\
         E-Cluster HW active residency: 90.00%\n\
         P-Cluster HW active frequency: 3000 MHz\n\
         P-Cluster HW active residency: 10.00%\n\
         CPU Power: 400 mW\n",
    );
    let r = RunResult {
        executor: Executor::Background,
        wall: Duration::from_secs(1),
        work_units: 10,
        aggregate: Aggregate::default(),
        energy_uj: None,
        power: Some(power),
    };
    let s = Summary::from_result(&r);
    assert_eq!(s.max_mhz, Some(3000));
    assert!(s.efficiency_pct.unwrap() > 80.0);
    assert!((s.energy_j.unwrap() - 0.4).abs() < 1e-6); // 400 mW * 1 s
    assert!(s.cpu_power_mw.is_some());
}

#[test]
fn background_not_hotter____background_lower____is_some_true() {
    let s = vec![summ("default", Some(3200)), summ("background", Some(1000))];
    assert_eq!(background_not_hotter(&s), Some(true));
}

#[test]
fn background_not_hotter____background_higher____is_some_false() {
    let s = vec![summ("default", Some(1000)), summ("background", Some(3200))];
    assert_eq!(background_not_hotter(&s), Some(false));
}

#[test]
fn background_not_hotter____missing_frequency____is_none() {
    let s = vec![summ("default", None), summ("background", Some(1000))];
    assert_eq!(background_not_hotter(&s), None);
}

#[test]
fn json____serializes_expected_fields() {
    let s = vec![summ("background", Some(1200))];
    let out = json(&s).unwrap();
    assert!(out.contains("\"executor\""));
    assert!(out.contains("\"throughput_per_s\""));
    assert!(out.contains("\"max_mhz\""));
}

#[test]
fn table____contains_header_and_rows() {
    let s = vec![summ("default", Some(3000)), summ("background", None)];
    let out = table(&s);
    assert!(out.contains("work/s"));
    assert!(out.contains("default"));
    assert!(out.contains("background"));
    // Missing frequency renders as "n/a", not a panic or empty cell.
    assert!(out.contains("n/a"));
}

// --- disk ------------------------------------------------------------------

/// A phase whose merged numbers come out at `mib_s` over one second.
fn phase(mib_s: f64, p95_us: Option<u32>) -> PhaseStats {
    PhaseStats {
        bytes: (mib_s * 1024.0 * 1024.0) as u64,
        reads: 100,
        errors: 0,
        elapsed: Duration::from_secs(1),
        p50_us: p95_us.map(|v| v / 2),
        p95_us,
        p99_us: p95_us,
        bypass: CacheBypass::Direct,
    }
}

fn io_row(executor: Executor, solo: f64, contended: f64, foreground: f64) -> IoSummary {
    let result = IoRunResult {
        executor,
        solo: phase(solo, Some(200)),
        contended: phase(contended, Some(30_000)),
        foreground: phase(foreground, Some(300)),
    };
    IoSummary::from_result(&result, &phase(400.0, None))
}

#[test]
fn io_summary____from_result____is_a_fraction_of_the_baseline() {
    let s = io_row(Executor::Background, 260.0, 2.0, 380.0);
    assert_eq!(s.executor, "background");
    assert!((s.solo_mib_s - 260.0).abs() < 0.5);
    assert!((s.contended_mib_s - 2.0).abs() < 0.5);
    // 380 of a 400 MiB/s baseline.
    assert!((s.foreground_protection_pct.unwrap() - 95.0).abs() < 0.5);
    // Latency is reported for the contended phase, where throttling shows up.
    assert_eq!(s.p95_us, Some(30_000));
    assert_eq!(s.cache_bypass, CacheBypass::Direct);
}

#[test]
fn io_summary____zero_baseline____has_no_protection_figure() {
    let result = IoRunResult {
        executor: Executor::Default,
        solo: phase(100.0, None),
        contended: phase(50.0, None),
        foreground: phase(50.0, None),
    };
    let s = IoSummary::from_result(&result, &PhaseStats::default());
    assert_eq!(s.foreground_protection_pct, None);
}

#[test]
fn background_yields_disk____background_protects_more____is_some_true() {
    let rows = vec![
        io_row(Executor::Default, 400.0, 200.0, 200.0),
        io_row(Executor::Background, 260.0, 2.0, 396.0),
    ];
    assert_eq!(background_yields_disk(&rows), Some(true));
}

#[test]
fn background_yields_disk____background_crowds_the_foreground____is_some_false() {
    let rows = vec![
        io_row(Executor::Default, 400.0, 200.0, 396.0),
        io_row(Executor::Background, 400.0, 380.0, 100.0),
    ];
    assert_eq!(background_yields_disk(&rows), Some(false));
}

#[test]
fn background_yields_disk____default_missing____is_none() {
    let rows = vec![io_row(Executor::Background, 260.0, 2.0, 396.0)];
    assert_eq!(background_yields_disk(&rows), None);
}

#[test]
fn device_saturated____default_left_the_foreground_intact____is_false() {
    // 396 of a 400 MiB/s baseline: nothing was actually contended for.
    let rows = vec![io_row(Executor::Default, 400.0, 396.0, 396.0)];
    assert!(!device_saturated(&rows));
}

#[test]
fn device_saturated____default_halved_the_foreground____is_true() {
    let rows = vec![io_row(Executor::Default, 400.0, 200.0, 200.0)];
    assert!(device_saturated(&rows));
}

#[test]
fn io_table____contains_header_and_rows() {
    let report = IoReport {
        foreground_baseline_mib_s: 400.0,
        cache_bypass: CacheBypass::Direct,
        io_scheduler: Some("bfq".to_owned()),
        rows: vec![io_row(Executor::Background, 260.0, 2.0, 396.0)],
    };
    let out = io_table(&report);
    assert!(out.contains("fg_prot%"));
    assert!(out.contains("background"));

    let json = io_json(&report).unwrap();
    assert!(json.contains("\"foreground_protection_pct\""));
    assert!(json.contains("\"io_scheduler\": \"bfq\""));
    assert!(json.contains("\"cache_bypass\": \"direct\""));
}
