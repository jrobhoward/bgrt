//! Energy quality-of-service classification.

/// How aggressively the OS should optimize a thread for energy efficiency over
/// speed. Applied to a thread via [`crate::apply`].
///
/// A class governs a thread's **resource demands, not only its CPU demands** —
/// block-I/O priority moves with it. That is one knob rather than two because
/// two of the three platforms bundle the axes in a single mechanism: there is no
/// documented way to ask Windows for quiet I/O without also asking for quiet
/// CPU, and macOS's QoS classes carry an I/O policy with them. An API offering
/// combinations it could not honour would be worse than a coarser one that
/// always means what it says.
///
/// Each class maps to native per-OS facilities:
///
/// | Class        | macOS                  | Windows               | Linux                                                        |
/// |--------------|------------------------|-----------------------|--------------------------------------------------------------|
/// | `Background` | `QOS_CLASS_BACKGROUND` | EcoQoS + below-normal | `nice(19)` + I/O best-effort 7 (+ opt-in E-core affinity, `uclamp` cap) |
/// | `Utility`    | `QOS_CLASS_UTILITY`    | EcoQoS + normal       | `nice(10)` + I/O best-effort 6                               |
/// | `Default`    | passthrough            | clear throttling      | `nice(0)`, I/O left alone                                    |
///
/// # I/O coverage is not uniform
///
/// - **macOS** — included automatically. `QOS_CLASS_BACKGROUND` implies disk-I/O
///   throttling; `bgrt` makes no extra call.
/// - **Linux** — an explicit `ioprio_set` to the best-effort class. Whether it
///   *bites* depends on the I/O scheduler: BFQ honours it fully, `mq-deadline`
///   since 5.18, and `none` — a common default for NVMe — ignores it entirely.
///   Inert rather than wrong, like the `uclamp` cap.
/// - **Windows** — **not yet covered.** EcoQoS and thread priority do not touch
///   I/O priority; the documented mechanism that would
///   (`THREAD_MODE_BACKGROUND_BEGIN`) also lowers CPU and memory priority, so it
///   needs measuring against this crate's never-starve requirement before
///   adoption. Tracked in `docs/DESIGN.md`.
///
/// On Linux the base CPU mapping is niceness; efficiency-core affinity and a
/// `uclamp` frequency cap are both opt-in per builder (`pin_efficiency_cores`,
/// `clamp_frequency`). The `uclamp` cap is the only lever that lowers clocks on
/// homogeneous CPUs, where `nice` alone leaves frequency untouched.
///
/// Every `Background` mapping is weighted-fair (not run-only-when-idle), so quiet
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
/// because constructing one is already a request for quiet execution. Don't read
/// `QosClass::default()` as "what `bgrt` does by default".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum QosClass {
    /// Lowest energy: prefer efficiency cores and low clock frequency, while
    /// still making (slow) forward progress under contention. The "fans never"
    /// class; on Apple Silicon it is confined to efficiency cores.
    Background,
    /// Quiet but unconfined: lower priority than normal work, but free to use
    /// performance cores. A middle ground when [`QosClass::Background`] is too
    /// slow (notably on macOS, where `Background` is E-core-jailed).
    Utility,
    /// No energy hint — ordinary OS scheduling.
    #[default]
    Default,
}
