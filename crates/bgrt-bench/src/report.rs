//! Formatting the comparison results as a table or JSON, plus the
//! "did the low-priority class stay cooler?" and "did it yield the disk?" checks.
//! Both checks allow a tolerance band, so equal rows read as "no difference"
//! rather than as a win or a loss decided by noise.

use serde::Serialize;

use crate::io_file::CacheBypass;
use crate::io_runner::IoRunResult;
use crate::io_workload::PhaseStats;
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

/// How the `background` executor compared with `default` on a verdict's measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Background did better by more than the tolerance.
    Better,
    /// Within the tolerance: no difference worth reporting.
    Same,
    /// Background did worse by more than the tolerance.
    Worse,
}

impl Verdict {
    /// Classify `advantage` (positive means background did better) against a
    /// symmetric tolerance band.
    fn from_advantage(advantage: f64, tolerance: f64) -> Self {
        if advantage > tolerance {
            Self::Better
        } else if advantage < -tolerance {
            Self::Worse
        } else {
            Self::Same
        }
    }
}

/// Mean-frequency difference, as a percentage of `default`'s, that counts as
/// "the same". Short runs at the same clock can differ by one frequency step
/// (3592 against 3692 MHz on a Threadripper, about 2.7%); every clamp effect
/// measured so far is 14% or more.
pub const FREQUENCY_TOLERANCE_PCT: f64 = 5.0;

/// Whether the `background` executor ran at a lower mean frequency than
/// `default` — the "didn't spin up the fans" check.
///
/// Compares means, not peaks: a single early sample taken before the governor
/// reacts can match `default`'s peak even when a clamp holds the rest of the
/// run 37% lower. Returns `None` if either executor is absent or lacks
/// frequency data (e.g. on macOS without `powermetrics`), so callers can skip
/// rather than fail.
pub fn frequency_verdict(summaries: &[Summary]) -> Option<Verdict> {
    let find = |label: &str| summaries.iter().find(|s| s.executor == label);
    let bg = find("background")?.mean_mhz?;
    let def = find("default")?.mean_mhz?;
    if def <= 0.0 {
        return None;
    }
    Some(Verdict::from_advantage(
        (def - bg) / def * 100.0,
        FREQUENCY_TOLERANCE_PCT,
    ))
}

fn opt(v: Option<String>) -> String {
    v.unwrap_or_else(|| "n/a".to_owned())
}

// --- disk ------------------------------------------------------------------

/// A flat, serializable summary of one executor's disk run.
#[derive(Debug, Clone, Serialize)]
pub struct IoSummary {
    /// Executor label (e.g. `background`).
    pub executor: String,
    /// Executor throughput reading alone (MiB/s).
    pub solo_mib_s: f64,
    /// Executor throughput while the foreground threads read (MiB/s).
    pub contended_mib_s: f64,
    /// Foreground throughput during that contended window (MiB/s).
    pub foreground_mib_s: f64,
    /// Foreground throughput as a percentage of its uncontended baseline — the
    /// "did the low-priority class get out of the way?" figure. `None` if the
    /// baseline measured nothing.
    pub foreground_protection_pct: Option<f64>,
    /// Executor reads per second while alone.
    pub solo_iops: f64,
    /// Median executor read latency under contention (µs).
    pub p50_us: Option<u32>,
    /// 95th-percentile executor read latency under contention (µs).
    pub p95_us: Option<u32>,
    /// 99th-percentile executor read latency under contention (µs).
    pub p99_us: Option<u32>,
    /// Executor reads completed under contention.
    pub reads: u64,
    /// Failed reads across both phases.
    pub errors: u64,
    /// Whether these reads reached the device or may have been served from cache.
    pub cache_bypass: CacheBypass,
}

