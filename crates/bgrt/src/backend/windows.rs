//! Windows energy-QoS backend.
//!
//! Enables EcoQoS (`SetThreadInformation` with
//! `THREAD_POWER_THROTTLING_EXECUTION_SPEED`) so the scheduler prefers efficient
//! cores and lower clocks, plus a `SetThreadPriority`:
//! [`QosClass::Background`] → below-normal, [`QosClass::Utility`] → normal
//! (deliberately not `IDLE`, to avoid starvation). [`QosClass::Default`] clears
//! EcoQoS (back to system-managed) and restores normal priority. All of this is
//! unprivileged.

use core::ffi::c_void;

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, SetThreadInformation, SetThreadPriority,
    THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
    THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL,
    ThreadPowerThrottling,
};

use crate::error::Error;
use crate::qos::QosClass;

pub(super) fn apply(class: QosClass) -> Result<(), Error> {
    let (eco, priority) = match class {
        QosClass::Background => (true, THREAD_PRIORITY_BELOW_NORMAL),
        QosClass::Utility => (true, THREAD_PRIORITY_NORMAL),
        QosClass::Default => (false, THREAD_PRIORITY_NORMAL),
    };
    set_eco_qos(eco)?;
    set_priority(priority)?;
    Ok(())
}

/// Enable or clear EcoQoS execution-speed throttling on the current thread.
/// Clearing (both masks 0) returns the thread to system-managed throttling.
fn set_eco_qos(enabled: bool) -> Result<(), Error> {
    let mask = if enabled {
        THREAD_POWER_THROTTLING_EXECUTION_SPEED
    } else {
        0
    };
    let state = THREAD_POWER_THROTTLING_STATE {
        Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: mask,
        StateMask: mask,
    };
    // SAFETY: passing a fully-initialized, correctly-sized power-throttling state
    // for the current-thread pseudo-handle (which needs no closing).
    let rc = unsafe {
        SetThreadInformation(
            GetCurrentThread(),
            ThreadPowerThrottling,
            (&raw const state).cast::<c_void>(),
            size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if rc == 0 {
        // SAFETY: `GetLastError` has no preconditions.
        let code = unsafe { GetLastError() };
        Err(Error::Backend {
            syscall: "SetThreadInformation(ThreadPowerThrottling)",
            source: std::io::Error::from_raw_os_error(code as i32),
        })
    } else {
        Ok(())
    }
}

fn set_priority(priority: i32) -> Result<(), Error> {
    // SAFETY: setting a documented priority constant on the current-thread handle.
    let rc = unsafe { SetThreadPriority(GetCurrentThread(), priority) };
    if rc == 0 {
        // SAFETY: `GetLastError` has no preconditions.
        let code = unsafe { GetLastError() };
        Err(Error::Backend {
            syscall: "SetThreadPriority",
            source: std::io::Error::from_raw_os_error(code as i32),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod windows_tests;
