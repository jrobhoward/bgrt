//! An energy-classified async runtime that wraps a Tokio runtime.
//!
//! Build one with [`RuntimeBuilder`]; every thread the runtime spawns (workers
//! and the blocking pool) has the configured [`QosClass`] applied at start, so
//! all work scheduled onto it runs at the chosen energy footprint.
//!
//! # Why there is no current-thread runtime
//!
//! The runtime is always multi-thread, even at one worker. A Tokio
//! *current-thread* runtime drives its tasks on whichever thread calls
//! `block_on` — the caller's thread, which `bgrt` does not own and must not
//! reclassify: on Linux an unprivileged thread can lower its niceness but never
//! raise it back, so classifying the caller would permanently deprioritize it.
//! Its `on_thread_start` hook fires only for blocking-pool threads, so the async
//! work would silently run *unclassified* — the exact opposite of the point.
//!
//! `worker_threads(1)` is therefore the single-quiet-worker configuration, and
//! it costs exactly one thread (Tokio drives I/O and timers on the worker
//! itself; there is no extra driver thread).

use std::future::Future;
use std::time::Duration;

use tokio::runtime::{Handle, Runtime as TokioRuntime};
use tokio::task::JoinHandle;

use crate::error::Error;
use crate::qos::QosClass;
use crate::topology;

/// Builder for an energy-classified [`Runtime`].
///
/// Defaults: [`QosClass::Background`], one worker thread, efficiency-core pinning
/// off, frequency clamp off.
///
/// Note that this default is **not** [`QosClass::default()`], which is
/// [`QosClass::Default`] (the passthrough, "no energy hint" class). The two
/// differ on purpose: `QosClass`'s own default is the neutral member of the
/// enum, whereas reaching for a *`bgrt` runtime* is itself the request for quiet
/// execution — a `RuntimeBuilder` that defaulted to passthrough would do nothing
/// unless configured. Set [`qos`](RuntimeBuilder::qos) explicitly if you want a
/// different class.
///
/// # Examples
///
/// ```
/// use bgrt::{QosClass, RuntimeBuilder};
///
/// let rt = RuntimeBuilder::new()
///     .qos(QosClass::Background)
///     .worker_threads(1)
///     .build()?;
/// let answer = rt.block_on(rt.spawn(async { 21 * 2 }))?;
/// assert_eq!(answer, 42);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct RuntimeBuilder {
    qos: QosClass,
    worker_threads: usize,
    thread_name: String,
    pin_efficiency_cores: bool,
    clamp_frequency: bool,
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self {
            qos: QosClass::Background,
            worker_threads: 1,
            thread_name: "bgrt-worker".to_owned(),
            pin_efficiency_cores: false,
            clamp_frequency: false,
        }
    }
}

impl RuntimeBuilder {
    /// Create a builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the energy [`QosClass`] applied to every runtime thread.
    pub fn qos(mut self, qos: QosClass) -> Self {
        self.qos = qos;
        self
    }

    /// Set the number of worker threads. Values below 1 are treated as 1.
    pub fn worker_threads(mut self, n: usize) -> Self {
        self.worker_threads = n;
        self
    }

    /// Set the name prefix used for the runtime's threads.
    pub fn thread_name(mut self, name: impl Into<String>) -> Self {
        self.thread_name = name.into();
        self
    }

    /// On Linux, also pin runtime threads to detected efficiency cores. No-op on
    /// macOS and Windows (the OS QoS/EcoQoS places work on efficient cores) and
    /// on homogeneous CPUs. Opt-in; off by default.
    pub fn pin_efficiency_cores(mut self, pin: bool) -> Self {
        self.pin_efficiency_cores = pin;
        self
    }

    /// On Linux, also cap runtime threads' CPU frequency via `uclamp` (a
    /// utilization clamp) for the [`QosClass::Background`] class — the only lever
    /// that lowers clocks on homogeneous CPUs, where `nice` leaves frequency
    /// untouched. No-op on macOS/Windows (their QoS/EcoQoS throttle frequency
    /// directly), for other classes, and on kernels or governors without uclamp
    /// support. Opt-in; off by default.
    pub fn clamp_frequency(mut self, clamp: bool) -> Self {
        self.clamp_frequency = clamp;
        self
    }

