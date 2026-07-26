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
//!
//! # Semver and wrapped dependencies
//!
//! `bgrt` **wraps** Tokio and rayon rather than hiding them, so their types are
//! part of its public API by design: [`Runtime::spawn`] returns Tokio's
//! `JoinHandle`, [`Runtime::handle`] hands back its `Handle`, and [`RayonPool`]
//! derefs to `rayon::ThreadPool`. Passing those through is the point — a wrapper
//! that hid them would force you to give up the ecosystem built on them.
//!
//! The consequence is that **a major release of Tokio or rayon is a major
//! release of `bgrt`**, and the two majors cannot be mixed: a
//! `&tokio_1::runtime::Handle` is a different type from its 2.x counterpart. To
//! name the exact versions `bgrt` resolved without declaring them yourself, use
//! the re-exports [`tokio`] and [`rayon`].
//!
//! Two things sit outside this. The [`telemetry`] module is exempt from semver
//! altogether (see its docs). And [`Error`] deliberately boxes rayon's build
//! error, so the enum's shape survives rayon reshaping its own error type —
//! that is about *this* enum staying stable, not about hiding the dependency.
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

/// The Tokio version `bgrt` wraps, re-exported so callers can name its types
/// (`JoinHandle`, `Handle`, …) without risking a mismatched second copy in the
/// dependency graph. See *Semver and wrapped dependencies*.
#[cfg(feature = "tokio")]
pub use tokio;

/// The rayon version `bgrt` wraps, re-exported so callers can name its types
/// without risking a mismatched second copy in the dependency graph. See *Semver
/// and wrapped dependencies*.
#[cfg(feature = "rayon")]
pub use rayon;

/// Apply an energy [`QosClass`] to the **current** thread.
///
/// This never requires elevated privileges: lowering a thread's scheduling
/// demands is always permitted, and where the reverse is *not* — raising them
/// back — `bgrt` declines rather than demanding privileges it promises not to
/// need. Classification is intended to happen once, early in a thread's life
/// (for example from a runtime's thread-start hook).
///
/// On Linux that reverse direction is genuinely unavailable: `nice` is one-way
/// for an unprivileged thread, so applying [`QosClass::Default`] to a thread
/// already classified `Background` (or running in an already-niced process)
/// leaves its niceness where it is and still returns `Ok`. macOS and Windows can
/// restore. See [`QosClass`] for the full picture.
///
/// # Errors
///
/// Returns [`Error::Backend`] if the underlying OS call fails. A kernel refusing
/// an optional hint — an unraisable `nice`, an absent `ioprio_set`, a governor
/// without `uclamp` — is not a failure and does not appear here.
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
