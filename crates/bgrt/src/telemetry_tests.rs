//! Tests for the telemetry primitives.
#![allow(non_snake_case)]

use super::{Aggregate, CoreType, Sample, classify, energy_delta, energy_uj, sample};

const EPS: f64 = 1e-9;

#[test]
fn classify____empty_efficiency_set____is_unknown() {
    assert_eq!(classify(0, &[]), CoreType::Unknown);
}

#[test]
fn classify____cpu_in_efficiency_set____is_efficiency() {
    assert_eq!(classify(2, &[2, 3]), CoreType::Efficiency);
}

#[test]
fn classify____cpu_outside_efficiency_set____is_performance() {
    assert_eq!(classify(0, &[2, 3]), CoreType::Performance);
}

#[test]
fn energy_delta____increasing_counter____is_difference() {
    assert_eq!(energy_delta(Some(100), Some(250)), Some(150));
}

#[test]
fn energy_delta____wrapped_counter____is_none() {
    assert_eq!(energy_delta(Some(250), Some(100)), None);
}

#[test]
fn energy_delta____missing_endpoint____is_none() {
    assert_eq!(energy_delta(None, Some(100)), None);
    assert_eq!(energy_delta(Some(100), None), None);
}

#[test]
fn aggregate____mixed_samples____reports_residency_and_freq() {
    let mut agg = Aggregate::default();
    agg.record(Sample {
        cpu: Some(0),
        core_type: CoreType::Efficiency,
        freq_mhz: Some(1000),
    });
    agg.record(Sample {
        cpu: Some(1),
        core_type: CoreType::Performance,
        freq_mhz: Some(3000),
    });
    agg.record(Sample {
        cpu: Some(0),
        core_type: CoreType::Efficiency,
        freq_mhz: None,
    });

    assert_eq!(agg.samples(), 3);
    assert_eq!(agg.distinct_cpus(), 2);
    assert!((agg.efficiency_fraction().unwrap() - 2.0 / 3.0).abs() < EPS);
    assert!((agg.mean_freq_mhz().unwrap() - 2000.0).abs() < EPS);
    assert_eq!(agg.max_freq_mhz(), Some(3000));
}

#[test]
fn aggregate____only_unknown_samples____fractions_are_none() {
    let mut agg = Aggregate::default();
    agg.record(Sample {
        cpu: None,
        core_type: CoreType::Unknown,
        freq_mhz: None,
    });
    assert_eq!(agg.efficiency_fraction(), None);
    assert_eq!(agg.mean_freq_mhz(), None);
    assert_eq!(agg.max_freq_mhz(), None);
}

#[test]
fn sample____returns_without_panicking() {
    // Contents are platform/privilege dependent; just exercise the path.
    let s = sample();
    let _ = (s.cpu, s.core_type, s.freq_mhz);
}

#[test]
fn energy_uj____returns_without_panicking() {
    let _ = energy_uj();
}
