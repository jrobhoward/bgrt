//! Windows energy-QoS backend.
//!
//! Three independent levers, applied together:
//!
//! 1. **Background processing mode** (`SetThreadPriority` with
//!    `THREAD_MODE_BACKGROUND_BEGIN`) — the only documented way to lower a
//!    thread's *block-I/O* priority on Windows, so this is what makes a
//!    [`QosClass`] cover disk as well as CPU. [`QosClass::Background`] only.
//! 2. **EcoQoS** (`SetThreadInformation` with
//!    `THREAD_POWER_THROTTLING_EXECUTION_SPEED`) so the scheduler prefers
//!    efficient cores and lower clocks. `Background` and `Utility`.
//! 3. **Thread priority** — [`QosClass::Background`] → below-normal,
//!    [`QosClass::Utility`] → normal (deliberately not `IDLE`, to avoid
//!    starvation).
//!
//! [`QosClass::Default`] reverses all three. All of this is unprivileged.
//!
//! # Why the *thread* background mode and never the process one
//!
//! `PROCESS_MODE_BACKGROUND_BEGIN` carries an **undocumented hard 32 MiB cap on
//! the process working set**, which has been measured making real programs
//! 250–800× slower. Mozilla investigated it and closed the idea WONTFIX; Chromium
//! dropped it too. Both recommended the per-thread flag used here instead, which
//! carries no such cap. `bgrt` classifies threads, not processes, so the
//! dangerous variant is not merely avoided — it is unreachable by design.
//!
//! # Starvation
//!
//! Microsoft documents that a thread in background mode "may not be scheduled
//! promptly, but it will never be starved", which is what lets this satisfy the
//! crate's never-starve rule. Note the hedge, though: never-starved is a weaker
//! promise than the weighted-fair share Linux `nice` gives, and Windows delivers
//! it by periodically boosting a thread that has been denied the CPU for too
//! long. Throughput under sustained foreground load is therefore expected to be
//! poor — this is the quiet end of the range, by design.
//!
//! # Memory priority is deliberately restored
//!
//! Background mode also drops the thread to `MEMORY_PRIORITY_VERY_LOW`, so its
//! pages are trimmed first. That is a latency hazard rather than an energy win —
//! trimmed pages get faulted back in, costing the disk I/O this class is trying
//! to avoid — so it is put back immediately afterwards. Chromium does the same
//! thing for the same reason.
//!
//! The value put back is the one the thread *had*, read just before entering the
//! mode — not a hard-coded `MEMORY_PRIORITY_NORMAL`. Normal is only the system
//! default: a process can lower its own default with
//! `SetProcessInformation(ProcessMemoryPriority)`, and new threads inherit that,
//! so a thread can legitimately start below normal (observed on GitHub Actions
//! `windows-latest` runners, which start threads at `MEMORY_PRIORITY_LOW`).
//! Writing normal unconditionally would *raise* such a thread above the policy
//! its process chose — `bgrt` only ever lowers.
//!
//! # Priority inversion
//!
//! Microsoft warns that a background-mode thread "should minimize sharing
//! resources such as critical sections, heaps, and handles with other threads in
//! the process, otherwise priority inversions can occur". `bgrt` threads *do*
//! share a heap (the global allocator) with the rest of the process. This is the
//! Windows analogue of the documented macOS join-promotion caveat: fire-and-
//! forget background work is fine, but a foreground thread that blocks on a
//! `Background` thread's lock can be held up. See `docs/DESIGN.md`.

use core::ffi::c_void;

use windows_sys::Win32::Foundation::{
    ERROR_THREAD_MODE_ALREADY_BACKGROUND, ERROR_THREAD_MODE_NOT_BACKGROUND, GetLastError,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, GetThreadInformation, MEMORY_PRIORITY_INFORMATION, SetThreadInformation,
    SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN, THREAD_MODE_BACKGROUND_END,
    THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
    THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL,
    ThreadMemoryPriority, ThreadPowerThrottling,
};

use crate::error::Error;
use crate::qos::QosClass;

