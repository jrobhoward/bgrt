//! Tests for the powermetrics parser and derived statistics.
#![allow(non_snake_case)]

use std::time::Duration;

use super::PowerStats;

const SAMPLE: &str = "\
*** Sampled system activity ***

**** Processor usage ****

E-Cluster HW active frequency: 1024 MHz
E-Cluster HW active residency:  80.00%
E-Cluster idle residency:  20.00%
P-Cluster HW active frequency: 3000 MHz
P-Cluster HW active residency:  10.00%

**** Power ****
CPU Power: 500 mW
GPU Power: 0 mW
";

const EPS: f64 = 1e-6;

#[test]
fn parse____sample____extracts_power_freq_residency() {
    let s = PowerStats::parse(SAMPLE);
    assert!((s.cpu_power_mw().unwrap() - 500.0).abs() < EPS);
    assert_eq!(s.max_freq_mhz(), Some(3000));
}

#[test]
fn efficiency_pct____from_residency____is_e_over_total() {
    let s = PowerStats::parse(SAMPLE);
    // 80 / (80 + 10) * 100
    assert!((s.efficiency_pct().unwrap() - (80.0 / 90.0 * 100.0)).abs() < EPS);
}

#[test]
fn mean_freq_mhz____is_residency_weighted() {
    let s = PowerStats::parse(SAMPLE);
    // (1024*80 + 3000*10) / 90
    let expected = (1024.0 * 80.0 + 3000.0 * 10.0) / 90.0;
    assert!((s.mean_freq_mhz().unwrap() - expected).abs() < EPS);
}

#[test]
fn energy_j____is_power_times_wall() {
    let s = PowerStats::parse(SAMPLE);
    // 500 mW = 0.5 W; over 2 s = 1.0 J
    assert!((s.energy_j(Duration::from_secs(2)).unwrap() - 1.0).abs() < EPS);
}

#[test]
fn parse____empty____is_all_none() {
    let s = PowerStats::parse("no useful lines here");
    assert_eq!(s.cpu_power_mw(), None);
    assert_eq!(s.efficiency_pct(), None);
    assert_eq!(s.mean_freq_mhz(), None);
    assert_eq!(s.max_freq_mhz(), None);
}

#[test]
fn parse____averages_across_multiple_samples() {
    let two = format!("CPU Power: 100 mW\n{}", "CPU Power: 300 mW\n");
    let s = PowerStats::parse(&two);
    assert!((s.cpu_power_mw().unwrap() - 200.0).abs() < EPS);
}
