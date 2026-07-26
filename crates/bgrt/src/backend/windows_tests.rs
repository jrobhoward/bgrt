//! Tests for the Windows QoS backend.
//!
//! EcoQoS power-throttling state is set-only (no documented read-back), so these
//! assert the observable `SetThreadPriority` effect and that `apply` succeeds.
//! Background-processing mode has no read-back either, but *is* observable
//! indirectly: memory priority is a documented side effect with a getter, and
//! the mode's own "already in that state" error codes prove the transitions.
#![allow(non_snake_case)]

use windows_sys::Win32::System::Threading::{
    MEMORY_PRIORITY_VERY_LOW, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL,
};

use super::{apply, memory_priority, set_background_mode};
use crate::qos::QosClass;
use crate::test_support::current_thread_priority as current_priority;

/// Read the calling thread's memory priority, panicking rather than propagating.
/// Background mode lowers this as a side effect, so it doubles as evidence the
/// mode was entered.
fn current_memory_priority() -> u32 {
    memory_priority().expect("GetThreadInformation(ThreadMemoryPriority) failed")
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
/// memory priority. This asserts the raw mode takes effect — without it,
/// the restore test below would pass just as happily if the mode were never
/// entered at all, since the restored value is also the starting one.
///
/// Deliberately compares against the *observed* starting value rather than
/// asserting either endpoint: what matters is that the mode demonstrably did
/// something, not which low value Windows picked — and not that the thread began
/// at `MEMORY_PRIORITY_NORMAL`, which it need not. A process may lower its own
/// default, and threads inherit that; GitHub Actions `windows-latest` runners
/// start threads at `MEMORY_PRIORITY_LOW`.
#[test]
fn set_background_mode____entered____lowers_memory_priority() {
    let h = std::thread::spawn(|| {
        let before = current_memory_priority();
        set_background_mode(true).unwrap();
        (before, current_memory_priority())
    });
    let (before, after) = h.join().unwrap();
    if before == MEMORY_PRIORITY_VERY_LOW {
        // Already at the floor, so there is nothing left to lower.
        assert_eq!(after, before);
    } else {
        assert!(
            after < before,
            "background mode left memory priority at {after} (was {before}); \
             it should have lowered it"
        );
    }
}

/// And this proves the backend undoes that side effect: paired with the test
/// above, the two together show the mode was entered *and* memory priority was
/// put back — back to what the thread had, which is the point. Asserting
/// `MEMORY_PRIORITY_NORMAL` here would let a backend that hard-codes normal pass
/// while silently *raising* a thread that started lower.
#[test]
fn apply____background____restores_memory_priority_after_entering_background_mode() {
    let h = std::thread::spawn(|| {
        let before = current_memory_priority();
        apply(QosClass::Background).unwrap();
        (before, current_memory_priority())
    });
    let (before, after) = h.join().unwrap();
    assert_eq!(after, before);
}

/// Re-applying the same class must not fail. Windows reports
/// `ERROR_THREAD_MODE_ALREADY_BACKGROUND` when a thread is already in background
/// mode, and `bgrt` classifies with `apply`, which callers may reach more than
/// once for one thread — so that error has to be swallowed.
#[test]
fn apply____background_twice____is_idempotent() {
    let h = std::thread::spawn(|| {
        let before = current_memory_priority();
        apply(QosClass::Background).unwrap();
        apply(QosClass::Background).unwrap();
        (current_priority(), before, current_memory_priority())
    });
    let (priority, before, memory) = h.join().unwrap();
    assert_eq!(priority, THREAD_PRIORITY_BELOW_NORMAL);
    assert_eq!(memory, before);
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
