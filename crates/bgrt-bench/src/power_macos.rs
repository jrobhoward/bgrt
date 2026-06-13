//! Optional macOS power/frequency sampling via `powermetrics` (requires root).
//!
//! `powermetrics` is the only way to read CPU power, per-cluster frequency, and
//! E/P residency on Apple Silicon, and it needs `sudo`. A [`Sampler`] runs it as
//! a child process for the duration of an executor's run; parsing is delegated to
//! the cross-platform [`crate::power::PowerStats`]. Best-effort: any failure (not
//! root, binary missing, unparseable output) yields `None`.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::power::PowerStats;

/// Sample interval handed to `powermetrics`.
const INTERVAL_MS: u64 = 200;

/// A running `powermetrics` child capturing CPU power for one executor run.
pub struct Sampler {
    child: Child,
}

impl Sampler {
    /// Start sampling for roughly `duration`. Returns `None` if `powermetrics`
    /// cannot be spawned.
    pub fn start(duration: Duration) -> Option<Self> {
        let count = u64::try_from(duration.as_millis())
            .unwrap_or(u64::MAX)
            .div_ceil(INTERVAL_MS)
            .max(1);
        let child = Command::new("powermetrics")
            .args([
                "--samplers",
                "cpu_power",
                "-i",
                &INTERVAL_MS.to_string(),
                "-n",
                &count.to_string(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        Some(Self { child })
    }

    /// Wait for sampling to finish and parse the result, or `None` on any failure
    /// (most commonly: not run under `sudo`).
    pub fn finish(self) -> Option<PowerStats> {
        let output = self.child.wait_with_output().ok()?;
        if !output.status.success() {
            return None;
        }
        Some(PowerStats::parse(&String::from_utf8_lossy(&output.stdout)))
    }
}
