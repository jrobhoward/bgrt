//! Tests for the Windows QoS backend.
//!
//! EcoQoS power-throttling state is set-only (no documented read-back), so these
//! assert the observable `SetThreadPriority` effect and that `apply` succeeds.
//! Background-processing mode has no read-back either, but *is* observable
//! indirectly: memory priority is a documented side effect with a getter, and
//! the mode's own "already in that state" error codes prove the transitions.
#![allow(non_snake_case)]

use core::ffi::c_void;

use windows_sys::Win32::System::Threading::{
    GetCurrentThread, GetThreadInformation, MEMORY_PRIORITY_INFORMATION, MEMORY_PRIORITY_NORMAL,
    THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL, ThreadMemoryPriority,
};

use super::{apply, set_background_mode};
use crate::qos::QosClass;
use crate::test_support::current_thread_priority as current_priority;

/// Read the calling thread's memory priority. Background mode lowers this as a
/// side effect, so it doubles as evidence the mode was entered.
fn current_memory_priority() -> u32 {
    let mut info = MEMORY_PRIORITY_INFORMATION { MemoryPriority: 0 };
    // SAFETY: writes a correctly-sized memory-priority struct through the
    // current-thread pseudo-handle.
    let rc = unsafe {
        GetThreadInformation(
            GetCurrentThread(),
            ThreadMemoryPriority,
            (&raw mut info).cast::<c_void>(),
            size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    };
    assert_ne!(rc, 0, "GetThreadInformation(ThreadMemoryPriority) failed");
    info.MemoryPriority
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

/// The tests above each start on a fresh thread, so they only ever *set* EcoQoS.
/// This one reaches `set_eco_qos(false)` — the clear path, which is the only way
/// a `Default` classification undoes an earlier `Background` one.
#[test]
fn apply____background_then_default____clears_throttling_and_restores_priority() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        let throttled = current_priority();
        apply(QosClass::Default).unwrap();
        (throttled, current_priority())
    });
    let (throttled, restored) = h.join().unwrap();
    assert_eq!(throttled, THREAD_PRIORITY_BELOW_NORMAL);
    assert_eq!(restored, THREAD_PRIORITY_NORMAL);
}

// --- background processing mode (the block-I/O lever) ---------------------

/// Windows exposes no getter for block-I/O priority, so the mode's *other*
/// documented side effect stands in for it: entering background mode lowers
/// memory priority. This asserts the raw mode really takes effect — without it,
/// the restore test below would pass just as happily if the mode were never
/// entered at all, since normal is also the default.
///
/// Deliberately compares against normal rather than asserting the exact value:
/// what matters is that the mode demonstrably did something, not which low value
/// Windows picked.
#[test]
fn set_background_mode____entered____lowers_memory_priority_below_normal() {
    let h = std::thread::spawn(|| {
        let before = current_memory_priority();
        set_background_mode(true).unwrap();
        (before, current_memory_priority())
    });
    let (before, after) = h.join().unwrap();
    assert_eq!(before, MEMORY_PRIORITY_NORMAL, "unexpected starting state");
    assert!(
        after < MEMORY_PRIORITY_NORMAL,
        "background mode left memory priority at {after}; it should have lowered it"
    );
}

/// And this proves the backend undoes that side effect: paired with the test
/// above, the two together show the mode was entered *and* memory priority was
/// put back.
#[test]
fn apply____background____restores_memory_priority_after_entering_background_mode() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        current_memory_priority()
    });
    assert_eq!(h.join().unwrap(), MEMORY_PRIORITY_NORMAL);
}

/// Re-applying the same class must not fail. Windows reports
/// `ERROR_THREAD_MODE_ALREADY_BACKGROUND` when a thread is already in background
/// mode, and `bgrt` classifies with `apply`, which callers may reach more than
/// once for one thread — so that error has to be swallowed.
#[test]
fn apply____background_twice____is_idempotent() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Background).unwrap();
        apply(QosClass::Background).unwrap();
        (current_priority(), current_memory_priority())
    });
    let (priority, memory) = h.join().unwrap();
    assert_eq!(priority, THREAD_PRIORITY_BELOW_NORMAL);
    assert_eq!(memory, MEMORY_PRIORITY_NORMAL);
}

/// The mirror case: leaving background mode when never in it reports
/// `ERROR_THREAD_MODE_NOT_BACKGROUND`, which every `Utility`/`Default`
/// classification of a fresh thread would otherwise hit.
#[test]
fn apply____default_on_a_fresh_thread____tolerates_not_being_in_background_mode() {
    let h = std::thread::spawn(|| {
        apply(QosClass::Default).unwrap();
        apply(QosClass::Utility).unwrap();
        current_priority()
    });
    assert_eq!(h.join().unwrap(), THREAD_PRIORITY_NORMAL);
}

#[test]
fn set_background_mode____round_trip____succeeds_in_both_directions() {
    let h = std::thread::spawn(|| {
        set_background_mode(true).unwrap();
        set_background_mode(true).unwrap(); // already in it
        set_background_mode(false).unwrap();
        set_background_mode(false).unwrap(); // already out of it
    });
    h.join().unwrap();
}
