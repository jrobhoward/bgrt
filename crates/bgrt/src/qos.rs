//! Energy quality-of-service classification.

/// How aggressively the OS should optimize a thread for energy efficiency over
/// speed. Applied to a thread via [`crate::apply`].
///
/// A class governs a thread's resource demands, not only its CPU demands —
/// block-I/O priority moves with it. That is one knob rather than two because
/// two of the three platforms bundle the axes in a single mechanism: there is no
/// documented way to ask Windows for low-priority I/O without also lowering
/// CPU, and macOS's QoS classes carry an I/O policy with them. An API offering
/// combinations it could not honour would be worse than a coarser one that
/// always means what it says.
///
/// Each class maps to native per-OS facilities:
///
/// | Class        | macOS                  | Windows               | Linux                                                        |
/// |--------------|------------------------|-----------------------|--------------------------------------------------------------|
/// | `Background` | `QOS_CLASS_BACKGROUND` | background mode + EcoQoS + below-normal | `nice(19)` + I/O best-effort 7 (+ opt-in E-core affinity, `uclamp` cap) |
/// | `Utility`    | `QOS_CLASS_UTILITY`    | EcoQoS + normal       | `nice(10)` + I/O best-effort 6                               |
/// | `Default`    | passthrough            | clear throttling      | `nice(0)`, I/O left alone                                    |
///
/// # I/O coverage is not uniform
///
/// - **macOS** — included automatically. `QOS_CLASS_BACKGROUND` implies disk-I/O
///   throttling; `bgrt` makes no extra call, and cannot: setting an explicit
///   thread I/O policy permanently opts the thread out of QoS, costing E-core
///   confinement. Darwin's bundling is the mechanism, not a coincidence.
/// - **Linux** — an explicit `ioprio_set` to the best-effort class. Whether it
///   *bites* depends on the I/O scheduler: BFQ honours it. `mq-deadline` does
///   not, because it separates priority classes but ignores the level within
///   best-effort, and `none` — a common default for NVMe — ignores priority
///   entirely. It does nothing there rather than something wrong, like the
///   `uclamp` cap.
/// - **Windows** — `Background` only, via background processing mode
///   (`THREAD_MODE_BACKGROUND_BEGIN`), the sole documented per-thread I/O lever.
///   `Utility` gets no I/O reduction: the mode is all-or-nothing and would drag
///   CPU priority down with it, which is what separates `Utility` from
///   `Background`. Microsoft documents that a background-mode thread "may not be
///   scheduled promptly, but it will never be starved" — weaker than Linux's
///   weighted-fair share, so expect low throughput under sustained foreground
///   load.
///
/// # Classification is not reversible on Linux
///
/// [`QosClass::Default`] restores a thread on macOS and Windows, but on Linux it
/// cannot: `nice` is one-way for an unprivileged thread (`RLIMIT_NICE` defaults
/// to 0, putting the floor at the thread's current value), so the kernel refuses
/// to lower a nice value that `Background` or `Utility` already raised. The same
/// applies to a process started already niced — `nice -n 10 …`, systemd `Nice=`
/// — where even a fresh thread starts above 0.
///
/// `bgrt` treats that refusal as a no-op rather than an error: [`crate::apply`]
/// returns `Ok`, and the thread keeps the niceness it had. Classify a thread
/// once, at creation, and use a second runtime or pool rather than expecting to
/// reclassify — which is what the builders' thread hooks are for.
///
/// # Threads spawned by classified threads
///
/// A class applies to the thread it was applied to — *not* to threads that
/// code running on it goes on to create. Whether a child inherits is an OS
/// decision, and the platforms disagree: Linux inherits (`nice` and I/O priority
/// are copied by `clone()`), macOS and Windows do not (a child reports
/// `QOS_CLASS_DEFAULT` / `THREAD_PRIORITY_NORMAL`). Nor can it be fixed
/// afterwards on those two — their classification APIs act only on the *calling*
/// thread. Hand libraries a `bgrt` runtime or pool where possible; see the README
/// for the full table and the workarounds.
///
/// On Linux the base CPU mapping is niceness; efficiency-core affinity and a
/// `uclamp` frequency cap are both opt-in per builder (`pin_efficiency_cores`,
/// `clamp_frequency`). The `uclamp` cap is the only lever that lowers clocks on
/// homogeneous CPUs, where `nice` alone leaves frequency untouched.
///
/// Every `Background` mapping is weighted-fair (not run-only-when-idle), so low-priority
/// work still makes forward progress under contention rather than starving. That
/// applies to the I/O half too: Linux uses best-effort level 7, deliberately not
/// `IOPRIO_CLASS_IDLE`, for the same reason it uses `nice(19)` and not
/// `SCHED_IDLE`.
///
/// # Default
///
/// `QosClass::default()` is [`QosClass::Default`] — the neutral, no-hint class,
/// matching the name. The *builders* deliberately differ: `RuntimeBuilder`,
/// `RayonBuilder`, and `ThreadBuilder` all default to [`QosClass::Background`],
/// because constructing one is already a request for low-priority execution. Do not read
/// `QosClass::default()` as "what `bgrt` does by default".
///
/// # Stability
///
/// This enum is `#[non_exhaustive]`: matching on it from outside `bgrt` needs a
/// `_` arm. The three classes cover what all three platforms express in common,
/// and the crate's promise to only ever *lower* a thread's demands rules out the
/// obvious additions (macOS's `USER_INITIATED` and `USER_INTERACTIVE` both raise
/// priority). A fourth class is therefore unlikely — but `#[non_exhaustive]`
/// keeps it a minor release rather than a major one if a platform ever exposes a
/// rung between these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum QosClass {
    /// Lowest energy: prefer efficiency cores and low clock frequency, while
    /// still making (slow) forward progress under contention. The "fans never"
    /// class; on Apple Silicon it is confined to efficiency cores.
    Background,
    /// Lowered but unconfined: below normal priority, but free to use
    /// performance cores. A middle ground when [`QosClass::Background`] is too
    /// slow (on macOS especially, where `Background` is confined to E-cores).
    Utility,
    /// No energy hint — ordinary OS scheduling.
    #[default]
    Default,
}
