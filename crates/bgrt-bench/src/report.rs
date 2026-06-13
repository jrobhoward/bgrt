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
    /// Total work units completed.
    pub work_units: u64,
    /// Work units per second (throughput).
    pub throughput_per_s: f64,
    /// Distinct CPUs the work landed on (self-sampling).
    pub distinct_cpus: usize,
    /// Percent of activity on efficiency cores.
    pub efficiency_pct: Option<f64>,
    /// Mean frequency (MHz).
    pub mean_mhz: Option<f64>,
    /// Max frequency (MHz).
    pub max_mhz: Option<u32>,
    /// Energy consumed (joules).
    pub energy_j: Option<f64>,
    /// Mean CPU power (milliwatts), macOS powermetrics only.
    pub cpu_power_mw: Option<f64>,
    /// Number of telemetry self-samples taken.
    pub samples: u64,
}

impl Summary {
    /// Derive a summary from a raw run result. Where macOS `powermetrics` stats
    /// are present they take precedence over the (unavailable-on-macOS)
    /// self-sampled placement/frequency/energy.
    pub fn from_result(r: &RunResult) -> Self {
        let agg = &r.aggregate;
        let wall_s = r.wall.as_secs_f64();
        let throughput_per_s = if wall_s > 0.0 {
            r.work_units as f64 / wall_s
        } else {
            0.0
        };

        let mut efficiency_pct = agg.efficiency_fraction().map(|f| f * 100.0);
        let mut mean_mhz = agg.mean_freq_mhz();
        let mut max_mhz = agg.max_freq_mhz();
        let mut energy_j = r.energy_uj.map(|uj| uj as f64 / 1e6);
        let mut cpu_power_mw = None;

        if let Some(p) = &r.power {
            efficiency_pct = p.efficiency_pct().or(efficiency_pct);
            mean_mhz = p.mean_freq_mhz().or(mean_mhz);
            max_mhz = p.max_freq_mhz().or(max_mhz);
            energy_j = p.energy_j(r.wall).or(energy_j);
            cpu_power_mw = p.cpu_power_mw();
        }

        Self {
            executor: r.executor.label().to_owned(),
            wall_ms: r.wall.as_millis(),
            work_units: r.work_units,
            throughput_per_s,
            distinct_cpus: agg.distinct_cpus(),
            efficiency_pct,
            mean_mhz,
            max_mhz,
            energy_j,
            cpu_power_mw,
            samples: agg.samples(),
        }
    }
}

/// Render the summaries as an aligned text table.
pub fn table(summaries: &[Summary]) -> String {
    let header = format!(
        "{:<20} {:>8} {:>10} {:>11} {:>6} {:>9} {:>8} {:>9}",
        "executor", "wall_ms", "work", "work/s", "%E", "mean_mhz", "max_mhz", "energy_j"
    );
    let mut out = String::new();
    out.push_str(&header);
    out.push('\n');
    for s in summaries {
        out.push_str(&format!(
            "{:<20} {:>8} {:>10} {:>11.0} {:>6} {:>9} {:>8} {:>9}\n",
            s.executor,
            s.wall_ms,
            s.work_units,
            s.throughput_per_s,
            opt(s.efficiency_pct.map(|v| format!("{v:.1}"))),
            opt(s.mean_mhz.map(|v| format!("{v:.0}"))),
            opt(s.max_mhz.map(|v| v.to_string())),
            opt(s.energy_j.map(|v| format!("{v:.3}"))),
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
