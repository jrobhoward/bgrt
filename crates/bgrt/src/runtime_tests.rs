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
fn current_thread_runtime____spawn____runs_task_to_completion() {
    let rt = RuntimeBuilder::new().current_thread(true).build().unwrap();
    let handle = rt.spawn(async { 20 + 22 });
    let out = rt.block_on(async move { handle.await.unwrap() });
    assert_eq!(out, 42);
}

#[test]
fn current_thread_runtime____spawn____runs_off_the_calling_thread() {
    // The whole point of the mode: tasks run on a bgrt-owned thread, not the
    // caller's, so that classifying it is legitimate.
    let rt = RuntimeBuilder::new().current_thread(true).build().unwrap();
    let caller = std::thread::current().id();
    let handle = rt.spawn(async { std::thread::current().id() });
    let task = rt.block_on(async move { handle.await.unwrap() });
    assert_ne!(task, caller);
}

#[test]
fn current_thread_runtime____many_tasks____all_share_one_thread() {
    let rt = RuntimeBuilder::new().current_thread(true).build().unwrap();
    let handles: Vec<_> = (0..8)
        .map(|_| rt.spawn(async { std::thread::current().id() }))
        .collect();
    let ids = rt.block_on(async move {
        let mut ids = Vec::new();
        for h in handles {
            ids.push(h.await.unwrap());
        }
        ids
    });
    assert!(
        ids.windows(2).all(|w| w[0] == w[1]),
        "current-thread tasks landed on more than one thread: {ids:?}"
    );
}

#[test]
fn current_thread_runtime____thread_name____comes_from_the_builder() {
    let rt = RuntimeBuilder::new()
        .current_thread(true)
        .thread_name("lowprio-driver")
        .build()
        .unwrap();
    let handle = rt.spawn(async { std::thread::current().name().map(str::to_owned) });
    let name = rt.block_on(async move { handle.await.unwrap() });
    assert_eq!(name.as_deref(), Some("lowprio-driver"));
}

#[test]
fn current_thread_runtime____timer____is_driven_by_the_owned_thread() {
    // `Handle::block_on` cannot drive a current-thread runtime's timer itself;
    // this passes only because the owned thread is parked in `Runtime::block_on`.
    let rt = RuntimeBuilder::new().current_thread(true).build().unwrap();
    let handle = rt.spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        42
    });
    let out = rt.block_on(async move { handle.await.unwrap() });
    assert_eq!(out, 42);
}

#[test]
fn current_thread_runtime____spawn_blocking____runs_closure_to_completion() {
    let rt = RuntimeBuilder::new().current_thread(true).build().unwrap();
    let handle = rt.spawn_blocking(|| 6 * 7);
    let out = rt.block_on(async move { handle.await.unwrap() });
    assert_eq!(out, 42);
}

#[test]
fn current_thread_runtime____drop____joins_the_driver_thread_without_hanging() {
    let started = std::time::Instant::now();
    {
        let rt = RuntimeBuilder::new().current_thread(true).build().unwrap();
        rt.spawn(async { std::future::pending::<()>().await });
    }
    // A pending async task must not keep the driver thread alive; only blocking
    // work does, and there is none here.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "dropping a current-thread runtime hung for {:?}",
        started.elapsed()
    );
}

#[test]
fn current_thread_runtime____shutdown_timeout____returns_without_waiting_for_blocking_work() {
    let rt = RuntimeBuilder::new().current_thread(true).build().unwrap();
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
fn current_thread_runtime____shutdown_background____returns_immediately() {
    let rt = RuntimeBuilder::new().current_thread(true).build().unwrap();
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

    // The regression test the current-thread mode exists for: a plain tokio
    // current-thread runtime would run this task unclassified, on the caller.
    #[test]
    fn current_thread_runtime____spawn____driver_thread_is_classified() {
        let rt = RuntimeBuilder::new()
            .qos(QosClass::Background)
            .current_thread(true)
            .build()
            .unwrap();
        let handle = rt.spawn(async { current_qos() });
        let qos = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(qos, QOS_CLASS_BACKGROUND);
    }

    #[test]
    fn current_thread_runtime____spawn_blocking____pool_thread_is_classified() {
        let rt = RuntimeBuilder::new()
            .qos(QosClass::Background)
            .current_thread(true)
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
    use crate::test_support::{current_ioprio, current_nice, ioprio_parts};
    use crate::{QosClass, RuntimeBuilder};

    const IOPRIO_CLASS_BE: i32 = 2;

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

    // The regression test the current-thread mode exists for: a plain tokio
    // current-thread runtime would run this task unclassified, on the caller.
    #[test]
    fn current_thread_runtime____spawn____driver_thread_is_nice_19() {
        let rt = RuntimeBuilder::new()
            .qos(QosClass::Background)
            .current_thread(true)
            .build()
            .unwrap();
        let handle = rt.spawn(async { current_nice() });
        let nice = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(nice, 19);
    }

    // The disk half of the class, at the level users actually reach it: a task
    // on a runtime worker, not a direct `apply` call.
    #[test]
    fn background_runtime____spawn____worker_thread_is_io_best_effort_7() {
        let rt = RuntimeBuilder::new()
            .qos(QosClass::Background)
            .worker_threads(1)
            .build()
            .unwrap();
        let handle = rt.spawn(async { current_ioprio() });
        let prio = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(ioprio_parts(prio), (IOPRIO_CLASS_BE, 7));
    }

    // `spawn_blocking` is where CPU- and disk-heavy work actually lands, so the
    // blocking pool carrying the I/O class matters more than the workers do.
    #[test]
    fn background_runtime____spawn_blocking____pool_thread_is_io_best_effort_7() {
        let rt = RuntimeBuilder::new()
            .qos(QosClass::Background)
            .build()
            .unwrap();
        let handle = rt.spawn_blocking(current_ioprio);
        let prio = rt.block_on(async move { handle.await.unwrap() });
        assert_eq!(ioprio_parts(prio), (IOPRIO_CLASS_BE, 7));
    }
}
