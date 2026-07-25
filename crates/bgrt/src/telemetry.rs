//! Measurement primitives for the comparison harness (feature `telemetry`).
//!
//! Every signal is best-effort and degrades gracefully: a value the current OS
//! or privilege level cannot provide is reported as `None` / [`CoreType::Unknown`],
//! never an error or a panic. Availability:
//!
//! | Signal          | Linux               | Windows                     | macOS                  |
//! |-----------------|---------------------|-----------------------------|------------------------|
//! | current CPU     | `sched_getcpu`      | `GetCurrentProcessorNumber` | — (no per-thread API)  |
//! | E/P core type   | sysfs `cpu_capacity`| `GetSystemCpuSetInformation`| —                      |
//! | frequency (MHz) | sysfs `cpufreq`     | `CallNtPowerInformation`    | — (needs `powermetrics`)|
//! | energy (µJ)     | RAPL (if readable)  | —                           | — (needs `powermetrics`)|
//!
//! macOS per-thread core / frequency / energy require `powermetrics` (root); that
//! privileged path is left to the harness (Phase 5).
//!
//! # Stability
//!
//! **This module is exempt from `bgrt`'s semver guarantees.** It exists to serve
//! the `bgrt-bench` comparison harness, and its surface may change or be removed
//! in any release — including a patch release — without a major version bump.
//! The rest of the crate (`QosClass`, `apply`, the builders, `Error`) carries the
//! usual guarantees; this module does not. Depend on it only if you can absorb
//! that, and pin an exact version if you do.

use std::collections::BTreeSet;

/// Whether a CPU is an efficiency or performance core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreType {
    /// An efficiency ("little") core.
    Efficiency,
    /// A performance ("big") core.
    Performance,
    /// Core type could not be determined.
    Unknown,
}

/// A point-in-time sample of where the calling thread runs and how fast.
#[derive(Debug, Clone, Copy)]
pub struct Sample {
    /// OS index of the CPU the calling thread is on, if available.
    pub cpu: Option<usize>,
    /// Whether that CPU is an efficiency or performance core.
    pub core_type: CoreType,
    /// Current frequency of that CPU in MHz, if available.
    pub freq_mhz: Option<u32>,
}

/// Sample the calling thread's current CPU, core type, and frequency.
pub fn sample() -> Sample {
    let cpu = current_cpu();
    let core_type = match cpu {
        Some(c) => classify(c, efficiency_cores()),
        None => CoreType::Unknown,
    };
    let freq_mhz = cpu.and_then(current_freq_mhz);
    Sample {
        cpu,
        core_type,
        freq_mhz,
    }
}

/// Total package energy in microjoules, if available (Linux RAPL).
///
/// Often root-only since CVE-2020-8694, in which case this returns `None`.
#[cfg(target_os = "linux")]
pub fn energy_uj() -> Option<u64> {
    let dir = std::fs::read_dir("/sys/class/powercap").ok()?;
    let mut total: u64 = 0;
    let mut found = false;
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // Top-level package domains only (e.g. `intel-rapl:0`, one colon), not
        // their subdomains (`intel-rapl:0:0`), to avoid double counting.
        if !name.starts_with("intel-rapl:") || name.matches(':').count() != 1 {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(entry.path().join("energy_uj")) {
            if let Ok(uj) = text.trim().parse::<u64>() {
                total = total.saturating_add(uj);
                found = true;
            }
        }
    }
    found.then_some(total)
}

/// Total package energy in microjoules. Unavailable (always `None`) on this
/// platform.
#[cfg(not(target_os = "linux"))]
pub fn energy_uj() -> Option<u64> {
    None
}

/// Measures energy consumed between [`start`](EnergyMeter::start) and
/// [`stop_uj`](EnergyMeter::stop_uj), in microjoules.
#[derive(Debug)]
pub struct EnergyMeter {
    start: Option<u64>,
}

impl EnergyMeter {
    /// Begin measuring from the current energy counter.
    pub fn start() -> Self {
        Self { start: energy_uj() }
    }

    /// Energy consumed since [`start`](EnergyMeter::start), or `None` if the
    /// counter was unavailable or wrapped.
    pub fn stop_uj(self) -> Option<u64> {
        energy_delta(self.start, energy_uj())
    }
}

/// Accumulates [`Sample`]s into residency and frequency statistics.
#[derive(Debug, Default, Clone)]
pub struct Aggregate {
    total: u64,
    efficiency: u64,
    performance: u64,
    cpus: BTreeSet<usize>,
    freq_sum: u64,
    freq_max: u32,
    freq_count: u64,
}

