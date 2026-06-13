//! Optional macOS power reading via `powermetrics` (requires root).
//!
//! `powermetrics` is the only way to read CPU power / per-cluster frequency on
//! Apple Silicon, and it needs `sudo`. This is best-effort: any failure (not
//! root, binary missing, unparseable output) yields `None`.

use std::process::Command;

/// Average CPU power in milliwatts over a short sample, or `None` if unavailable.
pub fn cpu_power_mw() -> Option<u32> {
    let output = Command::new("powermetrics")
        .args(["-n", "1", "-i", "200", "--samplers", "cpu_power"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_cpu_power_mw(&String::from_utf8_lossy(&output.stdout))
}

/// Extract `CPU Power: <n> mW` from `powermetrics` output.
fn parse_cpu_power_mw(text: &str) -> Option<u32> {
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("CPU Power:") {
            let digits = rest.trim().strip_suffix("mW").unwrap_or(rest);
            if let Ok(v) = digits.trim().parse::<u32>() {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
#[path = "power_macos_tests.rs"]
mod power_macos_tests;
