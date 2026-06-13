//! macOS energy-QoS backend.
//!
//! Phase 1 will call `pthread_set_qos_class_self_np` to map
//! [`QosClass::Background`] → `QOS_CLASS_BACKGROUND` (efficiency-core-confined)
//! and [`QosClass::Utility`] → `QOS_CLASS_UTILITY`, with [`QosClass::Default`]
//! mapping to `QOS_CLASS_DEFAULT`.

use crate::error::Error;
use crate::qos::QosClass;

pub(super) fn apply(_class: QosClass) -> Result<(), Error> {
    // TODO(phase-1): pthread_set_qos_class_self_np(qos_class_for(class), 0)
    Ok(())
}
