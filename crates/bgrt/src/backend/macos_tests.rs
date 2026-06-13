//! Tests for the macOS QoS backend.
#![allow(non_snake_case)]

use super::{QOS_CLASS_BACKGROUND, QOS_CLASS_DEFAULT, QOS_CLASS_UTILITY, apply};
use crate::qos::QosClass;
use crate::test_support::current_qos;

// Each test runs on a dedicated thread so it never lowers the test runner's QoS.

#[test]
fn apply____background____sets_background_qos_on_current_thread() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        current_qos()
    });
    assert_eq!(h.join().unwrap(), QOS_CLASS_BACKGROUND);
}

#[test]
fn apply____utility____sets_utility_qos_on_current_thread() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Utility).unwrap();
        current_qos()
    });
    assert_eq!(h.join().unwrap(), QOS_CLASS_UTILITY);
}

#[test]
fn apply____default____sets_default_qos_on_current_thread() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Default).unwrap();
        current_qos()
    });
    assert_eq!(h.join().unwrap(), QOS_CLASS_DEFAULT);
}
