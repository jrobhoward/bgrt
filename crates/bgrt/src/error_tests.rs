//! Tests for the error types.
#![allow(non_snake_case)]

use std::error::Error as _;

use super::Error;

fn backend(syscall: &'static str, errno: i32) -> Error {
    Error::Backend {
        syscall,
        source: std::io::Error::from_raw_os_error(errno),
    }
}

#[test]
fn backend_error____display____names_the_failing_syscall() {
    let e = backend("setpriority", 1);
    assert_eq!(e.to_string(), "failed to apply energy qos: setpriority");
}

#[test]
fn backend_error____source____carries_the_os_error() {
    let e = backend("setpriority", 1);
    let source = e.source().expect("backend errors must expose a source");
    // The OS message is the source's job, not the top-level Display's.
    assert!(!source.to_string().is_empty());
}

#[test]
fn backend_error____raw_os_error____round_trips_the_errno() {
    // 1 is EPERM on every platform bgrt supports.
    assert_eq!(backend("sched_setattr(uclamp)", 1).raw_os_error(), Some(1));
}

#[cfg(feature = "tokio")]
#[test]
fn runtime_error____raw_os_error____round_trips_the_errno() {
    let e = Error::Runtime(std::io::Error::from_raw_os_error(24));
    assert_eq!(e.raw_os_error(), Some(24));
    assert_eq!(e.to_string(), "failed to build runtime");
}

#[cfg(feature = "rayon")]
#[test]
fn thread_pool_error____raw_os_error____is_none_for_non_os_causes() {
    let e = Error::ThreadPool(Box::new(std::io::Error::other("boom")));
    assert_eq!(e.raw_os_error(), None);
    assert_eq!(e.to_string(), "failed to build thread pool");
    assert!(e.source().is_some());
}
