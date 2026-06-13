//! Tests for CPU topology detection.
#![allow(non_snake_case)]

use super::select_efficiency_cores;

#[test]
fn select_efficiency_cores____hybrid____returns_min_capacity_cpus() {
    // E-cores (cap 620) are cpus 0,1; P-cores (cap 1024) are 2,3.
    let caps = vec![(0, 620), (1, 620), (2, 1024), (3, 1024)];
    assert_eq!(select_efficiency_cores(caps), vec![0, 1]);
}

#[test]
fn select_efficiency_cores____homogeneous____is_empty() {
    let caps = vec![(0, 1024), (1, 1024), (2, 1024)];
    assert!(select_efficiency_cores(caps).is_empty());
}

#[test]
fn select_efficiency_cores____empty____is_empty() {
    assert!(select_efficiency_cores(Vec::new()).is_empty());
}

#[test]
fn select_efficiency_cores____three_tiers____returns_only_lowest() {
    // Some big.LITTLE layouts have three capacity tiers; only the lowest counts.
    let caps = vec![(0, 380), (1, 620), (2, 1024)];
    assert_eq!(select_efficiency_cores(caps), vec![0]);
}

#[test]
fn efficiency_cores____result____has_unique_indices() {
    // Hardware-dependent contents (empty on non-hybrid / macOS / Windows); we only
    // assert it returns without panicking and never reports a core twice.
    let cores = super::efficiency_cores();
    let mut unique = cores.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        cores.len(),
        "efficiency core indices must be unique"
    );
}
