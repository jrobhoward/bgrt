//! Tests for the cpufreq governor probe.
#![allow(non_snake_case)]

use rstest::rstest;

use super::{clamp_can_act, distinct, governors};

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|&n| n.to_owned()).collect()
}

#[test]
fn distinct____one_shared_policy_per_core____collapses_to_one() {
    let raw = owned(&["schedutil\n", "schedutil\n", "schedutil\n"]);
    assert_eq!(distinct(raw), Some(owned(&["schedutil"])));
}

#[test]
fn distinct____mixed_policies____are_sorted() {
    let raw = owned(&["schedutil\n", "ondemand\n", "schedutil\n"]);
    assert_eq!(distinct(raw), Some(owned(&["ondemand", "schedutil"])));
}

#[test]
fn distinct____nothing_read____is_none() {
    assert_eq!(distinct(Vec::new()), None);
    assert_eq!(distinct(owned(&["\n"])), None);
}

#[rstest]
#[case(&["schedutil"], true)]
#[case(&["ondemand"], false)] // the Raspberry Pi image default
#[case(&["performance"], false)]
#[case(&["powersave"], false)] // intel_pstate active mode
#[case(&["ondemand", "schedutil"], true)] // one policy is enough to show an effect
fn clamp_can_act____governors____needs_schedutil(#[case] names: &[&str], #[case] expected: bool) {
    assert_eq!(clamp_can_act(&owned(names)), expected);
}

#[test]
fn governors____any_host____does_not_panic() {
    // Real sysfs: absent off Linux and in most VMs, which must read as `None`.
    if let Some(found) = governors() {
        assert!(!found.is_empty());
    }
}
