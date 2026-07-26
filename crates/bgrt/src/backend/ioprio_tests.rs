//! Tests for the Linux block-I/O priority mapping.
#![allow(non_snake_case)]

use super::{IOPRIO_CLASS_BE, best_effort_level, ioprio_value};
use crate::qos::QosClass;

// The mapping and the bit-packing are pure, so they are asserted on every
// platform; the syscall itself can only be exercised on Linux (below).

#[test]
fn best_effort_level____background____is_the_lowest_be_level() {
    assert_eq!(best_effort_level(QosClass::Background), Some(7));
}

#[test]
fn best_effort_level____utility____is_between_background_and_normal() {
    // Matches what the kernel would derive from nice(10): (10 + 20) / 5 == 6.
    assert_eq!(best_effort_level(QosClass::Utility), Some(6));
}

#[test]
fn best_effort_level____default____leaves_io_priority_alone() {
    // Writing anything here would mean *raising* a priority, which this crate
    // never does — an unset I/O priority already tracks nice(0).
    assert_eq!(best_effort_level(QosClass::Default), None);
}

#[test]
fn ioprio_value____best_effort_7____packs_the_class_into_the_high_bits() {
    // IOPRIO_PRIO_VALUE(IOPRIO_CLASS_BE, 7) == (2 << 13) | 7 == 16391.
    assert_eq!(ioprio_value(IOPRIO_CLASS_BE, 7), 16391);
}

#[test]
fn ioprio_value____level_zero____is_the_bare_class() {
    assert_eq!(ioprio_value(IOPRIO_CLASS_BE, 0), 2 << 13);
}

// Linux: prove the syscall actually took, by reading the value back.
#[cfg(target_os = "linux")]
mod linux {
    use super::super::{IOPRIO_CLASS_BE, apply};
    use crate::qos::QosClass;
    use crate::test_support::{current_ioprio, ioprio_parts};

    // Each test runs on a dedicated thread so it never deprioritizes the runner.

    #[test]
    fn apply____background____sets_best_effort_level_7() {
        let h = std::thread::spawn(|| {
            apply(QosClass::Background).unwrap();
            current_ioprio()
        });
        let prio = h.join().unwrap();
        assert_eq!(ioprio_parts(prio), (IOPRIO_CLASS_BE, 7));
    }

    #[test]
    fn apply____utility____sets_best_effort_level_6() {
        let h = std::thread::spawn(|| {
            apply(QosClass::Utility).unwrap();
            current_ioprio()
        });
        let prio = h.join().unwrap();
        assert_eq!(ioprio_parts(prio), (IOPRIO_CLASS_BE, 6));
    }

    #[test]
    fn apply____default____leaves_the_thread_untouched() {
        // Reads the value before and after so the assertion holds whatever the
        // runner's inherited I/O priority happens to be.
        let h = std::thread::spawn(|| {
            let before = current_ioprio();
            apply(QosClass::Default).unwrap();
            (before, current_ioprio())
        });
        let (before, after) = h.join().unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn apply____background_then_utility____raises_nothing_below_its_own_class() {
        // Both land in best-effort; the point is that repeated application is
        // well-defined rather than accumulating.
        let h = std::thread::spawn(|| {
            apply(QosClass::Background).unwrap();
            apply(QosClass::Utility).unwrap();
            current_ioprio()
        });
        assert_eq!(ioprio_parts(h.join().unwrap()), (IOPRIO_CLASS_BE, 6));
    }
}
