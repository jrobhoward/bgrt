//! Per-OS dispatch for applying a [`QosClass`] to the current thread.
//!
//! Each platform module exposes `apply(QosClass) -> Result<(), Error>` acting on
//! the calling thread: macOS via `pthread_set_qos_class_self_np`, Linux via
//! `setpriority`, Windows via EcoQoS (`SetThreadInformation`) + `SetThreadPriority`.
//! Any other platform gets a no-op so the API is callable everywhere.

use crate::error::Error;
use crate::qos::QosClass;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
mod uclamp;
#[cfg(target_os = "windows")]
mod windows;

/// Opt-in Linux utilization clamp (a frequency hint); a no-op on other
/// platforms. See [`uclamp`] for the rationale.
pub(crate) use uclamp::clamp_current_thread;

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
