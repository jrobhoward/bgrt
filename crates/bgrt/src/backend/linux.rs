//! Linux energy-QoS backend.
//!
//! Sets the calling thread's niceness via `setpriority`. On Linux, niceness is a
//! per-thread (per-task) attribute, so this gives genuine per-thread control.
//! We use max-nice (weighted-fair) rather than `SCHED_IDLE`, so quiet work still
//! makes forward progress under contention instead of starving:
//! [`QosClass::Background`] → `nice(19)`, [`QosClass::Utility`] → `nice(10)`,
//! [`QosClass::Default`] → `nice(0)`.
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
    if rc == 0 {
        Ok(())
    } else {
        let err = std::io::Error::last_os_error();
        Err(Error::Backend(format!(
            "setpriority(PRIO_PROCESS, 0, {nice}) failed: {err}"
        )))
    }
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod linux_tests;
