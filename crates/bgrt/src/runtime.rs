//! An energy-classified async runtime that wraps a Tokio runtime.
//!
//! Build one with [`RuntimeBuilder`]; every thread the runtime spawns (workers
//! and the blocking pool) has the configured [`QosClass`] applied at start, so
//! all work scheduled onto it runs at the chosen energy footprint.
//!
//! # Multi-thread vs. current-thread
//!
//! By default the runtime uses Tokio's multi-thread scheduler with one worker.
//! That already costs exactly one thread — Tokio drives I/O and timers on the
//! worker itself, with no extra driver thread — so it is the ordinary
//! single-quiet-worker configuration.
//!
//! [`RuntimeBuilder::current_thread`] switches to Tokio's *current-thread*
//! scheduler, which `bgrt` runs on **one OS thread that it spawns and
//! classifies itself** — never on the caller's thread. That distinction is the
//! whole design. A plain Tokio current-thread runtime drives its tasks on
//! whichever thread calls `block_on`, and `bgrt` must not reclassify that
//! thread: it does not own it, and on Linux an unprivileged thread can lower
//! its niceness but never raise it back, so classifying the caller would
//! permanently deprioritize a thread that belongs to someone else. Worse, the
//! `on_thread_start` hook fires only for blocking-pool threads on a
//! current-thread runtime, so the async work would silently run *unclassified*
//! — the exact opposite of the point. Owning the driver thread is what makes
//! the energy guarantee hold in that mode.

use std::future::Future;
use std::sync::mpsc;
use std::time::Duration;

use tokio::runtime::{Handle, Runtime as TokioRuntime};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::error::Error;
use crate::qos::QosClass;
use crate::thread::{ThreadBuilder, classify};
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
    current_thread: bool,
}

impl Default for RuntimeBuilder {
    fn default() -> Self {
        Self {
            qos: QosClass::Background,
            worker_threads: 1,
            thread_name: "bgrt-worker".to_owned(),
            pin_efficiency_cores: false,
            clamp_frequency: false,
            current_thread: false,
        }
    }
}

impl RuntimeBuilder {
    /// Create a builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the energy [`QosClass`] applied to every runtime thread.
    #[must_use]
    pub fn qos(mut self, qos: QosClass) -> Self {
        self.qos = qos;
        self
    }

    /// Set the number of worker threads. Values below 1 are treated as 1.
    /// Ignored when [`current_thread`](RuntimeBuilder::current_thread) is set.
    #[must_use]
    pub fn worker_threads(mut self, n: usize) -> Self {
        self.worker_threads = n;
        self
    }

