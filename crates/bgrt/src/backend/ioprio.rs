//! Linux block-I/O priority — the disk half of a [`QosClass`].
//!
//! `bgrt` classifies a thread's *resource* demands, not only its CPU demands, so
//! the Linux backend lowers block-I/O priority alongside niceness:
//! [`QosClass::Background`] → best-effort level 7, [`QosClass::Utility`] →
//! best-effort level 6. macOS gets the same effect for free
//! (`QOS_CLASS_BACKGROUND` implies disk-I/O throttling); Windows does not yet —
//! see `docs/DESIGN.md`.
//!
//! # Why best-effort and not `IOPRIO_CLASS_IDLE`
//!
//! `IOPRIO_CLASS_IDLE` gets the disk only when nothing else wants it — the I/O
//! equivalent of `SCHED_IDLE`, which this project rejects for CPU because low-priority
//! work must still crawl forward under contention. Best-effort level 7 is the
//! weighted-fair choice, exactly parallel to using `nice(19)` over `SCHED_IDLE`.
//!
//! # Why set it explicitly when `nice` already implies it
//!
//! With no explicit I/O priority the kernel derives a best-effort level from
//! niceness — `(nice + 20) / 5`, which maps this backend's `nice(19)`/`nice(10)`
//! onto exactly levels 7 and 6. Setting it outright costs one syscall and buys
//! independence from that derivation, intent that is visible in `strace`, and a
//! value directly assertable via `ioprio_get` in tests.
//!
//! # What this does not do
//!
//! Whether the priority is *honoured* is the I/O scheduler's business, and in
//! practice only BFQ honours it. `mq-deadline` keeps one queue per priority
//! *class* (real-time, best-effort, idle) and ignores the level within a class,
//! so best-effort 7 and best-effort 4 look the same to it; the idle class would
//! register, but that is the starvation this module rules out. `none` — a
//! common default for NVMe — ignores priority entirely. Like the `uclamp`
//! frequency clamp, this is a hint that does nothing on some configurations;
//! check with `cat /sys/block/<dev>/queue/scheduler`.

#[cfg(target_os = "linux")]
use crate::error::Error;
#[cfg(any(target_os = "linux", test))]
use crate::qos::QosClass;

/// `IOPRIO_WHO_PROCESS` — with a pid of 0 this targets the calling *thread*,
/// despite the name (a Linux thread is a task).
#[cfg(target_os = "linux")]
const IOPRIO_WHO_PROCESS: i32 = 1;

/// `IOPRIO_CLASS_BE` — best-effort, the weighted-fair class.
#[cfg(any(target_os = "linux", test))]
const IOPRIO_CLASS_BE: i32 = 2;

/// The class occupies the high bits of the priority word.
#[cfg(any(target_os = "linux", test))]
const IOPRIO_CLASS_SHIFT: i32 = 13;

/// Best-effort level for `class`, or `None` to leave I/O priority untouched.
///
/// [`QosClass::Default`] deliberately sets nothing. `bgrt` only ever *lowers* a
/// thread's demands, and there is nothing to lower here: an unset I/O priority
/// already tracks the thread's niceness, which the `Default` mapping has just
/// set to 0. Writing an explicit value would be the one place this backend
/// raised a priority instead of lowering it.
///
/// Compiled on Linux (where it backs [`apply`]) and under `test`, so the mapping
/// itself is asserted on every platform.
#[cfg(any(target_os = "linux", test))]
fn best_effort_level(class: QosClass) -> Option<i32> {
    match class {
        QosClass::Background => Some(7), // the lowest best-effort level
        QosClass::Utility => Some(6),
        QosClass::Default => None,
    }
}

/// `IOPRIO_PRIO_VALUE(class, data)` from `<linux/ioprio.h>`.
#[cfg(any(target_os = "linux", test))]
fn ioprio_value(class: i32, level: i32) -> i32 {
    (class << IOPRIO_CLASS_SHIFT) | level
}

/// Lower the *current* thread's block-I/O priority to match `class`.
///
/// Best-effort: a kernel or sandbox that refuses the syscall degrades to a no-op
/// rather than failing the caller, since I/O priority is an optimization, not
/// correctness.
#[cfg(target_os = "linux")]
pub(super) fn apply(class: QosClass) -> Result<(), Error> {
    let Some(level) = best_effort_level(class) else {
        return Ok(());
    };
    let prio = ioprio_value(IOPRIO_CLASS_BE, level);

    // SAFETY: `ioprio_set` takes three integers by value and mutates only the
    // calling thread's I/O priority (`IOPRIO_WHO_PROCESS` with pid 0). `libc`
    // ships no wrapper for it, hence the raw syscall.
    let rc = unsafe { libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0, prio) };
    if rc == 0 {
        return Ok(());
    }

    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        // Syscall absent, or blocked by a seccomp sandbox: degrade to a no-op,
        // matching how the uclamp clamp treats an unsupported kernel.
        Some(libc::ENOSYS | libc::EPERM | libc::EINVAL) => {
            tracing::debug!(%err, "ioprio_set unavailable; leaving I/O priority alone");
            Ok(())
        }
        _ => Err(Error::Backend {
            syscall: "ioprio_set",
            source: err,
        }),
    }
}

#[cfg(test)]
#[path = "ioprio_tests.rs"]
mod ioprio_tests;
