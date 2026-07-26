//! Error types for `bgrt`.

use thiserror::Error;

/// Errors returned by `bgrt`.
///
/// Every variant preserves its underlying cause as [`Error::source`], so a
/// caller can inspect *why* a call failed rather than parse a message. The
/// common case — distinguishing "this kernel doesn't support the feature" from
/// a real failure — is served directly by [`Error::raw_os_error`]:
///
/// ```
/// # fn demo(e: &bgrt::Error) {
/// // Compare against a known errno (e.g. `libc::ENOSYS`) to treat a missing
/// // kernel feature as tolerable, while still surfacing real failures.
/// match e.raw_os_error() {
///     Some(code) => eprintln!("os error {code}"),
///     None => eprintln!("not an os failure: {e}"),
/// }
/// # }
/// ```
///
/// [`Error::source`]: std::error::Error::source
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// An operating-system call failed while applying a QoS class.
    #[error("failed to apply energy qos: {syscall}")]
    Backend {
        /// The OS call that failed, e.g. `"setpriority"`. Static, so matching on
        /// it is possible but discouraged — prefer [`Error::raw_os_error`].
        syscall: &'static str,
        /// The underlying OS error.
        #[source]
        source: std::io::Error,
    },

    /// The underlying Tokio runtime could not be built.
    #[cfg(feature = "tokio")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
    #[error("failed to build runtime")]
    Runtime(#[source] std::io::Error),

    /// A rayon thread pool could not be built.
    ///
    /// The cause is boxed rather than typed as `rayon::ThreadPoolBuildError` so
    /// that this enum does not change shape when rayon reshapes its error type.
    /// Downcast the source if you need the concrete type.
    ///
    /// Note this does **not** keep rayon out of `bgrt`'s public API, and is not
    /// meant to: [`RayonPool`](crate::RayonPool) derefs to `rayon::ThreadPool`,
    /// just as [`Runtime`](crate::Runtime) hands back Tokio's `JoinHandle` and
    /// `Handle`. Wrapping those runtimes is the point of the crate. See the
    /// crate-level *Semver and wrapped dependencies* section.
    #[cfg(feature = "rayon")]
    #[cfg_attr(docsrs, doc(cfg(feature = "rayon")))]
    #[error("failed to build thread pool")]
    ThreadPool(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// The raw OS error code behind this error, if it came from a failing
    /// syscall.
    ///
    /// Returns `None` for errors that did not originate in the OS (currently
    /// only [`Error::ThreadPool`]). On Windows this is the Win32 error code; on
    /// Unix, the `errno` value.
    #[must_use]
    pub fn raw_os_error(&self) -> Option<i32> {
        match self {
            Error::Backend { source, .. } => source.raw_os_error(),
            #[cfg(feature = "tokio")]
            Error::Runtime(source) => source.raw_os_error(),
            #[cfg(feature = "rayon")]
            Error::ThreadPool(_) => None,
        }
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;
