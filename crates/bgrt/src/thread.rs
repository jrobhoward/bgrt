//! Spawning energy-classified OS threads — the non-async path.
//!
//! Use [`spawn_thread`] for a quick classified thread (mirrors
//! [`std::thread::spawn`]), or [`ThreadBuilder`] to also set a name, stack size,
//! or efficiency-core pinning (mirrors [`std::thread::Builder`]).

use std::thread::{self, JoinHandle};

use crate::qos::QosClass;
use crate::topology;

/// Apply the energy class (and, if requested, efficiency-core pinning) to the
/// **current** thread. Best-effort: warns on failure rather than aborting, since
/// QoS/affinity are optimizations, not correctness.
fn classify(class: QosClass, efficiency_cores: &[usize]) {
    if let Err(e) = crate::apply(class) {
        tracing::warn!(error = %e, "bgrt: failed to apply qos to thread");
    }
    if !efficiency_cores.is_empty() {
        if let Err(e) = topology::pin_current_thread(efficiency_cores) {
            tracing::warn!(error = %e, "bgrt: failed to pin thread to efficiency cores");
        }
    }
}

/// Spawn an OS thread classified with `class`, running `f`.
///
/// The QoS class is applied to the new thread before `f` runs. This mirrors
/// [`std::thread::spawn`], including that it **panics** if the OS cannot create
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
        classify(class, &[]);
        f()
    })
}

/// Builder for an energy-classified OS thread.
///
/// Defaults: [`QosClass::Background`], no name, default stack size,
/// efficiency-core pinning off.
#[derive(Debug, Clone)]
pub struct ThreadBuilder {
    qos: QosClass,
    name: Option<String>,
    stack_size: Option<usize>,
    pin_efficiency_cores: bool,
}

impl Default for ThreadBuilder {
    fn default() -> Self {
        Self {
            qos: QosClass::Background,
            name: None,
            stack_size: None,
            pin_efficiency_cores: false,
        }
    }
}

impl ThreadBuilder {
    /// Create a builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the energy [`QosClass`] applied to the spawned thread.
    pub fn qos(mut self, qos: QosClass) -> Self {
        self.qos = qos;
        self
    }

    /// Set the thread's name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the thread's stack size, in bytes.
    pub fn stack_size(mut self, bytes: usize) -> Self {
        self.stack_size = Some(bytes);
        self
    }

    /// On Linux, also pin the spawned thread to detected efficiency cores. No-op
    /// on macOS/Windows and on homogeneous CPUs. Opt-in; off by default.
    pub fn pin_efficiency_cores(mut self, pin: bool) -> Self {
        self.pin_efficiency_cores = pin;
        self
    }

    /// Spawn the thread, running `f`. The QoS class (and pinning, if enabled) is
    /// applied before `f` runs.
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
            classify(qos, &efficiency_cores);
            f()
        })
    }
}

#[cfg(test)]
#[path = "thread_tests.rs"]
mod thread_tests;
