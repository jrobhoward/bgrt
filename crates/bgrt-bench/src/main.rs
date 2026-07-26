//! `bgrt-bench` — compare execution time, throughput, core placement, CPU
//! frequency, energy, and **disk behaviour** across bgrt executors.
//!
//! The CPU workload runs the same compute loop on each executor for a fixed
//! duration while self-sampling telemetry (and, with `--mac-power`, sampling
//! `powermetrics`). The disk workload (`--workload io`) reads the scratch file
//! randomly, bypassing the page cache, both alone and against a plain foreground
//! reader. Results are printed as a table or JSON.

mod io_file;
mod io_runner;
mod io_workload;
mod power;
mod report;
mod runner;
mod workload;

#[cfg(target_os = "macos")]
mod power_macos;

use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, ValueEnum};

use crate::io_file::{CacheBypass, ScratchFile};
use crate::io_runner::IoPlan;
use crate::io_workload::IoConfig;
use crate::report::{IoReport, IoSummary, Summary};
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

/// Which half of a `QosClass` to exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Workload {
    /// CPU-bound loop (default).
    Cpu,
    /// Random reads against a scratch file, solo and under contention.
    Io,
    /// Both, in that order.
    Both,
}

/// Compare execution characteristics across bgrt executors.
#[derive(Debug, Parser)]
#[command(about, long_about = None)]
struct Args {
    /// Run duration per executor, in seconds. With `--workload io` this is the
    /// duration of *each* phase (baseline, solo, contended).
    #[arg(long, default_value_t = 3.0)]
    duration: f64,

    /// Concurrent workers (tasks/threads) per executor.
    #[arg(long, default_value_t = 1)]
    workers: usize,

    /// Telemetry self-sample interval, in milliseconds (CPU workload only).
    #[arg(long, default_value_t = 50)]
    interval: u64,

    /// Executors to run (comma-separated). Defaults to all.
    #[arg(long, value_enum, num_args = 1.., value_delimiter = ',')]
    executors: Vec<Executor>,

    /// Which workload to run.
    #[arg(long, value_enum, default_value = "cpu")]
    workload: Workload,

    /// Output format.
    #[arg(long, value_enum, default_value = "table")]
    format: Format,

    /// Pin bgrt runtimes/threads to efficiency cores (Linux only).
    #[arg(long)]
    pin: bool,

    /// Cap background threads' CPU frequency via `uclamp` (Linux only; needs the
    /// schedutil governor and kernel ≥ 5.8 to take effect).
    #[arg(long)]
    clamp_frequency: bool,

    /// Also sample CPU power/frequency via `powermetrics` (macOS only; needs sudo).
    #[arg(long)]
    mac_power: bool,

    /// Directory for the disk workload's scratch file (default: temp dir).
    #[arg(long)]
    io_dir: Option<PathBuf>,

    /// Scratch file size in MiB. Larger is better: it keeps the device's own
    /// cache from serving the reads.
    #[arg(long, default_value_t = 512)]
    io_file_size_mib: u64,

    /// Read size in KiB; must be a multiple of 4 (direct-I/O alignment).
    #[arg(long, default_value_t = 64)]
    io_block_kib: usize,

    /// Plain (unclassified) foreground reader threads to contend with.
    #[arg(long, default_value_t = 1)]
    io_foreground: usize,

    /// Keep the scratch file instead of deleting it, so repeat runs reuse it.
    #[arg(long)]
    io_keep: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    warn_about_inapplicable_flags(&args);

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

    let cpu = match args.workload {
        Workload::Cpu | Workload::Both => Some(run_cpu(&args, &executors)?),
        Workload::Io => None,
    };
    let io = match args.workload {
        Workload::Io | Workload::Both => Some(run_io(&args, &executors)?),
        Workload::Cpu => None,
    };

    print_results(&args, cpu.as_deref(), io.as_ref())?;
    print_verdicts(cpu.as_deref(), io.as_ref());
    Ok(())
}

fn warn_about_inapplicable_flags(args: &Args) {
    if args.mac_power && !cfg!(target_os = "macos") {
        eprintln!("--mac-power is only supported on macOS; ignoring");
    }
    if args.clamp_frequency && !cfg!(target_os = "linux") {
        eprintln!("--clamp-frequency only affects Linux; it is a no-op elsewhere");
    }
}

/// Run the CPU workload on every executor.
fn run_cpu(
    args: &Args,
    executors: &[Executor],
) -> Result<Vec<Summary>, Box<dyn std::error::Error>> {
    let cfg = WorkloadConfig {
        duration: Duration::from_secs_f64(args.duration),
        sample_interval: Duration::from_millis(args.interval),
        workers: args.workers.max(1),
    };

    let mut summaries = Vec::with_capacity(executors.len());
    for &executor in executors {
        eprintln!("running {} for {:.1}s ...", executor.label(), args.duration);
        let result = runner::run(
            executor,
            cfg,
            args.pin,
            args.clamp_frequency,
            args.mac_power,
        )?;
        summaries.push(Summary::from_result(&result));
    }
    Ok(summaries)
}