    /// Use Tokio's current-thread scheduler, driven on a single OS thread that
    /// `bgrt` spawns and classifies. Off by default (multi-thread scheduler).
    ///
    /// **Despite the name — which follows Tokio's scheduler naming — tasks do
    /// not run on the calling thread.** `bgrt` owns the driver thread, because
    /// that is the only way the energy class can be guaranteed: a plain Tokio
    /// current-thread runtime would drive tasks on the caller's thread, and
    /// `bgrt` must never reclassify a thread it did not create (on Linux an
    /// unprivileged thread can lower its niceness but not raise it back). The
    /// observable differences from the default multi-thread mode are:
    ///
    /// - Every task runs on that one thread, so `spawn`ed futures need not be
    ///   `Send` between workers — but a task that blocks stalls all the others.
    /// - [`worker_threads`](RuntimeBuilder::worker_threads) is ignored.
    /// - [`block_on`](Runtime::block_on) goes through the runtime handle; the
    ///   future is still polled on the *calling* thread, while spawned tasks and
    ///   the I/O and timer drivers run on the owned thread.
    /// - The thread exists for the runtime's whole lifetime and is joined when
    ///   the [`Runtime`] is dropped or shut down.
    ///
    /// Prefer the default (`worker_threads(1)`) unless you specifically want
    /// single-threaded task semantics: it costs the same one thread.
    ///
    /// # Deadlock hazard on drop
    ///
    /// Because `bgrt` owns the driver thread, dropping (or shutting down) a
    /// current-thread [`Runtime`] **joins** that thread. Doing so from inside
    /// the runtime's own blocking pool therefore deadlocks, where a plain Tokio
    /// runtime would instead panic with "Cannot drop a runtime in a context
    /// where blocking is not allowed". Drop the [`Runtime`] from the thread that
    /// built it, or from any thread it does not own.
    ///
    /// # Examples
    ///
    /// ```
    /// use bgrt::{QosClass, RuntimeBuilder};
    ///
    /// let rt = RuntimeBuilder::new()
    ///     .qos(QosClass::Background)
    ///     .current_thread(true)
    ///     .build()?;
    /// let answer = rt.block_on(rt.spawn(async { 21 * 2 }))?;
    /// assert_eq!(answer, 42);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn current_thread(mut self, current_thread: bool) -> Self {
        self.current_thread = current_thread;
        self
    }

    /// Set the name prefix used for the runtime's threads.
    #[must_use]
    pub fn thread_name(mut self, name: impl Into<String>) -> Self {
        self.thread_name = name.into();
        self
    }

    /// On Linux, also pin runtime threads to detected efficiency cores. No-op on
    /// macOS and Windows (the OS QoS/EcoQoS places work on efficient cores) and
    /// on homogeneous CPUs. Opt-in; off by default.
    #[must_use]
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
    #[must_use]
    pub fn clamp_frequency(mut self, clamp: bool) -> Self {
        self.clamp_frequency = clamp;
        self
    }

    /// Build the runtime.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Runtime`] if the underlying Tokio runtime cannot be
    /// built, or — in [`current_thread`](RuntimeBuilder::current_thread) mode —
    /// if the OS refuses to create the driver thread.
    pub fn build(self) -> Result<Runtime, Error> {
        if self.current_thread {
            self.build_current_thread()
        } else {
            self.build_multi_thread()
        }
    }

    /// The E-cores to pin to, looked up once on the spawning thread.
    fn efficiency_cores(&self) -> Vec<usize> {
        if self.pin_efficiency_cores {
            topology::efficiency_cores()
        } else {
            Vec::new()
        }
    }

    fn build_multi_thread(self) -> Result<Runtime, Error> {
        let qos = self.qos;
        let clamp_frequency = self.clamp_frequency;
        // tokio panics on a worker count of 0; clamp to keep `build` total.
        let workers = self.worker_threads.max(1);
        let efficiency_cores = self.efficiency_cores();

        let inner = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .thread_name(self.thread_name)
            // Fires on every runtime thread here: workers and the blocking pool.
            .on_thread_start(move || classify(qos, &efficiency_cores, clamp_frequency))
            .enable_all()
            .build()
            .map_err(Error::Runtime)?;

        let handle = inner.handle().clone();
        Ok(Runtime {
            inner: Inner::MultiThread(inner),
            handle,
            qos,
        })
    }

    fn build_current_thread(self) -> Result<Runtime, Error> {
        let qos = self.qos;
        let clamp_frequency = self.clamp_frequency;
        let efficiency_cores = self.efficiency_cores();

        // The driver thread reports its handle — or the build error — back here,
        // so `build` stays fallible instead of handing out a dead runtime.
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<Handle, std::io::Error>>(1);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<ShutdownMode>();

        // Spawning through `ThreadBuilder` is what classifies the driver thread,
        // and it resolves qos/pin/clamp exactly as every other bgrt thread does.
        let join = ThreadBuilder::new()
            .qos(qos)
            .name(self.thread_name)
            .pin_efficiency_cores(self.pin_efficiency_cores)
            .clamp_frequency(clamp_frequency)
            .spawn(move || {
                drive(
                    qos,
                    efficiency_cores,
                    clamp_frequency,
                    ready_tx,
                    shutdown_rx,
                );
            })
            .map_err(Error::Runtime)?;

        let handle = match ready_rx.recv() {
            Ok(Ok(handle)) => handle,
            Ok(Err(e)) => return Err(Error::Runtime(e)),
            // The sender was dropped without a message: the driver thread died
            // before it could report, which means it panicked.
            Err(_) => {
                return Err(Error::Runtime(std::io::Error::other(
                    "bgrt: runtime driver thread exited before reporting readiness",
                )));
            }
        };

        Ok(Runtime {
            inner: Inner::Dedicated(Dedicated {
                shutdown: Some(shutdown_tx),
                join: Some(join),
            }),
            handle,
            qos,
        })
    }
}

/// How a dedicated driver thread should tear its runtime down. Mirrors the
/// three Tokio shutdown behaviours, sent across the thread boundary because the
/// runtime is owned by the driver, not by the [`Runtime`] handle.
#[derive(Debug)]
enum ShutdownMode {
    /// Drop the runtime, waiting for blocking tasks (plain `drop` semantics).
    Wait,
    /// Wait at most this long for blocking tasks.
    Timeout(Duration),
    /// Don't wait at all.
    Background,
}

/// Body of the dedicated driver thread: build a current-thread runtime, publish
/// its handle, then drive it until a shutdown mode arrives.
///
/// This thread has already been classified by [`ThreadBuilder`]; the
/// `on_thread_start` hook installed here covers the blocking pool, which is the
/// only thing it fires for on a current-thread runtime.
fn drive(
    qos: QosClass,
    efficiency_cores: Vec<usize>,
    clamp_frequency: bool,
    ready: mpsc::SyncSender<Result<Handle, std::io::Error>>,
    shutdown: oneshot::Receiver<ShutdownMode>,
) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .on_thread_start(move || classify(qos, &efficiency_cores, clamp_frequency))
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    if ready.send(Ok(rt.handle().clone())).is_err() {
        return; // The builder gave up before we were ready; nothing to drive.
    }

    // Parking here is what drives spawned tasks and the I/O and timer drivers
    // for the runtime's whole lifetime. A dropped sender means the `Runtime` was
    // leaked rather than shut down; treat that as the ordinary drop path.
    let mode = rt.block_on(async move { shutdown.await.unwrap_or(ShutdownMode::Wait) });
    match mode {
        ShutdownMode::Wait => drop(rt),
        ShutdownMode::Timeout(timeout) => rt.shutdown_timeout(timeout),
        ShutdownMode::Background => rt.shutdown_background(),
    }
}