impl IoSummary {
    /// Derive a summary from a raw disk result, given the foreground baseline
    /// every executor is measured against.
    pub fn from_result(r: &IoRunResult, baseline: &PhaseStats) -> Self {
        let base_mib_s = baseline.mib_per_s();
        let foreground_mib_s = r.foreground.mib_per_s();
        let foreground_protection_pct =
            (base_mib_s > 0.0).then(|| foreground_mib_s / base_mib_s * 100.0);

        Self {
            executor: r.executor.label().to_owned(),
            solo_mib_s: r.solo.mib_per_s(),
            contended_mib_s: r.contended.mib_per_s(),
            foreground_mib_s,
            foreground_protection_pct,
            solo_iops: r.solo.iops(),
            p50_us: r.contended.p50_us,
            p95_us: r.contended.p95_us,
            p99_us: r.contended.p99_us,
            reads: r.contended.reads,
            errors: r.solo.errors.saturating_add(r.contended.errors),
            cache_bypass: r.solo.bypass.merge(r.contended.bypass),
        }
    }
}

/// The disk report: the shared baseline plus one row per executor.
#[derive(Debug, Clone, Serialize)]
pub struct IoReport {
    /// Foreground throughput with nothing else running (MiB/s).
    pub foreground_baseline_mib_s: f64,
    /// Whether the baseline reads reached the device.
    pub cache_bypass: CacheBypass,
    /// Active Linux I/O scheduler for the scratch file's device, where known —
    /// `none` ignores I/O priority entirely.
    pub io_scheduler: Option<String>,
    /// One row per executor.
    pub rows: Vec<IoSummary>,
}

/// Render the disk report as an aligned text table.
pub fn io_table(report: &IoReport) -> String {
    let mut out = format!(
        "{:<20} {:>11} {:>11} {:>11} {:>9} {:>9}\n",
        "executor", "solo_mib/s", "cont_mib/s", "fg_mib/s", "fg_prot%", "p95_us"
    );
    for s in &report.rows {
        out.push_str(&format!(
            "{:<20} {:>11.1} {:>11.1} {:>11.1} {:>9} {:>9}\n",
            s.executor,
            s.solo_mib_s,
            s.contended_mib_s,
            s.foreground_mib_s,
            opt(s.foreground_protection_pct.map(|v| format!("{v:.1}"))),
            opt(s.p95_us.map(|v| v.to_string())),
        ));
    }
    out
}

/// Render the disk report as pretty JSON.
///
/// # Errors
///
/// Returns any [`serde_json`] serialization error.
pub fn io_json(report: &IoReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(report)
}

/// Both reports in one JSON document, for `--workload both --format json`.
#[derive(Debug, Serialize)]
struct Combined<'a> {
    cpu: &'a [Summary],
    io: &'a IoReport,
}

/// Render the CPU and disk reports as a single pretty JSON object.
///
/// # Errors
///
/// Returns any [`serde_json`] serialization error.
pub fn combined_json(cpu: &[Summary], io: &IoReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&Combined { cpu, io })
}

/// Protection level above which the `default` executor is judged not to have
/// dented the foreground — i.e. the device still had spare queue depth.
const SATURATION_PCT: f64 = 90.0;

/// Whether the run actually loaded the device enough for the classes to be
/// distinguishable. If even `default` leaves the foreground near its baseline,
/// nothing was contended for and the disk table says little. Unknown rows count
/// as saturated, so the hint only fires when there is something to say.
pub fn device_saturated(rows: &[IoSummary]) -> bool {
    rows.iter()
        .find(|s| s.executor == "default")
        .and_then(|s| s.foreground_protection_pct)
        .is_none_or(|pct| pct <= SATURATION_PCT)
}

/// `fg_prot%` difference, in percentage points, that counts as "the same". Equal
/// rows on a saturated microSD card differ by about one point between phases.
pub const PROTECTION_TOLERANCE_PTS: f64 = 5.0;

/// Whether the `background` executor left the foreground more disk than the
/// `default` executor did — the "low-priority work yields the device" check.
///
/// Returns `None` if either executor is absent or the baseline was unmeasurable,
/// so callers can skip rather than fail.
pub fn disk_verdict(rows: &[IoSummary]) -> Option<Verdict> {
    let find = |label: &str| rows.iter().find(|s| s.executor == label);
    let bg = find("background")?.foreground_protection_pct?;
    let def = find("default")?.foreground_protection_pct?;
    Some(Verdict::from_advantage(bg - def, PROTECTION_TOLERANCE_PTS))
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod report_tests;
