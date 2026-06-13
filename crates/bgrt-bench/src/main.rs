//! `bgrt-bench` — comparison harness.
//!
//! Measures task execution time, core placement, CPU frequency, and power across
//! bgrt executors (Default / Utility / Background runtimes and quiet threads).
//! The full comparison lands in Phase 5; today this is a telemetry smoke probe.

use bgrt::QosClass;
use bgrt::telemetry::{self, EnergyMeter};

fn main() {
    println!("bgrt-bench: comparison harness — full version in Phase 5 (see docs/ROADMAP.md).");

    if let Err(e) = bgrt::apply(QosClass::Background) {
        eprintln!("warning: could not apply qos: {e}");
    }

    let meter = EnergyMeter::start();
    let s = telemetry::sample();
    println!(
        "current sample: cpu={:?}, core_type={:?}, freq_mhz={:?}",
        s.cpu, s.core_type, s.freq_mhz
    );
    match meter.stop_uj() {
        Some(uj) => println!("energy since start: {uj} µJ"),
        None => println!("energy: unavailable (needs RAPL access / supported platform)"),
    }
}
