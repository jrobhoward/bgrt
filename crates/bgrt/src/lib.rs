//! `bgrt` — a *background runtime*: run async tasks and threads at the lowest
//! energy footprint the operating system allows (efficiency cores, low clock
//! frequency) without spinning up the fans, while still making forward progress
//! under load.
//!
//! The mechanism is a per-thread energy [`QosClass`] applied once when a thread
//! starts. It maps to the native low-energy facility on each OS — macOS QoS
//! classes, Windows EcoQoS + background mode, Linux `nice` + `ioprio` — and runs
//! as a regular (non-admin) user. A class covers **CPU and block I/O**; see
//! [`QosClass`] for what each platform delivers.
//!
//! # What's available
//!
//! | API | Requires |
//! |-----|----------|
//! | [`apply`], [`spawn_thread`], [`ThreadBuilder`] | always (no features needed) |
//! | [`RuntimeBuilder`], [`Runtime`] | feature `tokio` (default) |
//! | [`RayonBuilder`], [`RayonPool`] | feature `rayon` |
//!
//! # Examples
//!
//! Classify the current thread directly — no feature flags needed:
//!
//! ```
//! use bgrt::QosClass;
//!
//! bgrt::apply(QosClass::Background)?;
//! # Ok::<(), bgrt::Error>(())
//! ```
//!
//! Spawn a quiet OS thread:
//!
//! ```
//! use bgrt::QosClass;
//!
//! let handle = bgrt::spawn_thread(QosClass::Background, || (0..100u64).sum::<u64>());
//! assert_eq!(handle.join().unwrap(), 4950);
//! ```
//!
//! Build a quiet async runtime (feature `tokio`, on by default):
//!
//! ```
//! # #[cfg(feature = "tokio")] fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use bgrt::{QosClass, RuntimeBuilder};
//!
//! let rt = RuntimeBuilder::new().qos(QosClass::Background).build()?;
//! let sum = rt.block_on(rt.spawn(async { (0..100u64).sum::<u64>() }))?;
//! assert_eq!(sum, 4950);
//! # Ok(()) }
//! # #[cfg(not(feature = "tokio"))] fn main() {}
//! ```
//!
//! Build a quiet rayon pool (feature `rayon`):
//!
//! ```
//! # #[cfg(feature = "rayon")] fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use bgrt::{QosClass, RayonBuilder};
//!
//! let pool = RayonBuilder::new().qos(QosClass::Background).build()?;
//! let sum: u64 = pool.install(|| (0..100u64).sum());
//! assert_eq!(sum, 4950);
//! # Ok(()) }
//! # #[cfg(not(feature = "rayon"))] fn main() {}
//! ```
#![warn(missing_docs)]

mod backend;
pub mod error;
mod qos;
#[cfg(feature = "rayon")]
mod rayon_pool;
#[cfg(feature = "tokio")]
mod runtime;
mod thread;
mod topology;

#[cfg(feature = "telemetry")]
pub mod telemetry;

#[cfg(test)]
mod test_support;

pub use error::Error;
pub use qos::QosClass;
#[cfg(feature = "rayon")]
pub use rayon_pool::{RayonBuilder, RayonPool};
#[cfg(feature = "tokio")]
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
