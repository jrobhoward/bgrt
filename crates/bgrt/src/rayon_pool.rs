//! Energy-classified rayon thread pool.
//!
//! Build one with [`RayonBuilder`]; every thread the pool spawns has the
//! configured [`QosClass`] applied at start, so all work scheduled onto it —
//! `par_iter`, `join`, `scope` — runs at the chosen energy footprint.

use std::fmt;

use crate::error::Error;
use crate::qos::QosClass;
use crate::topology;

/// Builder for an energy-classified [`RayonPool`].
///
/// Defaults: [`QosClass::Background`], rayon's default thread count (one per
/// logical CPU), no explicit thread name.
///
/// # Examples
///
/// ```
/// use bgrt::{QosClass, RayonBuilder};
///
/// let pool = RayonBuilder::new()
///     .qos(QosClass::Background)
///     .num_threads(4)
///     .build()?;
/// let sum: u64 = pool.install(|| (0..100u64).sum());
/// assert_eq!(sum, 4950);
/// # Ok::<(), bgrt::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct RayonBuilder {
    qos: QosClass,
    num_threads: Option<usize>,
    thread_name: Option<String>,
    pin_efficiency_cores: bool,
}

impl Default for RayonBuilder {
    fn default() -> Self {
        Self {
            qos: QosClass::Background,
            num_threads: None,
            thread_name: None,
            pin_efficiency_cores: false,
        }
    }
}

impl RayonBuilder {
    /// Create a builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the energy [`QosClass`] applied to every pool thread.
    pub fn qos(mut self, qos: QosClass) -> Self {
        self.qos = qos;
        self
    }

    /// Set the number of threads in the pool. Defaults to one per logical CPU.
    pub fn num_threads(mut self, n: usize) -> Self {
        self.num_threads = Some(n);
        self
    }

    /// Set a name prefix for pool threads (e.g. `"bgrt-worker"` → threads are
    /// named `"bgrt-worker-0"`, `"bgrt-worker-1"`, …).
    pub fn thread_name(mut self, name: impl Into<String>) -> Self {
        self.thread_name = Some(name.into());
        self
    }

    /// On Linux, also pin pool threads to detected efficiency cores. No-op on
    /// macOS and Windows (the OS QoS/EcoQoS places work on efficient cores) and
    /// on homogeneous CPUs. Opt-in; off by default.
    pub fn pin_efficiency_cores(mut self, pin: bool) -> Self {
        self.pin_efficiency_cores = pin;
        self
    }

    /// Build the pool.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ThreadPool`] if rayon cannot create the pool.
    pub fn build(self) -> Result<RayonPool, Error> {
        let qos = self.qos;
        let efficiency_cores = if self.pin_efficiency_cores {
            topology::efficiency_cores()
        } else {
            Vec::new()
        };

        let mut builder = rayon::ThreadPoolBuilder::new();

        if let Some(n) = self.num_threads {
            builder = builder.num_threads(n);
        }

        if let Some(name) = self.thread_name {
            builder = builder.thread_name(move |idx| format!("{name}-{idx}"));
        }

        builder = builder.start_handler(move |_idx| {
            if let Err(e) = crate::apply(qos) {
                tracing::warn!(error = %e, "bgrt: failed to apply qos to rayon thread");
            }
            if !efficiency_cores.is_empty() {
                if let Err(e) = topology::pin_current_thread(&efficiency_cores) {
                    tracing::warn!(error = %e, "bgrt: failed to pin rayon thread to efficiency cores");
                }
            }
        });

        let inner = builder
            .build()
            .map_err(|e| Error::ThreadPool(e.to_string()))?;

        Ok(RayonPool { inner, qos })
    }
}

/// An energy-classified rayon thread pool.
///
/// All threads in this pool carry the [`QosClass`] set on the builder.
/// `Deref`s to [`rayon::ThreadPool`], so the full rayon API (`install`,
/// `spawn`, `scope`, `join`, …) is available directly.
pub struct RayonPool {
    inner: rayon::ThreadPool,
    qos: QosClass,
}

impl fmt::Debug for RayonPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RayonPool")
            .field("qos", &self.qos)
            .field("num_threads", &self.inner.current_num_threads())
            .finish()
    }
}

impl std::ops::Deref for RayonPool {
    type Target = rayon::ThreadPool;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl RayonPool {
    /// The energy [`QosClass`] applied to this pool's threads.
    pub fn qos(&self) -> QosClass {
        self.qos
    }
}

#[cfg(test)]
#[path = "rayon_pool_tests.rs"]
mod rayon_pool_tests;
