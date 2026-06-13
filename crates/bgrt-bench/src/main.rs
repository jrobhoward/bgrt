//! `bgrt-bench` — comparison harness.
//!
//! Measures task execution time, core placement, CPU frequency, and power across
//! bgrt executors (Default / Utility / Background runtimes and quiet threads).
//! Phase 5 implements the full harness; this is the Phase 0 stub.

use bgrt::QosClass;

fn main() {
    println!("bgrt-bench: comparison harness — not yet implemented (see docs/ROADMAP.md, Phase 5).");

    // Exercise the library so the dependency wiring is real from Phase 0.
    match bgrt::apply(QosClass::Default) {
        Ok(()) => println!("applied QosClass::Default to the current thread (no-op in Phase 0)."),
        Err(e) => eprintln!("error: {e}"),
    }
}
