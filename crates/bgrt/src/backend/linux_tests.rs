//! Tests for the Linux QoS backend.
#![allow(non_snake_case)]

use super::apply;
use crate::qos::QosClass;
use crate::test_support::current_nice;

// Each test runs on a dedicated thread so it never nices-down the test runner.

#[test]
fn apply____background____sets_nice_19() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        current_nice()
    });
    assert_eq!(h.join().unwrap(), 19);
}

#[test]
fn apply____utility____sets_nice_10() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Utility).unwrap();
        current_nice()
    });
    assert_eq!(h.join().unwrap(), 10);
}

#[test]
fn apply____default____sets_nice_0() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Default).unwrap();
        current_nice()
    });
    assert_eq!(h.join().unwrap(), 0);
}

// `nice` is one-way for an unprivileged thread, so a class cannot be undone.
// These assert the *graceful* half of that: `apply` reports success and leaves
// the thread where it is, rather than surfacing the kernel's EACCES as an error
// the caller can do nothing about. See the backend module docs.

#[test]
fn apply____default_after_background____succeeds_and_keeps_nice_19() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        let result = apply(QosClass::Default);
        (result, current_nice())
    });
    let (result, nice) = h.join().unwrap();
    assert!(
        result.is_ok(),
        "Default after Background must degrade to a no-op, not fail: {result:?}"
    );
    assert_eq!(nice, 19, "the thread keeps the niceness it already had");
}

#[test]
fn apply____background_after_utility____still_lowers_to_nice_19() {
    // Lowering priority further is always permitted; only raising is refused.
    let h = std::thread::spawn(|| {
        apply(QosClass::Utility).unwrap();
        apply(QosClass::Background).unwrap();
        current_nice()
    });
    assert_eq!(h.join().unwrap(), 19);
}

#[test]
fn apply____utility_after_background____succeeds_and_keeps_nice_19() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        let result = apply(QosClass::Utility);
        (result, current_nice())
    });
    let (result, nice) = h.join().unwrap();
    assert!(
        result.is_ok(),
        "Utility after Background must degrade to a no-op, not fail: {result:?}"
    );
    assert_eq!(nice, 19);
}
