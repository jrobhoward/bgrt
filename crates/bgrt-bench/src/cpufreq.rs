//! The Linux cpufreq governor, reported alongside the CPU table.
//!
//! A flat `--clamp-frequency` result means one of two things: the clamp did
//! nothing because the governor ignores it, or something is broken. Only the
//! governor tells them apart, so the harness prints it rather than leaving the
//! reader to check sysfs by hand. Stock Raspberry Pi images run `ondemand`, for
//! example, under which the clamp has no effect.

/// The distinct governors across every cpufreq policy, sorted. `None` off
/// Linux, and on Linux machines with no cpufreq at all (most VMs).
#[cfg(target_os = "linux")]
pub fn governors() -> Option<Vec<String>> {
    let entries = std::fs::read_dir("/sys/devices/system/cpu/cpufreq").ok()?;
    let found = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("policy"))
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("scaling_governor")).ok())
        .collect();
    distinct(found)
}

/// No cpufreq governors off Linux.
#[cfg(not(target_os = "linux"))]
pub fn governors() -> Option<Vec<String>> {
    None
}

/// Trim, sort, and de-duplicate raw sysfs reads; `None` when nothing was read.
#[cfg(any(target_os = "linux", test))]
fn distinct(raw: Vec<String>) -> Option<Vec<String>> {
    let mut governors: Vec<String> = raw
        .into_iter()
        .map(|g| g.trim().to_owned())
        .filter(|g| !g.is_empty())
        .collect();
    governors.sort_unstable();
    governors.dedup();
    (!governors.is_empty()).then_some(governors)
}

/// Whether a `uclamp` cap can move the clock under these governors: at least one
/// policy has to run `schedutil`, the only governor that reads the utilization
/// signal the cap lowers.
pub fn clamp_can_act(governors: &[String]) -> bool {
    governors.iter().any(|g| g == "schedutil")
}

#[cfg(test)]
#[path = "cpufreq_tests.rs"]
mod cpufreq_tests;
