//! Per-OS dispatch for applying a [`QosClass`] to the current thread.
//!
//! Each platform module exposes `apply(QosClass) -> Result<(), Error>` acting on
//! the calling thread. Phase 0 ships no-op skeletons; Phase 1 fills in the FFI
//! (macOS `pthread_set_qos_class_self_np`, Linux `setpriority` + affinity,
//! Windows EcoQoS via `SetThreadInformation`).

use crate::error::Error;
use crate::qos::QosClass;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
pub(crate) fn apply(class: QosClass) -> Result<(), Error> {
    macos::apply(class)
}

#[cfg(target_os = "linux")]
pub(crate) fn apply(class: QosClass) -> Result<(), Error> {
    linux::apply(class)
}

#[cfg(target_os = "windows")]
pub(crate) fn apply(class: QosClass) -> Result<(), Error> {
    windows::apply(class)
}

/// Unsupported platforms get a no-op so the API is usable everywhere.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub(crate) fn apply(_class: QosClass) -> Result<(), Error> {
    Ok(())
}
