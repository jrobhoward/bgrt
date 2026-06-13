//! Run a quiet async task on a Background-class runtime.
//!
//! ```text
//! cargo run --example background_task -p bgrt
//! ```

use bgrt::{Builder, QosClass};

fn main() -> Result<(), bgrt::Error> {
    // A single-worker runtime whose thread runs at the lowest energy footprint.
    let rt = Builder::new()
        .qos(QosClass::Background)
        .worker_threads(1)
        .build()?;

    // Schedule ordinary async work onto it.
    let handle = rt.spawn(async {
        let mut sum = 0u64;
        for i in 0..5_000_000u64 {
            sum = sum.wrapping_add(i);
        }
        sum
    });

    match rt.block_on(handle) {
        Ok(sum) => println!("background task finished: {sum}"),
        Err(e) => eprintln!("background task failed: {e}"),
    }
    Ok(())
}
