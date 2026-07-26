//! Tests for the macOS QoS backend.
#![allow(non_snake_case)]

use super::{QOS_CLASS_BACKGROUND, QOS_CLASS_DEFAULT, QOS_CLASS_UTILITY, apply};
use crate::qos::QosClass;
use crate::test_support::{IOPOL_DEFAULT, current_disk_iopolicy, current_qos};

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

/// The disk half of a class comes from the QoS band on macOS, and **must** —
/// this backend deliberately never calls `setiopolicy_np`.
///
/// Measured: setting an explicit thread-scope I/O policy permanently opts the
/// thread out of QoS. `pthread_get_qos_class_np` then reports
/// `QOS_CLASS_UNSPECIFIED` (0), and calling `pthread_set_qos_class_self_np`
/// afterwards does *not* restore it — neither ordering yields both. Trading
/// E-core confinement for an assertable I/O policy would be a catastrophic deal,
/// so we take Darwin's bundled behaviour and leave the override alone.
///
/// This guards against re-adding it: the QoS assertion is what breaks first.
#[test]
fn apply____background____keeps_qos_and_leaves_the_io_override_unset() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        (current_qos(), current_disk_iopolicy())
    });
    let (qos, iopolicy) = h.join().unwrap();
    assert_eq!(
        qos, QOS_CLASS_BACKGROUND,
        "QoS class was lost — did something set a thread scheduling override?"
    );
    assert_eq!(
        iopolicy, IOPOL_DEFAULT,
        "an explicit I/O policy override would opt this thread out of QoS"
    );
}
