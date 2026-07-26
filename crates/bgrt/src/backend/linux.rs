//! Linux energy-QoS backend.
//!
//! Sets the calling thread's niceness via `setpriority`. On Linux, niceness is a
//! per-thread (per-task) attribute, so this gives genuine per-thread control.
//! We use max-nice (weighted-fair) rather than `SCHED_IDLE`, so quiet work still
//! makes forward progress under contention instead of starving:
//! [`QosClass::Background`] → `nice(19)`, [`QosClass::Utility`] → `nice(10)`,
//! [`QosClass::Default`] → `nice(0)`.
//!
//! Then lowers **block-I/O priority** to match, via
//! [`ioprio`](super::ioprio) — a `QosClass` governs disk demands as well as CPU,
//! which macOS gets in a single call and Linux needs a second syscall for. Same
//! anti-starvation reasoning: best-effort, never `IOPRIO_CLASS_IDLE`.
//!
//! Raising niceness (lowering priority) is always unprivileged. Efficiency-core
//! affinity is handled separately and opt-in (see the runtime builder), not here.

use crate::error::Error;
use crate::qos::QosClass;

fn nice_value(class: QosClass) -> i32 {
    match class {
        QosClass::Background => 19,
        QosClass::Utility => 10,
        QosClass::Default => 0,
    }
}

pub(super) fn apply(class: QosClass) -> Result<(), Error> {
    let nice = nice_value(class);
    // SAFETY: `setpriority` with `who == 0` targets the calling thread and takes
    // an in-range nice value; it has no other preconditions.
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS as _, 0, nice) };
    if rc != 0 {
        return Err(Error::Backend {
            syscall: "setpriority",
            source: std::io::Error::last_os_error(),
        });
    }
    // The disk half of the class. Best-effort internally, so this only surfaces
    // an error the caller could not have caused.
    super::ioprio::apply(class)
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod linux_tests;
