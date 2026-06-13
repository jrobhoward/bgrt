//! Tests for the energy-classified rayon pool.
#![allow(non_snake_case)]

use crate::{QosClass, RayonBuilder};

#[test]
fn rayon_pool____install____runs_closure_on_pool() {
    let pool = RayonBuilder::new().build().unwrap();
    let result = pool.install(|| 6 * 7);
    assert_eq!(result, 42);
}

#[test]
fn builder____defaults____are_background_class() {
    let pool = RayonBuilder::new().build().unwrap();
    assert_eq!(pool.qos(), QosClass::Background);
}

#[test]
fn builder____num_threads____sets_pool_size() {
    let pool = RayonBuilder::new().num_threads(3).build().unwrap();
    assert_eq!(pool.current_num_threads(), 3);
}

#[test]
fn builder____zero_threads____lets_rayon_decide() {
    // rayon clamps 0 to its default; we don't interfere.
    let pool = RayonBuilder::new().num_threads(0).build().unwrap();
    assert!(pool.current_num_threads() >= 1);
}

// Linux: pool threads should carry nice 19 for Background.
#[cfg(target_os = "linux")]
mod linux {
    use crate::test_support::current_nice;
    use crate::{QosClass, RayonBuilder};

    #[test]
    fn background_pool____install____threads_are_nice_19() {
        let pool = RayonBuilder::new()
            .qos(QosClass::Background)
            .num_threads(1)
            .build()
            .unwrap();
        let nice = pool.install(current_nice);
        assert_eq!(nice, 19);
    }
}

// macOS: pool threads should carry the macOS background QoS class.
#[cfg(target_os = "macos")]
mod macos {
    use crate::test_support::{QOS_CLASS_BACKGROUND, current_qos};
    use crate::{QosClass, RayonBuilder};

    #[test]
    fn background_pool____install____threads_are_background_qos() {
        let pool = RayonBuilder::new()
            .qos(QosClass::Background)
            .num_threads(1)
            .build()
            .unwrap();
        let qos = pool.install(current_qos);
        assert_eq!(qos, QOS_CLASS_BACKGROUND);
    }
}
