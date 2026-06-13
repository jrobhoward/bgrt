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
    use crate::{QosClass, ThreadBuilder, spawn_thread};

    const QOS_CLASS_UTILITY: u32 = 0x11;
    const QOS_CLASS_BACKGROUND: u32 = 0x09;

    unsafe extern "C" {
        fn pthread_get_qos_class_np(
            thread: libc::pthread_t,
            qos_class: *mut u32,
            relative_priority: *mut i32,
        ) -> i32;
    }

    fn current_qos() -> u32 {
        let mut qos = 0u32;
        let mut rel = 0i32;
        // SAFETY: reads the calling thread's QoS into stack-local out-parameters.
        let rc = unsafe { pthread_get_qos_class_np(libc::pthread_self(), &mut qos, &mut rel) };
        assert_eq!(rc, 0);
        qos
    }

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
    use crate::{QosClass, spawn_thread};

    fn current_nice() -> i32 {
        // SAFETY: clears errno then reads the calling thread's nice value.
        unsafe {
            *libc::__errno_location() = 0;
            libc::getpriority(libc::PRIO_PROCESS as _, 0)
        }
    }

    #[test]
    fn spawn_thread____background____thread_is_nice_19() {
        let h = spawn_thread(QosClass::Background, current_nice);
        assert_eq!(h.join().unwrap(), 19);
    }
}
