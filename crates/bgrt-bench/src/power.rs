//! Parsed `powermetrics` statistics.
//!
//! The data type and parser are cross-platform (and unit-tested); only the
//! sampling that produces the text lives in the macOS-only `power_macos` module.
//! Numeric fields are averaged across all sample blocks in the input.

use std::time::Duration;

/// Aggregated CPU power / per-cluster stats parsed from `powermetrics` output.
#[derive(Debug, Default, Clone)]
pub struct PowerStats {
    cpu_power_mw: Option<f64>,
    e_residency_pct: Option<f64>,
    p_residency_pct: Option<f64>,
    e_freq_mhz: Option<f64>,
    p_freq_mhz: Option<f64>,
}

#[derive(Clone, Copy)]
enum Cluster {
    E,
    P,
}

impl PowerStats {
    /// Parse `powermetrics --samplers cpu_power` output, averaging across samples.
    pub fn parse(text: &str) -> Self {
        let mut cpu = Acc::new();
        let (mut e_freq, mut p_freq) = (Acc::new(), Acc::new());
        let (mut e_res, mut p_res) = (Acc::new(), Acc::new());

        for raw in text.lines() {
            let line = raw.trim();
            if let Some(rest) = metric_after(line, "CPU Power:") {
                if let Some(v) = leading_number(rest) {
                    cpu.add(v);
                }
                continue;
            }
            let Some(kind) = cluster_kind(line) else {
                continue;
            };
            if let Some(rest) = metric_after(line, "active frequency:") {
                if let Some(v) = leading_number(rest) {
                    freq_acc(kind, &mut e_freq, &mut p_freq).add(v);
                }
            } else if let Some(rest) = metric_after(line, "active residency:") {
                if let Some(v) = leading_number(rest) {
                    freq_acc(kind, &mut e_res, &mut p_res).add(v);
                }
            }
        }

        Self {
            cpu_power_mw: cpu.avg(),
            e_freq_mhz: e_freq.avg(),
            p_freq_mhz: p_freq.avg(),
            e_residency_pct: e_res.avg(),
            p_residency_pct: p_res.avg(),
        }
    }

    /// Average CPU power in milliwatts, if present.
    pub fn cpu_power_mw(&self) -> Option<f64> {
        self.cpu_power_mw
    }

    /// Fraction of CPU active residency on efficiency cores, as a percent.
    pub fn efficiency_pct(&self) -> Option<f64> {
        let e = self.e_residency_pct?;
        let p = self.p_residency_pct?;
        (e + p > 0.0).then(|| e / (e + p) * 100.0)
    }

    /// Residency-weighted mean cluster frequency (MHz), if any cluster reported.
    pub fn mean_freq_mhz(&self) -> Option<f64> {
        match (self.e_freq_mhz, self.p_freq_mhz) {
            (Some(ef), Some(pf)) => match (self.e_residency_pct, self.p_residency_pct) {
                (Some(er), Some(pr)) if er + pr > 0.0 => Some((ef * er + pf * pr) / (er + pr)),
                _ => Some((ef + pf) / 2.0),
            },
            (Some(f), None) | (None, Some(f)) => Some(f),
            (None, None) => None,
        }
    }

    /// Highest reported cluster frequency (MHz).
    pub fn max_freq_mhz(&self) -> Option<u32> {
        [self.e_freq_mhz, self.p_freq_mhz]
            .into_iter()
            .flatten()
            .fold(None, |acc: Option<f64>, v| {
                Some(acc.map_or(v, |a| a.max(v)))
            })
            .map(|v| v as u32)
    }

    /// Energy estimate (joules) = mean power × wall time.
    pub fn energy_j(&self, wall: Duration) -> Option<f64> {
        self.cpu_power_mw.map(|mw| mw / 1000.0 * wall.as_secs_f64())
    }
}

struct Acc {
    sum: f64,
    n: u32,
}

impl Acc {
    fn new() -> Self {
        Self { sum: 0.0, n: 0 }
    }
    fn add(&mut self, v: f64) {
        self.sum += v;
        self.n += 1;
    }
    fn avg(&self) -> Option<f64> {
        (self.n > 0).then(|| self.sum / f64::from(self.n))
    }
}

fn freq_acc<'a>(kind: Cluster, e: &'a mut Acc, p: &'a mut Acc) -> &'a mut Acc {
    match kind {
        Cluster::E => e,
        Cluster::P => p,
    }
}

fn cluster_kind(line: &str) -> Option<Cluster> {
    if line.starts_with("E-Cluster") {
        Some(Cluster::E)
    } else if line.starts_with('P') && line.contains("-Cluster") {
        Some(Cluster::P)
    } else {
        None
    }
}

fn metric_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.find(key).map(|i| &line[i + key.len()..])
}

/// Parse the leading numeric run (digits, `.`, `-`) of a string like `"1024 MHz"`.
fn leading_number(s: &str) -> Option<f64> {
    let s = s.trim();
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(s.len());
    s[..end].parse::<f64>().ok()
}

#[cfg(test)]
#[path = "power_tests.rs"]
mod power_tests;
