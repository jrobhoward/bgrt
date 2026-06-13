//! Detection of CPU efficiency cores and thread affinity.
//!
//! On Linux we read per-CPU capacity from sysfs and can pin threads to the
//! efficiency ("little") cores. macOS background QoS and Windows EcoQoS handle
//! efficiency-core placement themselves, so explicit pinning is a no-op there.

use crate::error::Error;

/// OS CPU indices of the efficiency cores, or empty if the topology is unknown
/// or homogeneous (in which case no pinning should be attempted).
#[cfg(target_os = "linux")]
pub(crate) fn efficiency_cores() -> Vec<usize> {
    let Ok(dir) = std::fs::read_dir("/sys/devices/system/cpu") else {
        return Vec::new();
    };
    let mut caps: Vec<(usize, u64)> = Vec::new();
    for entry in dir.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(idx) = name
            .strip_prefix("cpu")
            .and_then(|s| s.parse::<usize>().ok())
        else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(entry.path().join("cpu_capacity")) else {
            continue;
        };
        let Ok(cap) = text.trim().parse::<u64>() else {
            continue;
        };
        caps.push((idx, cap));
    }
    let min = caps.iter().map(|&(_, c)| c).min().unwrap_or(0);
    let max = caps.iter().map(|&(_, c)| c).max().unwrap_or(0);
    if caps.is_empty() || min == max {
        // Capacity info unavailable, or all cores equal → not a hybrid CPU.
        return Vec::new();
    }
    caps.into_iter()
        .filter(|&(_, c)| c == min)
        .map(|(idx, _)| idx)
        .collect()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn efficiency_cores() -> Vec<usize> {
    // macOS/Windows place efficiency work via QoS/EcoQoS; we don't pin explicitly.
    Vec::new()
}

/// Pin the **current** thread to the given CPU indices.
///
/// Linux only; a no-op elsewhere. Restricting a thread to a subset of its
/// allowed CPUs is unprivileged.
#[cfg(target_os = "linux")]
pub(crate) fn pin_current_thread(cpus: &[usize]) -> Result<(), Error> {
    use std::mem;
    // SAFETY: a zeroed `cpu_set_t` is a valid (empty) set; `CPU_ZERO` then clears it.
    let mut set: libc::cpu_set_t = unsafe { mem::zeroed() };
    unsafe { libc::CPU_ZERO(&mut set) };
    for &c in cpus {
        if c < libc::CPU_SETSIZE as usize {
            // SAFETY: `c` is bounds-checked against `CPU_SETSIZE`.
            unsafe { libc::CPU_SET(c, &mut set) };
        }
    }
    // SAFETY: `pid == 0` targets the calling thread; `set`/size are valid.
    let rc = unsafe { libc::sched_setaffinity(0, mem::size_of::<libc::cpu_set_t>(), &set) };
    if rc == 0 {
        Ok(())
    } else {
        Err(Error::Backend(format!(
            "sched_setaffinity failed: {}",
            std::io::Error::last_os_error()
        )))
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn pin_current_thread(_cpus: &[usize]) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
#[path = "topology_tests.rs"]
mod topology_tests;
