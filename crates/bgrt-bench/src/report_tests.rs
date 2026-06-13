//! Tests for report formatting and the comparison check.
#![allow(non_snake_case)]

use std::time::Duration;

use bgrt::telemetry::Aggregate;

use super::{Summary, background_not_hotter, json, table};
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
