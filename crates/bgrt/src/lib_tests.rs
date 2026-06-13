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
