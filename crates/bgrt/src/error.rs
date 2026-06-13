//! Error types for `bgrt`.

use thiserror::Error;

/// Errors returned by `bgrt`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// An operating-system backend call failed while applying a QoS class.
    #[error("failed to apply energy qos: {0}")]
    Backend(String),

    /// The underlying Tokio runtime could not be built.
    #[error("failed to build runtime: {0}")]
    Runtime(String),
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod error_tests;