/// Handle onto a dedicated driver thread and its shutdown channel.
#[derive(Debug)]
struct Dedicated {
    shutdown: Option<oneshot::Sender<ShutdownMode>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Dedicated {
    /// Tell the driver thread how to tear down and hand back its join handle, so
    /// the caller decides whether to wait. Idempotent: a second call is a no-op,
    /// which is what makes the explicit `shutdown_*` methods safe to combine
    /// with [`Drop`].
    fn stop(&mut self, mode: ShutdownMode) -> Option<std::thread::JoinHandle<()>> {
        if let Some(tx) = self.shutdown.take() {
            // An error means the driver thread is already gone — fine either way.
            let _ = tx.send(mode);
        }
        self.join.take()
    }
}

impl Drop for Dedicated {
    fn drop(&mut self) {
        if let Some(join) = self.stop(ShutdownMode::Wait) {
            let _ = join.join();
        }
    }
}

/// An energy-classified async runtime.
///
/// Schedule ordinary futures with [`spawn`](Runtime::spawn); they run on worker
/// threads carrying this runtime's [`QosClass`]. Use a second, default-class
/// runtime in the same process for latency-sensitive work.
#[derive(Debug)]
pub struct Runtime {
    inner: Inner,
    /// Cloned up front so `handle()` works the same in both modes — in
    /// current-thread mode the runtime itself lives on the driver thread.
    handle: Handle,
    qos: QosClass,
}

/// Where the wrapped Tokio runtime actually lives.
#[derive(Debug)]
enum Inner {
    /// Owned here; its worker threads classify themselves on start.
    MultiThread(TokioRuntime),
    /// Owned by a `bgrt`-spawned, classified driver thread.
    Dedicated(Dedicated),
}

impl Runtime {
    /// Spawn a future onto the runtime; it runs on a classified worker thread.
    pub fn spawn<F>(&self, future: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.handle.spawn(future)
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
        self.handle.spawn_blocking(f)
    }

    /// Run a future to completion, driving the runtime.
    ///
    /// Note: the future runs on the **calling** thread, which is not classified;
    /// use [`spawn`](Runtime::spawn) for work that should run at this runtime's QoS.
    ///
    /// # Panics
    ///
    /// Panics if `future` panics, or if called from within an asynchronous
    /// execution context — including from inside a task on this runtime or any
    /// other. This is Tokio's behaviour, passed through unchanged in both
    /// scheduler modes (`Runtime::block_on` for the default multi-thread
    /// scheduler, `Handle::block_on` in
    /// [`current_thread`](RuntimeBuilder::current_thread) mode). To await from
    /// inside async code, use `.await` on the [`spawn`](Runtime::spawn) handle
    /// rather than nesting `block_on`.
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        match &self.inner {
            Inner::MultiThread(rt) => rt.block_on(future),
            // The driver thread owns the runtime, so go through the handle. That
            // is only sound because the driver keeps the I/O and timer drivers
            // running for the runtime's whole lifetime.
            Inner::Dedicated(_) => self.handle.block_on(future),
        }
    }

    /// A handle to the underlying Tokio runtime, for APIs that expect one.
    #[must_use]
    pub fn handle(&self) -> &Handle {
        &self.handle
    }

    /// The energy [`QosClass`] applied to this runtime's threads.
    #[must_use]
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
        match self.inner {
            Inner::MultiThread(rt) => rt.shutdown_timeout(timeout),
            Inner::Dedicated(mut dedicated) => {
                // The driver thread applies the timeout to the runtime it owns,
                // so joining it here still returns within roughly `timeout`.
                if let Some(join) = dedicated.stop(ShutdownMode::Timeout(timeout)) {
                    let _ = join.join();
                }
            }
        }
    }

    /// Shut down the runtime without waiting for blocking tasks at all.
    ///
    /// Returns immediately; in-flight blocking work is leaked. The same caveat
    /// as [`shutdown_timeout`](Runtime::shutdown_timeout) applies, more so.
    pub fn shutdown_background(self) {
        match self.inner {
            Inner::MultiThread(rt) => rt.shutdown_background(),
            // Signal and detach: joining would reintroduce the wait.
            Inner::Dedicated(mut dedicated) => {
                let _ = dedicated.stop(ShutdownMode::Background);
            }
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod runtime_tests;
