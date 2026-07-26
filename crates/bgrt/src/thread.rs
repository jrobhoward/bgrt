//! Spawning energy-classified OS threads — the non-async path.
//!
//! Use [`spawn_thread`] for a quick classified thread (mirrors
//! [`std::thread::spawn`]), or [`ThreadBuilder`] to also set a name, stack size,
//! or efficiency-core pinning (mirrors [`std::thread::Builder`]).
//!
//! # The classification stops at this thread
//!
//! Threads spawned *by* the classified thread inherit it only on Linux; on macOS
//! and Windows they start unclassified. So running a library that manages its
//! own worker threads (RocksDB's compaction pool, for instance) on a
//! `Background` thread does not lower that library's own threads on two of
//! three platforms — and their classification APIs are current-thread-only, so
//! it cannot be corrected from outside either. See [`QosClass`] and the README.

use std::thread::{self, JoinHandle};

use crate::qos::QosClass;
use crate::topology;

/// Apply the energy class (and, if requested, efficiency-core pinning and a
/// frequency clamp) to the *current* thread. Best-effort: warns on failure
/// rather than aborting, since QoS/affinity/clamp are optimizations, not
/// correctness.
///
/// Shared by every thread `bgrt` starts — [`ThreadBuilder`] bodies and the
/// runtime's `on_thread_start` hook — so all three builders resolve their knobs
/// identically.
pub(crate) fn classify(class: QosClass, efficiency_cores: &[usize], clamp_frequency: bool) {
    if let Err(e) = crate::apply(class) {
        tracing::warn!(error = %e, "bgrt: failed to apply qos to thread");
    }
    if !efficiency_cores.is_empty() {
        if let Err(e) = topology::pin_current_thread(efficiency_cores) {
            tracing::warn!(error = %e, "bgrt: failed to pin thread to efficiency cores");
        }
    }
    if clamp_frequency {
        if let Err(e) = crate::backend::clamp_current_thread(class) {
            tracing::warn!(error = %e, "bgrt: failed to clamp thread frequency");
        }
    }
}

/// Spawn an OS thread classified with `class`, running `f`.
///
/// The QoS class is applied to the new thread before `f` runs. This mirrors
/// [`std::thread::spawn`], including that it *panics* if the OS cannot create
/// the thread; use [`ThreadBuilder::spawn`] for a non-panicking `Result`. It does
/// not pin to efficiency cores — use [`ThreadBuilder`] for that.
///
/// # Examples
///
/// ```
/// use bgrt::QosClass;
///
/// let handle = bgrt::spawn_thread(QosClass::Background, || 2 + 2);
/// assert_eq!(handle.join().unwrap(), 4);
/// ```
pub fn spawn_thread<F, T>(class: QosClass, f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    thread::spawn(move || {
        classify(class, &[], false);
        f()
    })
}

/// Builder for an energy-classified OS thread.
///
/// Defaults: [`QosClass::Background`], no name, default stack size,
/// efficiency-core pinning off, frequency clamp off.
#[derive(Debug, Clone)]
pub struct ThreadBuilder {
    qos: QosClass,
    name: Option<String>,
    stack_size: Option<usize>,
    pin_efficiency_cores: bool,
    clamp_frequency: bool,
}

impl Default for ThreadBuilder {
    fn default() -> Self {
        Self {
            qos: QosClass::Background,
            name: None,
            stack_size: None,
            pin_efficiency_cores: false,
            clamp_frequency: false,
        }
    }
}

impl ThreadBuilder {
    /// Create a builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the energy [`QosClass`] applied to the spawned thread.
    #[must_use]
    pub fn qos(mut self, qos: QosClass) -> Self {
        self.qos = qos;
        self
    }

    /// Set the thread's name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the thread's stack size, in bytes.
    #[must_use]
    pub fn stack_size(mut self, bytes: usize) -> Self {
        self.stack_size = Some(bytes);
        self
    }

    /// On Linux, also pin the spawned thread to detected efficiency cores. No-op
    /// on macOS/Windows and on homogeneous CPUs. Opt-in; off by default.
    #[must_use]
    pub fn pin_efficiency_cores(mut self, pin: bool) -> Self {
        self.pin_efficiency_cores = pin;
        self
    }

    /// On Linux, also cap the spawned thread's CPU frequency via `uclamp` (a
    /// utilization clamp) for the [`QosClass::Background`] class. This is the
    /// only lever that lowers clocks on homogeneous CPUs, where `nice` alone
    /// leaves frequency untouched. No-op on macOS/Windows (their QoS/EcoQoS
    /// throttle frequency directly), for other classes, and on kernels or
    /// governors without uclamp support. Opt-in; off by default.
    #[must_use]
    pub fn clamp_frequency(mut self, clamp: bool) -> Self {
        self.clamp_frequency = clamp;
        self
    }

    /// Spawn the thread, running `f`. The QoS class (and pinning/clamp, if
    /// enabled) is applied before `f` runs.
    ///
    /// # Errors
    ///
    /// Returns the [`std::io::Error`] from [`std::thread::Builder::spawn`] if the
    /// OS cannot create the thread.
    pub fn spawn<F, T>(self, f: F) -> std::io::Result<JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        let qos = self.qos;
        let clamp_frequency = self.clamp_frequency;
        let efficiency_cores = if self.pin_efficiency_cores {
            topology::efficiency_cores()
        } else {
            Vec::new()
        };

        let mut builder = thread::Builder::new();
        if let Some(name) = self.name {
            builder = builder.name(name);
        }
        if let Some(bytes) = self.stack_size {
            builder = builder.stack_size(bytes);
        }

        builder.spawn(move || {
            classify(qos, &efficiency_cores, clamp_frequency);
            f()
        })
    }
}

#[cfg(test)]
#[path = "thread_tests.rs"]
mod thread_tests;
