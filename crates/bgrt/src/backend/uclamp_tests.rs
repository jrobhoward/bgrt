//! Tests for the Linux uclamp frequency clamp.
#![allow(non_snake_case)]

use rstest::rstest;

use super::{clamp_current_thread, governor_honours_clamp, note_clamp_governor, util_max_for};
use crate::qos::QosClass;
use crate::test_support::current_uclamp_max;

// Each clamp test runs on a dedicated thread so it never clamps the test runner.

#[test]
fn util_max_for____background____clamps_to_a_fifth() {
    assert_eq!(util_max_for(QosClass::Background), Some(1024 / 5));
}

#[test]
fn util_max_for____utility_and_default____unclamped() {
    assert_eq!(util_max_for(QosClass::Utility), None);
    assert_eq!(util_max_for(QosClass::Default), None);
}

#[test]
fn clamp_current_thread____background____lowers_uclamp_max() {
    let h = std::thread::spawn(|| {
        let before = current_uclamp_max();
        clamp_current_thread(QosClass::Background).unwrap();
        (before, current_uclamp_max())
    });
    let (before, after) = h.join().unwrap();
    match (before, after) {
        // Kernel without CONFIG_UCLAMP_TASK: the value is unreadable; nothing to
        // assert (the clamp is a documented no-op there).
        (None, _) | (_, None) => {}
        // Applied (needs kernel >= 5.8 for SCHED_FLAG_KEEP_ALL): the exact cap.
        (Some(b), Some(a)) if a < b => assert_eq!(a, 1024 / 5),
        // Unchanged: KEEP_ALL unsupported (kernel 5.3..5.8); degraded no-op.
        (Some(b), Some(a)) => assert_eq!(a, b),
    }
}

#[test]
fn clamp_current_thread____utility____is_a_noop() {
    let h = std::thread::spawn(|| {
        let before = current_uclamp_max();
        // Unclamped classes must not touch the thread's util_max at all.
        clamp_current_thread(QosClass::Utility).unwrap();
        (before, current_uclamp_max())
    });
    let (before, after) = h.join().unwrap();
    assert_eq!(before, after);
}

#[rstest]
#[case("schedutil", true)]
#[case("schedutil\n", true)] // as read from sysfs
#[case("ondemand", false)] // the Raspberry Pi image default
#[case("performance", false)]
#[case("powersave", false)] // intel_pstate active mode
#[case("conservative", false)]
fn governor_honours_clamp____governor____only_schedutil(
    #[case] governor: &str,
    #[case] expected: bool,
) {
    assert_eq!(governor_honours_clamp(governor), expected);
}

#[rstest]
#[case(QosClass::Background)]
#[case(QosClass::Utility)]
#[case(QosClass::Default)]
fn note_clamp_governor____any_host____does_not_panic(#[case] class: QosClass) {
    // Reads real sysfs, which may be absent (VMs, containers): must stay silent.
    note_clamp_governor(class);
}
