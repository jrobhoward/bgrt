//! Linux energy-QoS backend.
//!
//! Phase 1 will set the thread's niceness via `setpriority` — `nice(19)` for
//! [`QosClass::Background`], `nice(10)` for [`QosClass::Utility`], `nice(0)` for
//! [`QosClass::Default`] — using max-nice (weighted-fair) rather than
//! `SCHED_IDLE` so quiet work never starves. When efficiency-core pinning is
//! opted in, it will additionally restrict affinity to detected E-cores via
//! `sched_setaffinity`.

use crate::error::Error;
use crate::qos::QosClass;

pub(super) fn apply(_class: QosClass) -> Result<(), Error> {
    // TODO(phase-1): setpriority(PRIO_PROCESS, 0, nice_for(class))
    //                [+ optional sched_setaffinity to E-cores]
    Ok(())
}