pub(super) fn apply(class: QosClass) -> Result<(), Error> {
    let (background, eco, priority) = match class {
        QosClass::Background => (true, true, THREAD_PRIORITY_BELOW_NORMAL),
        // Utility is "quiet but unconfined": EcoQoS, but no I/O reduction.
        // Windows offers no intermediate I/O tier — background mode is
        // all-or-nothing — and taking it here would drag CPU priority down too,
        // which is exactly what distinguishes Utility from Background.
        QosClass::Utility => (false, true, THREAD_PRIORITY_NORMAL),
        QosClass::Default => (false, false, THREAD_PRIORITY_NORMAL),
    };

    // Background mode first: it clobbers memory priority and nudges CPU
    // priority, so the explicit settings below must land after it. The memory
    // priority to put back has to be sampled before the mode clobbers it.
    let previous_memory_priority = if background {
        Some(memory_priority()?)
    } else {
        None
    };
    set_background_mode(background)?;
    if let Some(previous) = previous_memory_priority {
        set_memory_priority(previous)?;
    }
    set_eco_qos(eco)?;
    set_priority(priority)?;
    Ok(())
}

/// Enter or leave background processing mode on the current thread.
///
/// This is the I/O lever: entering also lowers block-I/O priority to "very low",
/// which no other documented per-thread Windows API exposes.
///
/// Windows reports "already in that state" as a *failure*, but for a classifier
/// that may be applied more than once to the same thread it is the desired
/// outcome, so both such codes are treated as success. That is what makes
/// `apply` idempotent on Windows.
fn set_background_mode(enter: bool) -> Result<(), Error> {
    let (mode, already) = if enter {
        (
            THREAD_MODE_BACKGROUND_BEGIN,
            ERROR_THREAD_MODE_ALREADY_BACKGROUND,
        )
    } else {
        (THREAD_MODE_BACKGROUND_END, ERROR_THREAD_MODE_NOT_BACKGROUND)
    };

    // SAFETY: setting a documented mode constant on the current-thread
    // pseudo-handle, which is what these two values require and needs no closing.
    let rc = unsafe { SetThreadPriority(GetCurrentThread(), mode) };
    if rc != 0 {
        return Ok(());
    }

    // SAFETY: `GetLastError` has no preconditions.
    let code = unsafe { GetLastError() };
    if code == already {
        return Ok(()); // Already in the requested state.
    }
    Err(Error::Backend {
        syscall: if enter {
            "SetThreadPriority(THREAD_MODE_BACKGROUND_BEGIN)"
        } else {
            "SetThreadPriority(THREAD_MODE_BACKGROUND_END)"
        },
        source: std::io::Error::from_raw_os_error(code as i32),
    })
}

/// Read the current thread's memory priority, so background mode's clobbering of
/// it can be undone with the value the thread actually had.
fn memory_priority() -> Result<u32, Error> {
    let mut info = MEMORY_PRIORITY_INFORMATION { MemoryPriority: 0 };
    // SAFETY: writes a correctly-sized memory-priority struct through the
    // current-thread pseudo-handle (which needs no closing).
    let rc = unsafe {
        GetThreadInformation(
            GetCurrentThread(),
            ThreadMemoryPriority,
            (&raw mut info).cast::<c_void>(),
            size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    };
    if rc == 0 {
        // SAFETY: `GetLastError` has no preconditions.
        let code = unsafe { GetLastError() };
        return Err(Error::Backend {
            syscall: "GetThreadInformation(ThreadMemoryPriority)",
            source: std::io::Error::from_raw_os_error(code as i32),
        });
    }
    Ok(info.MemoryPriority)
}

/// Put the thread's memory priority back after background mode lowered it. See
/// the module docs for why this is undone rather than kept.
fn set_memory_priority(priority: u32) -> Result<(), Error> {
    let info = MEMORY_PRIORITY_INFORMATION {
        MemoryPriority: priority,
    };
    // SAFETY: passing a fully-initialized, correctly-sized memory-priority
    // struct for the current-thread pseudo-handle.
    let rc = unsafe {
        SetThreadInformation(
            GetCurrentThread(),
            ThreadMemoryPriority,
            (&raw const info).cast::<c_void>(),
            size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
        )
    };
    if rc == 0 {
        // SAFETY: `GetLastError` has no preconditions.
        let code = unsafe { GetLastError() };
        return Err(Error::Backend {
            syscall: "SetThreadInformation(ThreadMemoryPriority)",
            source: std::io::Error::from_raw_os_error(code as i32),
        });
    }
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
