//! `bgrt` — a *background runtime*: run async tasks and threads at the lowest
//! energy footprint the operating system allows (efficiency cores, low clock
//! frequency) without spinning up the fans, while still making forward progress
//! under load.
//!
//! The mechanism is a per-thread energy [`QosClass`] applied once when a thread
//! starts. It maps to the native low-energy facility on each OS — macOS QoS
//! classes, Windows EcoQoS, Linux `nice` + efficiency-core affinity — and runs
//! as a regular (non-admin) user.
//!
//! This is **Phase 0** scaffolding: [`apply`] dispatches to per-OS backends that
//! are currently no-ops. The QoS backends, tokio [`Runtime`](crate) wrapper,
//! quiet-thread spawner, and measurement harness land in later phases — see
//! `docs/ROADMAP.md`.

mod backend;
pub mod error;
mod qos;

pub use error::Error;
pub use qos::QosClass;

/// Apply an energy [`QosClass`] to the **current** thread.
///
/// This only ever *lowers* the calling thread's scheduling demands, so it never
/// requires elevated privileges. Classification is intended to happen once,
/// early in a thread's life (for example from a runtime's thread-start hook).
///
/// # Errors
///
/// Returns [`Error::Backend`] if the underlying OS call fails.
pub fn apply(class: QosClass) -> Result<(), Error> {
    tracing::trace!(?class, "applying energy qos to current thread");
    backend::apply(class)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;