/// Run the disk workload: one shared foreground baseline, then solo and
/// contended phases per executor.
fn run_io(args: &Args, executors: &[Executor]) -> Result<IoReport, Box<dyn std::error::Error>> {
    let block = io_file::validate_block(args.io_block_kib.saturating_mul(1024))?;
    let size = args.io_file_size_mib.saturating_mul(1024 * 1024);
    if size < block as u64 * 16 {
        return Err(format!(
            "--io-file-size-mib is too small for a {} KiB block; use at least {} MiB",
            args.io_block_kib,
            (block as u64 * 16).div_ceil(1024 * 1024)
        )
        .into());
    }

    let dir = args.io_dir.clone().unwrap_or_else(std::env::temp_dir);
    eprintln!(
        "preparing {} MiB scratch file in {} ...",
        args.io_file_size_mib,
        dir.display()
    );
    let scratch = ScratchFile::create(&dir, size, args.io_keep)?;
    let plan = IoPlan {
        cfg: IoConfig {
            duration: Duration::from_secs_f64(args.duration),
            block,
            blocks: io_file::blocks_in(scratch.size(), block),
        },
        path: scratch.path().to_path_buf(),
        workers: args.workers.max(1),
        foreground: args.io_foreground.max(1),
        pin: args.pin,
        clamp: args.clamp_frequency,
    };

    eprintln!(
        "measuring foreground baseline for {:.1}s ...",
        args.duration
    );
    let baseline = io_runner::foreground_baseline(&plan);

    let mut bypass = baseline.bypass;
    let mut rows = Vec::with_capacity(executors.len());
    for &executor in executors {
        eprintln!(
            "running {} disk phases (solo + contended, {:.1}s each) ...",
            executor.label(),
            args.duration
        );
        let row = IoSummary::from_result(&io_runner::run(executor, &plan)?, &baseline);
        bypass = bypass.merge(row.cache_bypass);
        rows.push(row);
    }

    Ok(IoReport {
        foreground_baseline_mib_s: baseline.mib_per_s(),
        cache_bypass: bypass,
        io_scheduler: io_file::io_scheduler(scratch.path()),
        rows,
    })
}

fn print_results(
    args: &Args,
    cpu: Option<&[Summary]>,
    io: Option<&IoReport>,
) -> Result<(), Box<dyn std::error::Error>> {
    match args.format {
        Format::Table => {
            if let Some(cpu) = cpu {
                println!("{}", report::table(cpu));
            }
            if let Some(io) = io {
                println!("{}", io_preamble(io));
                println!("{}", report::io_table(io));
            }
        }
        Format::Json => match (cpu, io) {
            (Some(cpu), None) => println!("{}", report::json(cpu)?),
            (None, Some(io)) => println!("{}", report::io_json(io)?),
            (Some(cpu), Some(io)) => println!("{}", report::combined_json(cpu, io)?),
            (None, None) => {}
        },
    }
    Ok(())
}

/// The context a disk table is meaningless without: the baseline it is measured
/// against, whether the reads reached the device, and (Linux) whether the I/O
/// scheduler honours priority at all.
fn io_preamble(io: &IoReport) -> String {
    let mut out = format!(
        "disk: foreground baseline {:.1} MiB/s, reads {}",
        io.foreground_baseline_mib_s,
        io.cache_bypass.label()
    );
    if let Some(sched) = &io.io_scheduler {
        out.push_str(&format!(", scheduler {sched}"));
    }
    if io.cache_bypass == CacheBypass::Buffered {
        out.push_str(
            "\nwarning: cache bypass unavailable here — reads may be served from the page\n\
             cache, so these are not disk numbers. Try --io-dir on a real filesystem.",
        );
    }
    if io.io_scheduler.as_deref() == Some("none") {
        out.push_str(
            "\nnote: the `none` I/O scheduler ignores I/O priority entirely, so a null\n\
             result here is the scheduler's doing, not the class's. Try bfq or mq-deadline.",
        );
    }
    out
}

fn print_verdicts(cpu: Option<&[Summary]>, io: Option<&IoReport>) {
    if let Some(cooler) = cpu.and_then(report::background_not_hotter) {
        eprintln!(
            "verdict: background peak frequency {} default",
            if cooler {
                "≤ (stayed cool)"
            } else {
                "> (ran hot!)"
            }
        );
    }
    if let Some(yielded) = io.and_then(|io| report::background_yields_disk(&io.rows)) {
        eprintln!(
            "verdict: background left the foreground {} disk throughput than default did",
            if yielded {
                "≥ (got out of the way)"
            } else {
                "< (crowded it out!)"
            }
        );
    }
    // MSRV 1.85 predates let-chains, so this stays a nested `if`.
    if io.is_some_and(|io| !report::device_saturated(&io.rows)) {
        eprintln!(
            "note: `default` barely dented the foreground, so the device still had \
             headroom.\n      Raise --workers and --io-foreground (e.g. 4 and 4) to make \
             the classes separate."
        );
    }
}