impl Aggregate {
    /// Fold one sample into the running statistics.
    pub fn record(&mut self, s: Sample) {
        self.total = self.total.saturating_add(1);
        match s.core_type {
            CoreType::Efficiency => self.efficiency = self.efficiency.saturating_add(1),
            CoreType::Performance => self.performance = self.performance.saturating_add(1),
            CoreType::Unknown => {}
        }
        if let Some(c) = s.cpu {
            self.cpus.insert(c);
        }
        if let Some(f) = s.freq_mhz {
            self.freq_sum = self.freq_sum.saturating_add(u64::from(f));
            self.freq_max = self.freq_max.max(f);
            self.freq_count = self.freq_count.saturating_add(1);
        }
    }

    /// Number of samples recorded.
    pub fn samples(&self) -> u64 {
        self.total
    }

    /// Number of distinct CPUs the samples landed on.
    pub fn distinct_cpus(&self) -> usize {
        self.cpus.len()
    }

    /// Fraction of *classified* samples on efficiency cores (0.0–1.0), or `None`
    /// if no sample had a known core type.
    pub fn efficiency_fraction(&self) -> Option<f64> {
        let classified = self.efficiency + self.performance;
        (classified > 0).then(|| self.efficiency as f64 / classified as f64)
    }

    /// Mean sampled frequency in MHz, or `None` if no frequency was available.
    pub fn mean_freq_mhz(&self) -> Option<f64> {
        (self.freq_count > 0).then(|| self.freq_sum as f64 / self.freq_count as f64)
    }

    /// Maximum sampled frequency in MHz, or `None` if no frequency was available.
    pub fn max_freq_mhz(&self) -> Option<u32> {
        (self.freq_count > 0).then_some(self.freq_max)
    }
}

// --- internals -------------------------------------------------------------

/// Cached efficiency-core set (sysfs read once).
fn efficiency_cores() -> &'static [usize] {
    use std::sync::OnceLock;
    static CORES: OnceLock<Vec<usize>> = OnceLock::new();
    CORES.get_or_init(crate::topology::efficiency_cores)
}

fn classify(cpu: usize, efficiency_cores: &[usize]) -> CoreType {
    if efficiency_cores.is_empty() {
        CoreType::Unknown
    } else if efficiency_cores.contains(&cpu) {
        CoreType::Efficiency
    } else {
        CoreType::Performance
    }
}

fn energy_delta(start: Option<u64>, end: Option<u64>) -> Option<u64> {
    match (start, end) {
        (Some(s), Some(e)) if e >= s => Some(e - s),
        _ => None, // unavailable, or the counter wrapped
    }
}

#[cfg(target_os = "linux")]
fn current_cpu() -> Option<usize> {
    // SAFETY: `sched_getcpu` has no preconditions.
    let c = unsafe { libc::sched_getcpu() };
    (c >= 0).then_some(c as usize)
}

#[cfg(target_os = "windows")]
fn current_cpu() -> Option<usize> {
    // SAFETY: `GetCurrentProcessorNumber` has no preconditions.
    Some(unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessorNumber() } as usize)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn current_cpu() -> Option<usize> {
    None
}

#[cfg(target_os = "linux")]
fn current_freq_mhz(cpu: usize) -> Option<u32> {
    let path = format!("/sys/devices/system/cpu/cpu{cpu}/cpufreq/scaling_cur_freq");
    let khz: u64 = std::fs::read_to_string(path).ok()?.trim().parse().ok()?;
    u32::try_from(khz / 1000).ok()
}

#[cfg(target_os = "windows")]
fn current_freq_mhz(cpu: usize) -> Option<u32> {
    use core::ffi::c_void;
    use windows_sys::Win32::System::Power::{
        CallNtPowerInformation, PROCESSOR_POWER_INFORMATION, ProcessorInformation,
    };

    let count = std::thread::available_parallelism().ok()?.get();
    // SAFETY: `PROCESSOR_POWER_INFORMATION` is a plain struct of integers; zero
    // is a valid initial value.
    let mut buf: Vec<PROCESSOR_POWER_INFORMATION> = vec![unsafe { core::mem::zeroed() }; count];
    let bytes = u32::try_from(count * size_of::<PROCESSOR_POWER_INFORMATION>()).ok()?;
    // SAFETY: no input buffer; the output buffer is `bytes` long and writable.
    let status = unsafe {
        CallNtPowerInformation(
            ProcessorInformation,
            core::ptr::null(),
            0,
            buf.as_mut_ptr().cast::<c_void>(),
            bytes,
        )
    };
    if status != 0 {
        return None; // NTSTATUS: 0 == STATUS_SUCCESS
    }
    buf.get(cpu).map(|p| p.CurrentMhz)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn current_freq_mhz(_cpu: usize) -> Option<u32> {
    None
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod telemetry_tests;
