//! Run latency-sensitive work and quiet work in the same process, by keeping a
//! Default-class runtime and a Background-class runtime side by side.
//!
//! ```text
//! cargo run --example mixed_runtimes -p bgrt
//! ```

use bgrt::{QosClass, RuntimeBuilder};

fn main() -> Result<(), bgrt::Error> {
    let foreground = RuntimeBuilder::new()
        .qos(QosClass::Default)
        .thread_name("fg-worker")
        .build()?;
    let background = RuntimeBuilder::new()
        .qos(QosClass::Background)
        .thread_name("bg-worker")
        .build()?;

    // Quiet, deprioritized work goes to the background runtime...
    let bg = background.spawn(async {
        let mut acc = 0u64;
        for i in 0..5_000_000u64 {
            acc = acc.wrapping_add(i);
        }
        acc
    });

    // ...while responsive work goes to the foreground runtime.
    let fg = foreground.spawn(async { "responsive result" });

    // Drive both to completion from the foreground runtime.
    let (fg_out, bg_out) = foreground.block_on(async move { (fg.await, bg.await) });
    println!("foreground => {fg_out:?}");
    println!("background => {bg_out:?}");
    Ok(())
}
