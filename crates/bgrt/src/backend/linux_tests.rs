//! Tests for the Linux QoS backend.
#![allow(non_snake_case)]

use super::apply;
use crate::qos::QosClass;

/// Read back the calling thread's nice value (-20..=19).
fn current_nice() -> i32 {
    // `getpriority` can legitimately return -1, so clear errno first to tell a
    // real error from a nice value of -1.
    // SAFETY: writing through the libc errno location; reading the caller's nice.
    unsafe {
        *libc::__errno_location() = 0;
        let v = libc::getpriority(libc::PRIO_PROCESS as _, 0);
        let errno = *libc::__errno_location();
        assert_eq!(errno, 0, "getpriority failed: errno {errno}");
        v
    }
}

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
