//! Energy quality-of-service classification.

/// How aggressively the OS should optimize a thread for energy efficiency over
/// speed. Applied to a thread via [`crate::apply`].
///
/// Each class maps to a native per-OS facility (see `docs/ROADMAP.md`):
///
/// | Class        | macOS                  | Windows               | Linux                              |
/// |--------------|------------------------|-----------------------|------------------------------------|
/// | `Background` | `QOS_CLASS_BACKGROUND` | EcoQoS + below-normal | `nice(19)` (+ opt-in E-core affinity) |
/// | `Utility`    | `QOS_CLASS_UTILITY`    | EcoQoS + normal       | `nice(10)`                         |
/// | `Default`    | passthrough            | clear throttling      | `nice(0)`                          |
///
/// Efficiency-core affinity is opt-in and not yet wired up (see the runtime
/// builder, a later phase); today the Linux mapping is niceness only.
///
/// Every `Background` mapping is weighted-fair (not run-only-when-idle), so quiet
/// work still makes forward progress under contention rather than starving.
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
