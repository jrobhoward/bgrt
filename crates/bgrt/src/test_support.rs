//! Shared helpers for unit tests (compiled only under `cfg(test)`).
//!
//! These read back the effects of [`crate::apply`] so multiple test modules can
//! assert classification without each re-declaring the platform FFI.

/// macOS QoS class values from `<sys/qos.h>`.
#[cfg(target_os = "macos")]
pub const QOS_CLASS_BACKGROUND: u32 = 0x09;
/// macOS `QOS_CLASS_UTILITY`.
#[cfg(target_os = "macos")]
pub const QOS_CLASS_UTILITY: u32 = 0x11;
/// macOS `QOS_CLASS_DEFAULT` — what an unclassified thread reports.
#[cfg(target_os = "macos")]
pub const QOS_CLASS_DEFAULT: u32 = 0x15;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn pthread_get_qos_class_np(
        thread: libc::pthread_t,
        qos_class: *mut u32,
        relative_priority: *mut i32,
    ) -> i32;
}

/// Read the calling thread's effective QoS class (macOS).
#[cfg(target_os = "macos")]
pub fn current_qos() -> u32 {
    let mut qos = 0u32;
    let mut rel = 0i32;
    // SAFETY: reads the calling thread's QoS into stack-local out-parameters.
    let rc = unsafe { pthread_get_qos_class_np(libc::pthread_self(), &mut qos, &mut rel) };
    assert_eq!(rc, 0, "pthread_get_qos_class_np failed: {rc}");
    qos
}

/// macOS `IOPOL_DEFAULT` from `<sys/resource.h>` — "no thread override".
#[cfg(target_os = "macos")]
pub const IOPOL_DEFAULT: i32 = 0;
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn getiopolicy_np(iotype: i32, scope: i32) -> i32;
}

/// Read the calling thread's disk I/O policy (macOS).
///
/// Reports the thread's *explicit override*, not the effective QoS-derived
/// policy — a `QOS_CLASS_BACKGROUND` thread reads `IOPOL_DEFAULT` here even
/// though Darwin is throttling its disk I/O. Used to assert that `bgrt` leaves
/// the override alone; see `backend/macos_tests.rs`.
#[cfg(target_os = "macos")]
pub fn current_disk_iopolicy() -> i32 {
    const IOPOL_TYPE_DISK: i32 = 0;
    const IOPOL_SCOPE_THREAD: i32 = 1;
    // SAFETY: reads the calling thread's disk I/O policy; no preconditions.
    unsafe { getiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD) }
}

/// Read the calling thread's `uclamp.max` (0..=1024) from
/// `/proc/thread-self/sched`, or `None` if the kernel lacks `CONFIG_UCLAMP_TASK`
/// (in which case the line is absent and the clamp does nothing).
#[cfg(target_os = "linux")]
pub fn current_uclamp_max() -> Option<u32> {
    let sched = std::fs::read_to_string("/proc/thread-self/sched").ok()?;
    sched
        .lines()
        .find_map(|line| line.strip_prefix("uclamp.max"))
        // Lines read "uclamp.max  :  1024"; take the value after the colon.
        .and_then(|rest| rest.rsplit(':').next())
        .and_then(|v| v.trim().parse().ok())
}

/// Read the calling thread's raw block-I/O priority word (Linux).
///
/// `ioprio_get` reports whatever was *set*, independent of whether the active
/// I/O scheduler honours it, so assertions on this are deterministic even on a
/// runner using the `none` scheduler.
#[cfg(target_os = "linux")]
pub fn current_ioprio() -> i32 {
    const IOPRIO_WHO_PROCESS: i32 = 1;
    // SAFETY: reads the calling thread's I/O priority (`who` = process, pid 0).
    let rc = unsafe { libc::syscall(libc::SYS_ioprio_get, IOPRIO_WHO_PROCESS, 0) };
    assert!(rc >= 0, "ioprio_get failed: {rc}");
    rc as i32
}

/// Split a raw I/O priority word into `(class, level)`.
#[cfg(target_os = "linux")]
pub fn ioprio_parts(prio: i32) -> (i32, i32) {
    const IOPRIO_CLASS_SHIFT: i32 = 13;
    const IOPRIO_PRIO_MASK: i32 = (1 << IOPRIO_CLASS_SHIFT) - 1;
    (prio >> IOPRIO_CLASS_SHIFT, prio & IOPRIO_PRIO_MASK)
}

/// Read the calling thread's nice value, -20..=19 (Linux).
#[cfg(target_os = "linux")]
pub fn current_nice() -> i32 {
    // `getpriority` can legitimately return -1, so clear errno first.
    // SAFETY: writes/reads the libc errno location; reads the caller's nice.
    unsafe {
        *libc::__errno_location() = 0;
        let v = libc::getpriority(libc::PRIO_PROCESS as _, 0);
        let errno = *libc::__errno_location();
        assert_eq!(errno, 0, "getpriority failed: errno {errno}");
        v
    }
}

/// Read the calling thread's priority (Windows).
#[cfg(target_os = "windows")]
pub fn current_thread_priority() -> i32 {
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadPriority};
    // SAFETY: reads the priority of the current-thread pseudo-handle.
    unsafe { GetThreadPriority(GetCurrentThread()) }
}
