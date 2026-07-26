//! Tests for CPU topology detection.
#![allow(non_snake_case)]

use super::{parse_cpulist, select_efficiency_cores};

// The Intel hybrid PMU path (`cpu_atom/cpus`) is a cpulist string, so the whole
// decision is testable off Linux — which matters, because the sysfs read itself
// only ever executes on hybrid x86 hardware nobody here has.

#[test]
fn parse_cpulist____single_range____expands_inclusively() {
    // Real i9-12900K value: cpu_core/cpus reads "0-15", cpu_atom/cpus "16-23".
    assert_eq!(
        parse_cpulist("16-23\n"),
        vec![16, 17, 18, 19, 20, 21, 22, 23]
    );
}

#[test]
fn parse_cpulist____comma_separated_singles____keeps_each() {
    assert_eq!(parse_cpulist("0,2,4"), vec![0, 2, 4]);
}

#[test]
fn parse_cpulist____mixed_ranges_and_singles____merges_sorted_and_deduped() {
    assert_eq!(parse_cpulist("8-11,0,2-3,8"), vec![0, 2, 3, 8, 9, 10, 11]);
}

#[test]
fn parse_cpulist____single_cpu____is_one_entry() {
    assert_eq!(parse_cpulist("7"), vec![7]);
}

#[test]
fn parse_cpulist____empty_or_blank____is_empty() {
    assert!(parse_cpulist("").is_empty());
    assert!(parse_cpulist("\n").is_empty());
    assert!(parse_cpulist(",,").is_empty());
}

#[test]
fn parse_cpulist____malformed_fields____are_skipped_not_fatal() {
    // A partial answer beats none: the good fields survive a corrupt neighbour.
    assert_eq!(parse_cpulist("0-1,junk,4,x-y"), vec![0, 1, 4]);
}

#[test]
fn parse_cpulist____reversed_range____is_dropped() {
    // "5-2" is corruption, not an empty set; it must not loop or panic.
    assert_eq!(parse_cpulist("5-2,9"), vec![9]);
}

#[test]
fn parse_cpulist____absurd_range____does_not_allocate_unboundedly() {
    // Guards against a corrupt file turning into a multi-gigabyte Vec.
    assert!(parse_cpulist("0-99999999").is_empty());
}

#[test]
fn parse_cpulist____out_of_range_singles____are_filtered() {
    // pin_current_thread cannot address beyond CPU_SETSIZE, so neither do we.
    assert_eq!(parse_cpulist("3,4096"), vec![3]);
}

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

// Windows: the CPU Sets read is the half of E/P detection that unit tests of the
// pure selector cannot reach, and CI is the only place it ever executes.
#[cfg(target_os = "windows")]
mod windows {
    use super::super::cpu_set_efficiency_classes;

    #[test]
    fn cpu_set_efficiency_classes____any_windows_machine____reports_each_cpu_once_in_group_range() {
        let classes = cpu_set_efficiency_classes();
        assert!(
            !classes.is_empty(),
            "GetSystemCpuSetInformation returned no CPU sets in group 0"
        );

        let mut indices: Vec<usize> = classes.iter().map(|&(idx, _)| idx).collect();
        indices.sort_unstable();
        let unique = {
            let mut u = indices.clone();
            u.dedup();
            u
        };
        assert_eq!(
            unique, indices,
            "a logical processor was reported more than once: {indices:?}"
        );

        // The indices must be *group-relative* — that is what makes them
        // comparable with `GetCurrentProcessorNumber` in telemetry. A group holds
        // at most 64 processors, so a global processor number (the plausible bug)
        // would show up here as an out-of-range index.
        assert!(
            indices.iter().all(|&i| i < 64),
            "CPU set index outside processor group 0: {indices:?}"
        );
    }

    #[test]
    fn efficiency_cores____windows____are_a_strict_subset_of_all_cpus() {
        // Contents are hardware-dependent: empty on a homogeneous runner, a
        // proper subset on a hybrid one. Never *all* CPUs — that would mean the
        // homogeneous case leaked through as "everything is an E-core".
        let total = cpu_set_efficiency_classes().len();
        let e_cores = super::super::efficiency_cores();
        assert!(
            e_cores.len() < total.max(1),
            "{} of {total} CPUs classified as efficiency cores",
            e_cores.len()
        );
    }
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
