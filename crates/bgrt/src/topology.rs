//! Detection of CPU efficiency cores and thread affinity.
//!
//! **Detection** works on Linux (per-CPU capacity from sysfs) and Windows (the
//! CPU Sets API); macOS exposes no unprivileged equivalent. **Pinning** is Linux
//! only — macOS background QoS and Windows EcoQoS place work on efficient cores
//! themselves, and a hard affinity mask would fight that rather than help, so
//! [`pin_current_thread`] is deliberately a no-op off Linux even where the
//! efficiency-core set is known.
//!
//! Both platforms reduce to the same rule: efficiency cores are the CPUs at the
//! *minimum* capacity/class, and a CPU where every core is equal is homogeneous
//! and yields an empty set. See [`select_efficiency_cores`].

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
    select_efficiency_cores(caps)
}

/// Given `(cpu_index, capacity)` pairs, return the CPUs at the **minimum**
/// capacity (the efficiency cores), or empty if the set is empty or homogeneous
/// (capacity unavailable, or all cores equal → not a hybrid CPU).
///
/// Shared by the Linux and Windows detectors — sysfs `cpu_capacity` and the
/// Windows `EfficiencyClass` are both "bigger means faster" scales, so the same
/// minimum-wins rule applies to each. Keeping the decision in one pure function
/// is what lets it be unit-tested on a machine with neither.
#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn select_efficiency_cores(caps: Vec<(usize, u64)>) -> Vec<usize> {
    let min = caps.iter().map(|&(_, c)| c).min().unwrap_or(0);
    let max = caps.iter().map(|&(_, c)| c).max().unwrap_or(0);
    if caps.is_empty() || min == max {
        return Vec::new();
    }
    caps.into_iter()
        .filter(|&(_, c)| c == min)
        .map(|(idx, _)| idx)
        .collect()
}

/// OS CPU indices of the efficiency cores, via the Windows CPU Sets API.
///
/// Windows reports an `EfficiencyClass` per logical processor: the scale is
/// relative and "higher is more performant", with 0 the least performant, so the
/// efficiency cores are those at the minimum class. A machine whose processors
/// all share one class is homogeneous and yields an empty set, exactly as on
/// Linux.
///
/// **Limited to processor group 0.** `LogicalProcessorIndex` is group-relative,
/// and so is `GetCurrentProcessorNumber` — which is what
/// [`telemetry::sample`](crate::telemetry::sample) compares these indices
/// against — so mixing groups would silently alias CPU 3 of group 0 with CPU 3
/// of group 1. Systems with more than 64 logical processors are the only ones
/// affected, and hybrid consumer CPUs (the whole point of this lookup) are
/// single-group.
///
/// Used for *classification* only; Windows placement is EcoQoS's job (see the
/// module docs).
#[cfg(target_os = "windows")]
pub(crate) fn efficiency_cores() -> Vec<usize> {
    select_efficiency_cores(cpu_set_efficiency_classes())
}

/// Read `(logical_processor_index, efficiency_class)` for every CPU set in
/// processor group 0, or empty if the API is unavailable or fails.
#[cfg(target_os = "windows")]
fn cpu_set_efficiency_classes() -> Vec<(usize, u64)> {
    use windows_sys::Win32::System::SystemInformation::{
        CpuSetInformation, GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let record = size_of::<SYSTEM_CPU_SET_INFORMATION>();

    // Size query first: the API returns a run of variable-length records, so the
    // byte count is not derivable from the processor count.
    let mut needed: u32 = 0;
    // SAFETY: the documented size query — a null buffer of length 0 writes only
    // `needed` and returns FALSE. `GetCurrentProcess` is a pseudo-handle that
    // needs no closing.
    unsafe {
        GetSystemCpuSetInformation(
            core::ptr::null_mut(),
            0,
            &mut needed,
            GetCurrentProcess(),
            0,
        );
    }
    let len = (needed as usize).div_ceil(record);
    if len == 0 {
        return Vec::new();
    }

    // Allocated as `SYSTEM_CPU_SET_INFORMATION` rather than bytes so the buffer
    // carries the struct's alignment.
    // SAFETY: the struct is a union of plain integers; all-zero is a valid value.
    let mut buf: Vec<SYSTEM_CPU_SET_INFORMATION> = vec![unsafe { core::mem::zeroed() }; len];
    let Ok(bytes) = u32::try_from(len * record) else {
        return Vec::new();
    };
    let mut written: u32 = 0;
    // SAFETY: `buf` is `bytes` long and writable; `written` receives the number
    // of bytes actually filled.
    let ok = unsafe {
        GetSystemCpuSetInformation(
            buf.as_mut_ptr(),
            bytes,
            &mut written,
            GetCurrentProcess(),
            0,
        )
    };
    if ok == 0 {
        return Vec::new();
    }

    // Clamp to what we actually allocated before walking, so a bogus `written`
    // cannot walk us out of the buffer.
    let written = (written as usize).min(len * record);
    let base = buf.as_ptr().cast::<u8>();
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset + record <= written {
        // SAFETY: `offset + record <= written`, which is clamped to the
        // allocation, so the read stays in bounds. `read_unaligned` is used
        // because records are only guaranteed to be `Size` apart.
        let info = unsafe {
            base.add(offset)
                .cast::<SYSTEM_CPU_SET_INFORMATION>()
                .read_unaligned()
        };
        if info.Type == CpuSetInformation {
            // SAFETY: `Type == CpuSetInformation` selects the `CpuSet` variant,
            // which is currently the union's only member.
            let set = unsafe { info.Anonymous.CpuSet };
            if set.Group == 0 {
                out.push((
                    usize::from(set.LogicalProcessorIndex),
                    u64::from(set.EfficiencyClass),
                ));
            }
        }
        // A record shorter than the struct we just read would leave `offset`
        // stuck or moving backwards; stop rather than loop forever.
        if (info.Size as usize) < record {
            break;
        }
        offset += info.Size as usize;
    }
    out
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub(crate) fn efficiency_cores() -> Vec<usize> {
    // macOS exposes no unprivileged per-CPU efficiency-class API; background QoS
    // handles placement itself, so there is nothing to detect or pin.
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
        Err(Error::Backend {
            syscall: "sched_setaffinity",
            source: std::io::Error::last_os_error(),
        })
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn pin_current_thread(_cpus: &[usize]) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
#[path = "topology_tests.rs"]
mod topology_tests;
