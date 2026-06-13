//! Formatting the comparison results as a table or JSON, plus the headline
//! "did the quiet class stay cooler?" check.

use serde::Serialize;

use crate::runner::RunResult;

/// A flat, serializable summary of one executor's run.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// Executor label (e.g. `background`).
    pub executor: String,
    /// Wall-clock time in milliseconds.
    pub wall_ms: u128,
    /// Distinct CPUs the work landed on.
    pub distinct_cpus: usize,
    /// Percent of classified samples on efficiency cores.
    pub efficiency_pct: Option<f64>,
    /// Mean sampled frequency (MHz).
    pub mean_mhz: Option<f64>,
    /// Max sampled frequency (MHz).
    pub max_mhz: Option<u32>,
    /// Energy consumed (joules).
    pub energy_j: Option<f64>,
    /// Number of telemetry samples taken.
    pub samples: u64,
}

impl Summary {
    /// Derive a summary from a raw run result.
    pub fn from_result(r: &RunResult) -> Self {
        let agg = &r.aggregate;
        Self {
            executor: r.executor.label().to_owned(),
            wall_ms: r.wall.as_millis(),
            distinct_cpus: agg.distinct_cpus(),
            efficiency_pct: agg.efficiency_fraction().map(|f| f * 100.0),
            mean_mhz: agg.mean_freq_mhz(),
            max_mhz: agg.max_freq_mhz(),
            energy_j: r.energy_uj.map(|uj| uj as f64 / 1e6),
            samples: agg.samples(),
        }
    }
}

/// Render the summaries as an aligned text table.
pub fn table(summaries: &[Summary]) -> String {
    let header = format!(
        "{:<20} {:>8} {:>5} {:>6} {:>9} {:>8} {:>9} {:>8}",
        "executor", "wall_ms", "cpus", "%E", "mean_mhz", "max_mhz", "energy_j", "samples"
    );
    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    for s in summaries {
        out.push_str(&format!(
            "{:<20} {:>8} {:>5} {:>6} {:>9} {:>8} {:>9} {:>8}\n",
            s.executor,
            s.wall_ms,
            s.distinct_cpus,
            opt(s.efficiency_pct.map(|v| format!("{v:.1}"))),
            opt(s.mean_mhz.map(|v| format!("{v:.0}"))),
            opt(s.max_mhz.map(|v| v.to_string())),
            opt(s.energy_j.map(|v| format!("{v:.3}"))),
            s.samples,
        ));
    }
    out
}

/// Render the summaries as pretty JSON.
///
/// # Errors
///
/// Returns any [`serde_json`] serialization error.
pub fn json(summaries: &[Summary]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(summaries)
}

/// Whether the `background` executor's peak frequency stayed at or below the
/// `default` executor's — the headline "didn't spin up the fans" check.
///
/// Returns `None` if either executor is absent or lacks frequency data (e.g. on
/// macOS without `powermetrics`), so callers can skip rather than fail.
pub fn background_not_hotter(summaries: &[Summary]) -> Option<bool> {
    let find = |label: &str| summaries.iter().find(|s| s.executor == label);
    let bg = find("background")?;
    let def = find("default")?;
    Some(bg.max_mhz? <= def.max_mhz?)
}

fn opt(v: Option<String>) -> String {
    v.unwrap_or_else(|| "n/a".to_owned())
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod report_tests;
