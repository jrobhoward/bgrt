//! Energy-classified rayon thread pool.
//!
//! Build one with [`RayonBuilder`]; every thread the pool spawns has the
//! configured [`QosClass`] applied at start, so all work scheduled onto it —
//! `par_iter`, `join`, `scope` — runs at the chosen energy footprint.

use std::fmt;

use crate::error::Error;
use crate::qos::QosClass;
use crate::thread::classify;
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
    clamp_frequency: bool,
}

impl Default for RayonBuilder {
    fn default() -> Self {
        Self {
            qos: QosClass::Background,
            num_threads: None,
            thread_name: None,
            pin_efficiency_cores: false,
            clamp_frequency: false,
        }
    }
}

impl RayonBuilder {
    /// Create a builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the energy [`QosClass`] applied to every pool thread.
    #[must_use]
    pub fn qos(mut self, qos: QosClass) -> Self {
        self.qos = qos;
        self
    }

    /// Set the number of threads in the pool. Defaults to one per logical CPU.
    #[must_use]
    pub fn num_threads(mut self, n: usize) -> Self {
        self.num_threads = Some(n);
        self
    }

    /// Set a name prefix for pool threads (e.g. `"bgrt-worker"` → threads are
    /// named `"bgrt-worker-0"`, `"bgrt-worker-1"`, …).
    #[must_use]
    pub fn thread_name(mut self, name: impl Into<String>) -> Self {
        self.thread_name = Some(name.into());
        self
    }

    /// On Linux, also pin pool threads to detected efficiency cores. No-op on
    /// macOS and Windows (the OS QoS/EcoQoS places work on efficient cores) and
    /// on homogeneous CPUs. Opt-in; off by default.
    #[must_use]
    pub fn pin_efficiency_cores(mut self, pin: bool) -> Self {
        self.pin_efficiency_cores = pin;
        self
    }

    /// On Linux, also cap pool threads' CPU frequency via `uclamp` (a
    /// utilization clamp) for the [`QosClass::Background`] class — the only lever
    /// that lowers clocks on homogeneous CPUs, where `nice` leaves frequency
    /// untouched. No-op on macOS/Windows (their QoS/EcoQoS throttle frequency
    /// directly), for other classes, and on kernels or governors without uclamp
    /// support. Opt-in; off by default.
    ///
    /// Only the `schedutil` governor reads the clamp; `build` logs at debug
    /// level when no cpufreq policy runs it. Where cores share a clock (a
    /// Raspberry Pi, many arm64 boards), the domain runs at the speed its
    /// busiest core asks for, so the clamp holds the clock down only while
    /// nothing unclamped is busy in that domain.
    #[must_use]
    pub fn clamp_frequency(mut self, clamp: bool) -> Self {
        self.clamp_frequency = clamp;
        self
    }

    /// Build the pool.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ThreadPool`] if rayon cannot create the pool.
    pub fn build(self) -> Result<RayonPool, Error> {
        let qos = self.qos;
        let clamp_frequency = self.clamp_frequency;
        if clamp_frequency {
            crate::backend::note_clamp_governor(qos);
        }
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

        // The same qos → pin → clamp step every bgrt-spawned thread runs; see
        // `thread::classify`. Shared rather than reimplemented so a knob added to
        // one builder cannot silently skip this one.
        builder =
            builder.start_handler(move |_idx| classify(qos, &efficiency_cores, clamp_frequency));

        let inner = builder
            .build()
            .map_err(|e| Error::ThreadPool(Box::new(e)))?;

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
    #[must_use]
    pub fn qos(&self) -> QosClass {
        self.qos
    }
}

#[cfg(test)]
#[path = "rayon_pool_tests.rs"]
mod rayon_pool_tests;