    /// Build the runtime.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Runtime`] if the underlying Tokio runtime cannot be built.
    pub fn build(self) -> Result<Runtime, Error> {
        let qos = self.qos;
        let clamp_frequency = self.clamp_frequency;
        // tokio panics on a worker count of 0; clamp to keep `build` total.
        let workers = self.worker_threads.max(1);
        let efficiency_cores = if self.pin_efficiency_cores {
            topology::efficiency_cores()
        } else {
            Vec::new()
        };

        let inner = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .thread_name(self.thread_name)
            .on_thread_start(move || {
                // Runs on every runtime thread (workers and the blocking pool).
                // QoS/affinity/clamp are best-effort optimizations: warn, don't abort.
                if let Err(e) = crate::apply(qos) {
                    tracing::warn!(error = %e, "bgrt: failed to apply qos to runtime thread");
                }
                if !efficiency_cores.is_empty() {
                    if let Err(e) = topology::pin_current_thread(&efficiency_cores) {
                        tracing::warn!(error = %e, "bgrt: failed to pin thread to efficiency cores");
                    }
                }
                if clamp_frequency {
                    if let Err(e) = crate::backend::clamp_current_thread(qos) {
                        tracing::warn!(error = %e, "bgrt: failed to clamp thread frequency");
                    }
                }
            })
            .enable_all()
            .build()
            .map_err(Error::Runtime)?;

        Ok(Runtime { inner, qos })
    }
}

/// An energy-classified async runtime.
///
/// Schedule ordinary futures with [`spawn`](Runtime::spawn); they run on worker
/// threads carrying this runtime's [`QosClass`]. Use a second, default-class
/// runtime in the same process for latency-sensitive work.
#[derive(Debug)]
pub struct Runtime {
    inner: TokioRuntime,
    qos: QosClass,
}

impl Runtime {
    /// Spawn a future onto the runtime; it runs on a classified worker thread.
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.inner.spawn(future)
    }

    /// Run a blocking closure on the runtime's blocking pool.
    ///
    /// Blocking-pool threads carry the **same** [`QosClass`] as the async
    /// workers: `on_thread_start` fires for both, and that is deliberate — a
    /// background runtime whose `spawn_blocking` work ran at default priority
    /// would defeat the point, since CPU-bound work is exactly what tends to go
    /// there. There is no separate class for the blocking pool; if you need
    /// blocking work at a different energy class, build a second runtime.
    pub fn spawn_blocking<F, R>(&self, f: F) -> JoinHandle<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.inner.spawn_blocking(f)
    }

    /// Run a future to completion, driving the runtime.
    ///
    /// Note: the future runs on the **calling** thread, which is not classified;
    /// use [`spawn`](Runtime::spawn) for work that should run at this runtime's QoS.
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.inner.block_on(future)
    }

    /// A handle to the underlying Tokio runtime, for APIs that expect one.
    pub fn handle(&self) -> &Handle {
        self.inner.handle()
    }

    /// The energy [`QosClass`] applied to this runtime's threads.
    pub fn qos(&self) -> QosClass {
        self.qos
    }

    /// Shut down the runtime, waiting at most `timeout` for blocking tasks to
    /// finish.
    ///
    /// Dropping a [`Runtime`] waits for blocking tasks *indefinitely*, which for
    /// a background runtime can be a long time — quiet work is slow by design.
    /// Use this to bound that wait. Tasks still running when the timeout expires
    /// are leaked, not cancelled.
    pub fn shutdown_timeout(self, timeout: Duration) {
        self.inner.shutdown_timeout(timeout);
    }

    /// Shut down the runtime without waiting for blocking tasks at all.
    ///
    /// Returns immediately; in-flight blocking work is leaked. The same caveat
    /// as [`shutdown_timeout`](Runtime::shutdown_timeout) applies, more so.
    pub fn shutdown_background(self) {
        self.inner.shutdown_background();
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod runtime_tests;
