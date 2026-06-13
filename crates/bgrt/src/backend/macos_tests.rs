//! Tests for the macOS QoS backend.
#![allow(non_snake_case)]

use super::{QOS_CLASS_BACKGROUND, QOS_CLASS_DEFAULT, QOS_CLASS_UTILITY, apply};
use crate::qos::QosClass;

unsafe extern "C" {
    fn pthread_get_qos_class_np(
        thread: libc::pthread_t,
        qos_class: *mut u32,
        relative_priority: *mut i32,
    ) -> i32;
}

/// Read back the calling thread's effective QoS class.
fn current_qos() -> u32 {
    let mut qos: u32 = 0;
    let mut rel: i32 = 0;
    // SAFETY: reads the calling thread's QoS into stack-local out-parameters.
    let rc = unsafe { pthread_get_qos_class_np(libc::pthread_self(), &mut qos, &mut rel) };
    assert_eq!(rc, 0, "pthread_get_qos_class_np failed: {rc}");
    qos
}

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
