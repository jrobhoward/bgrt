//! `bgrt-bench` — compare execution time, throughput, core placement, CPU
//! frequency, and energy across bgrt executors.
//!
//! Each selected executor runs the same CPU-bound workload for a fixed duration
//! while self-sampling telemetry (and, with `--mac-power`, sampling
//! `powermetrics`); results are printed as a table or JSON.

mod power;
mod report;
mod runner;
mod workload;

#[cfg(target_os = "macos")]
mod power_macos;

use std::time::Duration;

use clap::{Parser, ValueEnum};

use crate::report::Summary;
use crate::runner::Executor;
use crate::workload::WorkloadConfig;

/// Output format.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    /// Aligned text table.
    Table,
    /// Pretty JSON.
    Json,
}

/// Compare execution characteristics across bgrt executors.
#[derive(Debug, Parser)]
#[command(about, long_about = None)]
struct Args {
    /// Run duration per executor, in seconds.
    #[arg(long, default_value_t = 3.0)]
    duration: f64,

    /// Concurrent workers (tasks/threads) per executor.
    #[arg(long, default_value_t = 1)]
    workers: usize,

    /// Telemetry self-sample interval, in milliseconds.
    #[arg(long, default_value_t = 50)]
    interval: u64,

    /// Executors to run (comma-separated). Defaults to all.
    #[arg(long, value_enum, num_args = 1.., value_delimiter = ',')]
    executors: Vec<Executor>,

    /// Output format.
    #[arg(long, value_enum, default_value = "table")]
    format: Format,

    /// Pin bgrt runtimes/threads to efficiency cores (Linux only).
    #[arg(long)]
    pin: bool,

    /// Also sample CPU power/frequency via `powermetrics` (macOS only; needs sudo).
    #[arg(long)]
    mac_power: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if args.mac_power && !cfg!(target_os = "macos") {
        eprintln!("--mac-power is only supported on macOS; ignoring");
    }

    let executors = if args.executors.is_empty() {
        vec![
            Executor::Default,
            Executor::Utility,
            Executor::Background,
            Executor::BackgroundThreads,
        ]
    } else {
        args.executors.clone()
    };

    let cfg = WorkloadConfig {
        duration: Duration::from_secs_f64(args.duration),
        sample_interval: Duration::from_millis(args.interval),
        workers: args.workers.max(1),
    };

    let mut summaries = Vec::with_capacity(executors.len());
    for executor in executors {
        eprintln!("running {} for {:.1}s ...", executor.label(), args.duration);
        let result = runner::run(executor, cfg, args.pin, args.mac_power)?;
        summaries.push(Summary::from_result(&result));
    }

    match args.format {
        Format::Table => println!("{}", report::table(&summaries)),
        Format::Json => println!("{}", report::json(&summaries)?),
    }

    if let Some(cooler) = report::background_not_hotter(&summaries) {
        eprintln!(
            "verdict: background peak frequency {} default",
            if cooler {
                "≤ (stayed cool)"
            } else {
                "> (ran hot!)"
            }
        );
    }

    Ok(())
}
