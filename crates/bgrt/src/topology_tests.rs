//! Tests for CPU topology detection.
#![allow(non_snake_case)]

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
