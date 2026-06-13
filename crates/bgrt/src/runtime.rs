//! An energy-classified async runtime that wraps a Tokio runtime.
//!
//! Build one with [`Builder`]; every thread the runtime spawns (workers and the
//! blocking pool) has the configured [`QosClass`] applied at start, so all work
//! scheduled onto it runs at the chosen energy footprint.

use std::future::Future;

use tokio::runtime::{Handle, Runtime as TokioRuntime};
use tokio::task::JoinHandle;

use crate::error::Error;
use crate::qos::QosClass;
use crate::topology;

/// Builder for an energy-classified [`Runtime`].
///
/// Defaults: [`QosClass::Background`], one worker thread, efficiency-core pinning
/// off.
#[derive(Debug, Clone)]
pub struct Builder {
    qos: QosClass,
    worker_threads: usize,
    thread_name: String,
    pin_efficiency_cores: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            qos: QosClass::Background,
            worker_threads: 1,
            thread_name: "bgrt-worker".to_owned(),
            pin_efficiency_cores: false,
        }
    }
}

impl Builder {
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

    /// Build the runtime.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Runtime`] if the underlying Tokio runtime cannot be built.
    pub fn build(self) -> Result<Runtime, Error> {
        let qos = self.qos;
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
                // QoS/affinity are best-effort optimizations: warn, don't abort.
                if let Err(e) = crate::apply(qos) {
                    tracing::warn!(error = %e, "bgrt: failed to apply qos to runtime thread");
                }
                if !efficiency_cores.is_empty() {
                    if let Err(e) = topology::pin_current_thread(&efficiency_cores) {
                        tracing::warn!(error = %e, "bgrt: failed to pin thread to efficiency cores");
                    }
                }
            })
            .enable_all()
            .build()
            .map_err(|e| Error::Runtime(e.to_string()))?;

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

    /// Run a blocking closure on the runtime's (classified) blocking pool.
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
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod runtime_tests;
