//! Tests for the energy-classified runtime.
#![allow(non_snake_case)]

use crate::{QosClass, RuntimeBuilder};

#[test]
fn runtime____spawn____runs_task_to_completion() {
    let rt = RuntimeBuilder::new().build().unwrap();
    let handle = rt.spawn(async { 20 + 22 });
    let out = rt.block_on(async move { handle.await.unwrap() });
    assert_eq!(out, 42);
}

#[test]
fn runtime____spawn_blocking____runs_closure_to_completion() {
    let rt = RuntimeBuilder::new().build().unwrap();
    let handle = rt.spawn_blocking(|| 6 * 7);
    let out = rt.block_on(async move { handle.await.unwrap() });
    assert_eq!(out, 42);
}

#[test]
fn builder____defaults____are_background_class() {
    let rt = RuntimeBuilder::new().build().unwrap();
    assert_eq!(rt.qos(), QosClass::Background);
}

#[test]
fn runtime____shutdown_timeout____returns_without_waiting_for_blocking_work() {
    let rt = RuntimeBuilder::new().build().unwrap();
    // A blocking task that outlives the timeout: shutdown must not wait for it.
    rt.spawn_blocking(|| std::thread::sleep(std::time::Duration::from_secs(30)));
    let started = std::time::Instant::now();
    rt.shutdown_timeout(std::time::Duration::from_millis(50));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "shutdown_timeout waited {:?}, far past its 50ms bound",
        started.elapsed()
    );
}

#[test]
fn runtime____shutdown_background____returns_immediately() {
    let rt = RuntimeBuilder::new().build().unwrap();
    rt.spawn_blocking(|| std::thread::sleep(std::time::Duration::from_secs(30)));
    let started = std::time::Instant::now();
    rt.shutdown_background();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "shutdown_background waited {:?}",
        started.elapsed()
    );
}

#[test]
fn builder____zero_workers____clamps_instead_of_panicking() {
    // tokio panics on worker_threads(0); the builder must clamp it.
    let rt = RuntimeBuilder::new().worker_threads(0).build().unwrap();
    assert_eq!(rt.block_on(async { 42 }), 42);
}

// macOS: prove the on_thread_start hook actually classified the runtime threads.
#[cfg(target_os = "macos")]
mod macos {
    use crate::test_support::{QOS_CLASS_BACKGROUND, current_qos};
    use crate::{QosClass, RuntimeBuilder};

    #[test]
    fn background_runtime____spawn____worker_thread_is_classified() {
        let rt = RuntimeBuilder::new()
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
        let rt = RuntimeBuilder::new()
            .qos(QosClass::Background)
            .build()
            .unwrap();
        let handle = rt.spawn_blocking(current_qos);
        let qos = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(qos, QOS_CLASS_BACKGROUND);
    }
}

// Linux: worker threads should carry nice 19 (runs on CI / Linux hardware).
#[cfg(target_os = "linux")]
mod linux {
    use crate::test_support::current_nice;
    use crate::{QosClass, RuntimeBuilder};

    #[test]
    fn background_runtime____spawn____worker_thread_is_nice_19() {
        let rt = RuntimeBuilder::new()
            .qos(QosClass::Background)
            .worker_threads(1)
            .build()
            .unwrap();
        let handle = rt.spawn(async { current_nice() });
        let nice = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(nice, 19);
    }
}
