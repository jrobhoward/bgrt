//! Tests for the Windows QoS backend.
//!
//! EcoQoS power-throttling state is set-only (no documented read-back), so these
//! assert the observable `SetThreadPriority` effect and that `apply` succeeds.
#![allow(non_snake_case)]

use windows_sys::Win32::System::Threading::{
    GetCurrentThread, GetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL,
};

use super::apply;
use crate::qos::QosClass;

fn current_priority() -> i32 {
    // SAFETY: reads the priority of the current-thread pseudo-handle.
    unsafe { GetThreadPriority(GetCurrentThread()) }
}

// Each test runs on a dedicated thread so it never lowers the test runner.

#[test]
fn apply____background____sets_below_normal_priority() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        current_priority()
    });
    assert_eq!(h.join().unwrap(), THREAD_PRIORITY_BELOW_NORMAL);
}

#[test]
fn apply____utility____sets_normal_priority() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Utility).unwrap();
        current_priority()
    });
    assert_eq!(h.join().unwrap(), THREAD_PRIORITY_NORMAL);
}

#[test]
fn apply____default____sets_normal_priority() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Default).unwrap();
        current_priority()
    });
    assert_eq!(h.join().unwrap(), THREAD_PRIORITY_NORMAL);
}
