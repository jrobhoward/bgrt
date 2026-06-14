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

/// Read the calling thread's `uclamp.max` (0..=1024) from
/// `/proc/thread-self/sched`, or `None` if the kernel lacks `CONFIG_UCLAMP_TASK`
/// (in which case the line is absent and the clamp is inert).
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
