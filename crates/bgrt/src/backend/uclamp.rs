//! Linux utilization clamping (uclamp) — an opt-in per-task frequency hint.
//!
//! On homogeneous CPUs there are no efficiency cores to confine to, and `nice`
//! gives the cpufreq governor (schedutil / EAS) *no* frequency input: a busy
//! background loop still drives clocks to the top. `uclamp` is the missing
//! lever — it caps the utilization signal a task contributes, so the governor
//! selects a lower OPP (operating performance point) for it even at 100% busy.
//!
//! We only ever *lower* `util_max`, which is always unprivileged (raising
//! `util_min` above the system cap would need `CAP_SYS_NICE`; we never do that),
//! keeping the library's no-elevation guarantee. This is opt-in and best-effort:
//! on kernels without uclamp (< 5.3) or `SCHED_FLAG_KEEP_ALL` (< 5.8) the
//! syscall fails and we degrade to a no-op, exactly like unknown topology.
//!
//! Effect requires the `schedutil` governor (or `intel_pstate=passive`); under
//! fixed governors or HWP the kernel's util signal is bypassed and the cap is
//! inert. Off Linux this is a no-op: macOS/Windows throttle frequency via their
//! own QoS/EcoQoS facilities.

#[cfg(target_os = "linux")]
use crate::error::Error;
#[cfg(target_os = "linux")]
use crate::qos::QosClass;

/// Full utilization (`SCHED_CAPACITY_SCALE`); the `util_*` fields are `0..=1024`.
#[cfg(target_os = "linux")]
const CAPACITY_SCALE: u32 = 1024;

// `sched_attr.sched_flags` bits (uapi/linux/sched.h).
#[cfg(target_os = "linux")]
const SCHED_FLAG_KEEP_POLICY: u64 = 0x08;
#[cfg(target_os = "linux")]
const SCHED_FLAG_KEEP_PARAMS: u64 = 0x10;
/// Keep the current policy and params; touch only the uclamp fields. Requires
/// kernel ≥ 5.8 (older kernels reject the flag, handled as a no-op below).
#[cfg(target_os = "linux")]
const SCHED_FLAG_KEEP_ALL: u64 = SCHED_FLAG_KEEP_POLICY | SCHED_FLAG_KEEP_PARAMS;
#[cfg(target_os = "linux")]
const SCHED_FLAG_UTIL_CLAMP_MAX: u64 = 0x40;

/// `struct sched_attr` (uapi/linux/sched/types.h). `libc` ships no wrapper for
/// `sched_setattr` nor this struct, so we declare it. The kernel versions the
/// struct by its `size`; passing our (newer) size is fine on capable kernels
/// and rejected on pre-uclamp ones, which we treat as a no-op.
#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Default)]
struct SchedAttr {
    size: u32,
    sched_policy: u32,
    sched_flags: u64,
    sched_nice: i32,
    sched_priority: u32,
    sched_runtime: u64,
    sched_deadline: u64,
    sched_period: u64,
    sched_util_min: u32,
    sched_util_max: u32,
}

/// `util_max` cap per class, or `None` to leave the task unclamped.
///
/// Only [`QosClass::Background`] is clamped: [`QosClass::Utility`] is
/// deliberately "quiet but unconfined" (free to ask for high clocks), matching
/// its E-core-free stance elsewhere. ~20% confines background work to the low
/// OPPs that keep the fans off. Tunable; this is the one knob worth measuring.
#[cfg(target_os = "linux")]
fn util_max_for(class: QosClass) -> Option<u32> {
    match class {
        QosClass::Background => Some(CAPACITY_SCALE / 5), // 204 ≈ 20%
        QosClass::Utility | QosClass::Default => None,
    }
}

/// Cap the **current** thread's `util_max` for `class`. A no-op for classes that
/// aren't clamped, and a graceful no-op on kernels/governors without uclamp.
#[cfg(target_os = "linux")]
pub(crate) fn clamp_current_thread(class: QosClass) -> Result<(), Error> {
    let Some(util_max) = util_max_for(class) else {
        return Ok(());
    };

    // KEEP_ALL preserves the policy and the nice value the backend already set
    // via setpriority; we touch only util_max.
    let attr = SchedAttr {
        size: size_of::<SchedAttr>() as u32,
        sched_flags: SCHED_FLAG_KEEP_ALL | SCHED_FLAG_UTIL_CLAMP_MAX,
        sched_util_max: util_max,
        ..SchedAttr::default()
    };

    // SAFETY: `pid == 0` targets the calling thread; `attr` is a
    // fully-initialized, correctly-sized `sched_attr`; the flags arg is 0.
    let rc = unsafe { libc::syscall(libc::SYS_sched_setattr, 0, &raw const attr, 0) };
    if rc == 0 {
        return Ok(());
    }

    let err = std::io::Error::last_os_error();
    match err.raw_os_error() {
        // Pre-uclamp kernel (ENOSYS), unknown flag / no util fields (EINVAL,
        // E2BIG), or syscall blocked by a sandbox (EPERM, EOPNOTSUPP): this is a
        // best-effort hint, so degrade to a no-op rather than failing the build.
        Some(libc::ENOSYS | libc::EINVAL | libc::E2BIG | libc::EPERM | libc::EOPNOTSUPP) => {
            tracing::debug!(%err, "uclamp unsupported; skipping frequency clamp");
            Ok(())
        }
        _ => Err(Error::Backend {
            syscall: "sched_setattr(uclamp)",
            source: err,
        }),
    }
}

/// macOS/Windows place and throttle via QoS/EcoQoS; no separate uclamp lever.
#[cfg(not(target_os = "linux"))]
pub(crate) fn clamp_current_thread(
    _class: crate::qos::QosClass,
) -> Result<(), crate::error::Error> {
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
#[path = "uclamp_tests.rs"]
mod uclamp_tests;
