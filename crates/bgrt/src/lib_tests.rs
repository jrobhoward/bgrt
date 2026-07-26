//! Tests for the crate root.
//!
//! Test names follow the `subject____condition____result` convention (four
//! underscores), so this module opts out of `non_snake_case`.
#![allow(non_snake_case)]

use super::{QosClass, apply};

#[test]
fn apply____background_class____returns_ok() {
    assert!(apply(QosClass::Background).is_ok());
}

#[test]
fn apply____utility_class____returns_ok() {
    assert!(apply(QosClass::Utility).is_ok());
}

#[test]
fn apply____default_class____returns_ok() {
    assert!(apply(QosClass::Default).is_ok());
}

#[test]
fn qos_class____default_impl____is_default_variant() {
    assert_eq!(QosClass::default(), QosClass::Default);
}

// The re-exports exist so callers can name the wrapped types without risking a
// second, incompatible copy in the dependency graph. These are compile-level
// assertions: each binding only type-checks if the re-exported path denotes the
// very type the `bgrt` API hands back. A mismatch is the bug they guard against.

#[cfg(feature = "tokio")]
#[test]
fn tokio_reexport____runtime_handle____is_the_same_type() {
    let rt = crate::RuntimeBuilder::new().build().unwrap();
    let _handle: &crate::tokio::runtime::Handle = rt.handle();
}

#[cfg(feature = "rayon")]
#[test]
fn rayon_reexport____pool_deref_target____is_the_same_type() {
    let pool = crate::RayonBuilder::new().num_threads(1).build().unwrap();
    let _inner: &crate::rayon::ThreadPool = &pool;
}
