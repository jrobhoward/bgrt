//! Spawn energy-classified OS threads — the non-async path.
//!
//! ```text
//! cargo run --example quiet_threads -p bgrt
//! ```

use bgrt::{QosClass, ThreadBuilder, spawn_thread};

fn main() -> std::io::Result<()> {
    // Simplest: a fire-and-forget Background thread (mirrors std::thread::spawn).
    let quick = spawn_thread(QosClass::Background, || {
        let mut acc = 0u64;
        for i in 0..5_000_000u64 {
            acc = acc.wrapping_add(i);
        }
        acc
    });

    // Configured: name, stack size, and (on Linux) efficiency-core pinning.
    let configured = ThreadBuilder::new()
        .qos(QosClass::Utility)
        .name("bgrt-example")
        .stack_size(512 * 1024)
        .pin_efficiency_cores(true)
        .spawn(|| "configured thread done")?;

    if let Ok(acc) = quick.join() {
        println!("background thread sum: {acc}");
    }
    if let Ok(msg) = configured.join() {
        println!("{msg}");
    }
    Ok(())
}
