//! Tests for the energy-classified runtime.
#![allow(non_snake_case)]

use crate::{Builder, QosClass};

#[test]
fn runtime____spawn____runs_task_to_completion() {
    let rt = Builder::new().build().unwrap();
    let handle = rt.spawn(async { 20 + 22 });
    let out = rt.block_on(async move { handle.await.unwrap() });
    assert_eq!(out, 42);
}

#[test]
fn runtime____spawn_blocking____runs_closure_to_completion() {
    let rt = Builder::new().build().unwrap();
    let handle = rt.spawn_blocking(|| 6 * 7);
    let out = rt.block_on(async move { handle.await.unwrap() });
    assert_eq!(out, 42);
}

#[test]
fn builder____defaults____are_background_class() {
    let rt = Builder::new().build().unwrap();
    assert_eq!(rt.qos(), QosClass::Background);
}

#[test]
fn builder____zero_workers____clamps_instead_of_panicking() {
    // tokio panics on worker_threads(0); the builder must clamp it.
    let rt = Builder::new().worker_threads(0).build().unwrap();
    assert_eq!(rt.block_on(async { 42 }), 42);
}

// macOS: prove the on_thread_start hook actually classified the runtime threads.
#[cfg(target_os = "macos")]
mod macos {
    use crate::{Builder, QosClass};

    const QOS_CLASS_BACKGROUND: u32 = 0x09;

    unsafe extern "C" {
        fn pthread_get_qos_class_np(
            thread: libc::pthread_t,
            qos_class: *mut u32,
            relative_priority: *mut i32,
        ) -> i32;
    }

    fn current_qos() -> u32 {
        let mut qos = 0u32;
        let mut rel = 0i32;
        // SAFETY: reads the calling thread's QoS into stack-local out-parameters.
        let rc = unsafe { pthread_get_qos_class_np(libc::pthread_self(), &mut qos, &mut rel) };
        assert_eq!(rc, 0);
        qos
    }

    #[test]
    fn background_runtime____spawn____worker_thread_is_classified() {
        let rt = Builder::new()
            .qos(QosClass::Background)
            .worker_threads(1)
            .build()
            .unwrap();
        let handle = rt.spawn(async { current_qos() });
        let qos = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(qos, QOS_CLASS_BACKGROUND);
    }

    #[test]
    fn background_runtime____spawn_blocking____pool_thread_is_classified() {
        let rt = Builder::new().qos(QosClass::Background).build().unwrap();
        let handle = rt.spawn_blocking(current_qos);
        let qos = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(qos, QOS_CLASS_BACKGROUND);
    }
}

// Linux: worker threads should carry nice 19 (runs on CI / Linux hardware).
#[cfg(target_os = "linux")]
mod linux {
    use crate::{Builder, QosClass};

    fn current_nice() -> i32 {
        // SAFETY: clears errno then reads the calling thread's nice value.
        unsafe {
            *libc::__errno_location() = 0;
            libc::getpriority(libc::PRIO_PROCESS as _, 0)
        }
    }

    #[test]
    fn background_runtime____spawn____worker_thread_is_nice_19() {
        let rt = Builder::new()
            .qos(QosClass::Background)
            .worker_threads(1)
            .build()
            .unwrap();
        let handle = rt.spawn(async { current_nice() });
        let nice = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(nice, 19);
    }
}
