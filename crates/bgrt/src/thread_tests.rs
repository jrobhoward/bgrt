//! Tests for the quiet-thread spawn API.
#![allow(non_snake_case)]

use crate::{QosClass, ThreadBuilder, spawn_thread};

#[test]
fn spawn_thread____returns_closure_value() {
    let h = spawn_thread(QosClass::Background, || 6 * 7);
    assert_eq!(h.join().unwrap(), 42);
}

#[test]
fn thread_builder____named_with_stack____spawns_and_runs() {
    let h = ThreadBuilder::new()
        .qos(QosClass::Utility)
        .name("bgrt-test")
        .stack_size(64 * 1024)
        .spawn(|| 42)
        .unwrap();
    assert_eq!(h.join().unwrap(), 42);
}

// macOS: prove the spawned thread actually carries the QoS class.
#[cfg(target_os = "macos")]
mod macos {
    use crate::test_support::{QOS_CLASS_BACKGROUND, QOS_CLASS_UTILITY, current_qos};
    use crate::{QosClass, ThreadBuilder, spawn_thread};

    #[test]
    fn spawn_thread____background____thread_is_classified() {
        let h = spawn_thread(QosClass::Background, current_qos);
        assert_eq!(h.join().unwrap(), QOS_CLASS_BACKGROUND);
    }

    #[test]
    fn thread_builder____utility____thread_is_classified() {
        let h = ThreadBuilder::new()
            .qos(QosClass::Utility)
            .spawn(current_qos)
            .unwrap();
        assert_eq!(h.join().unwrap(), QOS_CLASS_UTILITY);
    }
}

// Linux: the spawned thread should carry nice 19 (runs on CI / Linux hardware).
#[cfg(target_os = "linux")]
mod linux {
    use crate::test_support::{current_ioprio, current_nice, ioprio_parts};
    use crate::{QosClass, spawn_thread};

    const IOPRIO_CLASS_BE: i32 = 2;

    #[test]
    fn spawn_thread____background____thread_is_nice_19() {
        let h = spawn_thread(QosClass::Background, current_nice);
        assert_eq!(h.join().unwrap(), 19);
    }

    // The disk half of the class has to reach spawned threads too, not just
    // direct `apply` calls.
    #[test]
    fn spawn_thread____background____thread_is_io_best_effort_7() {
        let h = spawn_thread(QosClass::Background, current_ioprio);
        assert_eq!(ioprio_parts(h.join().unwrap()), (IOPRIO_CLASS_BE, 7));
    }
}

// --- inheritance ----------------------------------------------------------
//
// Does a thread spawned *by* a classified thread inherit the classification?
// This is the RocksDB question: a library handed a `Background` thread will
// spawn its own compaction/flush workers, and whether those stay quiet decides
// whether the classification means anything for that workload.
//
// The three platforms disagree, so each assertion below is deliberate, not a
// copy of its neighbour. They exist to pin the behaviour down in CI rather than
// leave the README's table resting on documentation.

/// macOS: **not** inherited. Measured — a child of a `Background` (0x09) or
/// `Utility` (0x11) thread reports `QOS_CLASS_DEFAULT` (0x15). Darwin propagates
/// QoS through dispatch queues and `pthread_attr_set_qos_class_np`, not through
/// plain `pthread_create`.
#[cfg(target_os = "macos")]
#[test]
fn spawn_thread____child_thread____does_not_inherit_the_qos_class() {
    use crate::test_support::{QOS_CLASS_BACKGROUND, QOS_CLASS_DEFAULT, current_qos};

    let h = spawn_thread(QosClass::Background, || {
        let parent = current_qos();
        let child = std::thread::spawn(current_qos).join().unwrap();
        (parent, child)
    });
    let (parent, child) = h.join().unwrap();
    assert_eq!(
        parent, QOS_CLASS_BACKGROUND,
        "the parent should be classified"
    );
    assert_eq!(
        child, QOS_CLASS_DEFAULT,
        "macOS inheritance changed — the README's per-OS table needs updating"
    );
}

/// Linux: **inherited**. `nice` and I/O priority live in `task_struct` and are
/// copied by `clone()`, so a library's own threads stay quiet for free. This is
/// the only platform where that holds.
#[cfg(target_os = "linux")]
#[test]
fn spawn_thread____child_thread____inherits_nice_and_io_priority() {
    use crate::test_support::{current_ioprio, current_nice, ioprio_parts};

    const IOPRIO_CLASS_BE: i32 = 2;

    let h = spawn_thread(QosClass::Background, || {
        std::thread::spawn(|| (current_nice(), current_ioprio()))
            .join()
            .unwrap()
    });
    let (nice, ioprio) = h.join().unwrap();
    assert_eq!(nice, 19, "Linux nice inheritance changed");
    assert_eq!(
        ioprio_parts(ioprio),
        (IOPRIO_CLASS_BE, 7),
        "Linux ioprio inheritance changed"
    );
}

/// Windows: **not** inherited. MSDN: "All threads initially start at
/// `THREAD_PRIORITY_NORMAL`." Background processing mode and EcoQoS are
/// per-thread and don't propagate either.
#[cfg(target_os = "windows")]
#[test]
fn spawn_thread____child_thread____does_not_inherit_the_thread_priority() {
    use crate::test_support::current_thread_priority;
    use windows_sys::Win32::System::Threading::{
        THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_NORMAL,
    };

    let h = spawn_thread(QosClass::Background, || {
        let parent = current_thread_priority();
        let child = std::thread::spawn(current_thread_priority).join().unwrap();
        (parent, child)
    });
    let (parent, child) = h.join().unwrap();
    assert_eq!(
        parent, THREAD_PRIORITY_BELOW_NORMAL,
        "the parent should be classified"
    );
    assert_eq!(
        child, THREAD_PRIORITY_NORMAL,
        "Windows inheritance changed — the README's per-OS table needs updating"
    );
}
