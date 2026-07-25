//! macOS energy-QoS backend.
//!
//! Maps a [`QosClass`] onto a Darwin QoS class via `pthread_set_qos_class_self_np`.
//! [`QosClass::Background`] → `QOS_CLASS_BACKGROUND`, which on Apple Silicon is
//! confined to efficiency cores; [`QosClass::Utility`] → `QOS_CLASS_UTILITY`
//! (quieter than default, all cores); [`QosClass::Default`] → `QOS_CLASS_DEFAULT`.
//! Lowering QoS never requires privileges.

use crate::error::Error;
use crate::qos::QosClass;

// `qos_class_t` values from `<sys/qos.h>` — a stable ABI.
const QOS_CLASS_DEFAULT: u32 = 0x15;
const QOS_CLASS_UTILITY: u32 = 0x11;
const QOS_CLASS_BACKGROUND: u32 = 0x09;

unsafe extern "C" {
    // Provided by libSystem (linked by default). Sets the calling thread's QoS.
    fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
}

fn qos_value(class: QosClass) -> u32 {
    match class {
        QosClass::Background => QOS_CLASS_BACKGROUND,
        QosClass::Utility => QOS_CLASS_UTILITY,
        QosClass::Default => QOS_CLASS_DEFAULT,
    }
}

pub(super) fn apply(class: QosClass) -> Result<(), Error> {
    let qos = qos_value(class);
    // SAFETY: `pthread_set_qos_class_self_np` mutates only the calling thread's
    // QoS and takes a documented `qos_class_t` value with relative priority 0.
    let rc = unsafe { pthread_set_qos_class_self_np(qos, 0) };
    if rc == 0 {
        Ok(())
    } else {
        // pthread functions return the error code directly rather than setting
        // errno, so build the `io::Error` from `rc` instead of `last_os_error`.
        Err(Error::Backend {
            syscall: "pthread_set_qos_class_self_np",
            source: std::io::Error::from_raw_os_error(rc),
        })
    }
}

#[cfg(test)]
#[path = "macos_tests.rs"]
mod macos_tests;
