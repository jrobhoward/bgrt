//! `bgrt` — a *background runtime*: run async tasks and threads at the lowest
//! energy footprint the operating system allows (efficiency cores, low clock
//! frequency) without spinning up the fans, while still making forward progress
//! under load.
//!
//! The mechanism is a per-thread energy [`QosClass`] applied once when a thread
//! starts. It maps to the native low-energy facility on each OS — macOS QoS
//! classes, Windows EcoQoS, Linux `nice` — and runs as a regular (non-admin)
//! user.
//!
//! `bgrt` provides an energy-classified async runtime — build one with
//! [`RuntimeBuilder`] and schedule futures onto it with [`Runtime::spawn`] — a
//! quiet-thread spawner for the non-async path ([`spawn_thread`] /
//! [`ThreadBuilder`]), and [`apply`], which classifies the **current** thread
//! directly.
//!
//! # Example
//!
//! ```
//! use bgrt::{QosClass, RuntimeBuilder};
//!
//! // A quiet, single-worker runtime; its thread runs on efficiency cores at a
//! // low clock where the OS supports it.
//! let rt = RuntimeBuilder::new().qos(QosClass::Background).build()?;
//! let sum = rt.block_on(rt.spawn(async { (0..100u64).sum::<u64>() }))?;
//! assert_eq!(sum, 4950);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#![warn(missing_docs)]

mod backend;
pub mod error;
mod qos;
mod runtime;
mod thread;
mod topology;

#[cfg(feature = "telemetry")]
pub mod telemetry;

#[cfg(test)]
mod test_support;

pub use error::Error;
pub use qos::QosClass;
pub use runtime::{Runtime, RuntimeBuilder};
pub use thread::{ThreadBuilder, spawn_thread};

/// Apply an energy [`QosClass`] to the **current** thread.
///
/// This only ever *lowers* the calling thread's scheduling demands, so it never
/// requires elevated privileges. Classification is intended to happen once,
/// early in a thread's life (for example from a runtime's thread-start hook).
///
/// # Errors
///
/// Returns [`Error::Backend`] if the underlying OS call fails.
///
/// # Examples
///
/// ```
/// use bgrt::QosClass;
///
/// // Make the current thread quiet and energy-efficient.
/// bgrt::apply(QosClass::Background)?;
/// # Ok::<(), bgrt::Error>(())
/// ```
pub fn apply(class: QosClass) -> Result<(), Error> {
    tracing::trace!(?class, "applying energy qos to current thread");
    backend::apply(class)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;
