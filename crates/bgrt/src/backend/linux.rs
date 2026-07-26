//! Linux energy-QoS backend.
//!
//! Sets the calling thread's niceness via `setpriority`. On Linux, niceness is a
//! per-thread (per-task) attribute, so this gives genuine per-thread control.
//! Max-nice (weighted-fair) is used rather than `SCHED_IDLE`, so low-priority work still
//! makes forward progress under contention instead of starving:
//! [`QosClass::Background`] → `nice(19)`, [`QosClass::Utility`] → `nice(10)`,
//! [`QosClass::Default`] → `nice(0)`.
//!
//! Then lowers block-I/O priority to match, via
//! [`ioprio`](super::ioprio) — a `QosClass` governs disk demands as well as CPU,
//! which macOS gets in a single call and Linux needs a second syscall for. Same
//! anti-starvation reasoning: best-effort, never `IOPRIO_CLASS_IDLE`.
//!
//! Raising niceness (lowering priority) is always unprivileged. Efficiency-core
//! affinity is handled separately and opt-in (see the runtime builder), not here.
//!
//! # `nice` is one-way, so [`QosClass::Default`] cannot always be honoured
//!
//! An unprivileged thread can raise its nice value but never lower it again:
//! `RLIMIT_NICE` defaults to 0, which puts the floor at the thread's current
//! value. So `setpriority` is refused with `EACCES` whenever the target is
//! *below* where the thread already sits — which happens in two ordinary
//! situations, neither of them the caller's fault:
//!
//! - the process was started already niced (`nice -n 10 …`, systemd `Nice=`), so
//!   every thread inherits a positive nice value and a `Default`-class runtime
//!   would fail on each worker; and
//! - [`QosClass::Default`] is applied to a thread previously classified
//!   `Background` or `Utility`, which cannot be undone at all.
//!
//! Both are treated as a graceful no-op rather than an error, matching how
//! [`ioprio`](super::ioprio) and `uclamp` degrade when the kernel declines a
//! hint: the thread keeps the niceness it had, and `apply` still reports success.
//! macOS and Windows have no such restriction — `Default` does restore
//! there — so this is the one place a class is not fully reversible.
//!
//! Note the halves come apart under reclassification: best-effort I/O levels
//! carry no such privilege check, so `Utility` applied to a `Background` thread
//! moves I/O priority to 6 while niceness stays at 19. That mixed state is a
//! reason to classify once at thread creation — what the builders' thread hooks
//! are for — rather than a defect worth papering over here.

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
    set_nice(nice_value(class))?;
    // The disk half of the class. Best-effort internally, so this only surfaces
    // an error the caller could not have caused.
    super::ioprio::apply(class)
}

/// Set the *current* thread's nice value, tolerating the kernel's refusal to
/// lower one. See the module docs for why that refusal is expected rather than
/// exceptional.
fn set_nice(nice: i32) -> Result<(), Error> {
    // SAFETY: `setpriority` with `who == 0` targets the calling thread and takes
    // an in-range nice value; it has no other preconditions.
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS as _, 0, nice) };
    if rc == 0 {
        return Ok(());
    }

    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        // The only way `who == 0` earns these is by asking to *raise* priority,
        // which is exactly the case to absorb: `bgrt` never needs privileges, so
        // it declines to demand them. Any other errno is a real failure.
        Some(libc::EACCES | libc::EPERM) => {
            tracing::debug!(
                %err,
                nice,
                "unprivileged thread cannot lower its nice value; leaving it alone"
            );
            Ok(())
        }
        _ => Err(Error::Backend {
            syscall: "setpriority",
            source: err,
        }),
    }
}

#[cfg(test)]
#[path = "linux_tests.rs"]
mod linux_tests;
