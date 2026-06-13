//! Windows energy-QoS backend.
//!
//! Phase 1 will enable EcoQoS (`SetThreadInformation` with
//! `THREAD_POWER_THROTTLING_EXECUTION_SPEED`) plus an appropriate
//! `SetThreadPriority`: `THREAD_PRIORITY_BELOW_NORMAL` for
//! [`QosClass::Background`] and `THREAD_PRIORITY_NORMAL` for
//! [`QosClass::Utility`] (deliberately not `IDLE`, to avoid starvation).
//! [`QosClass::Default`] clears power throttling and restores normal priority.

use crate::error::Error;
use crate::qos::QosClass;

pub(super) fn apply(_class: QosClass) -> Result<(), Error> {
    // TODO(phase-1): SetThreadInformation(ThreadPowerThrottling, EXECUTION_SPEED)
    //                + SetThreadPriority(priority_for(class))
    Ok(())
}
