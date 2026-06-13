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
