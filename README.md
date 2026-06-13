# bgrt

**bgrt** ("background runtime") runs your async tasks and threads at the lowest
energy footprint the OS allows — on **efficiency cores**, at **low clock
frequency**, **without spinning up the fans** — while still making forward
progress when the machine is busy. Write ordinary async/sync Rust and schedule
it onto a quiet executor.

- **Per-thread energy QoS** — classify work as `Background`, `Utility`, or
  `Default`, mapped to each OS's native facility: macOS QoS classes, Windows
  EcoQoS, Linux `nice` + efficiency-core affinity.
- **No admin required** — the library only ever *lowers* its own threads' demands.
- **Wraps tokio** (no fork) — an energy-classified runtime, plus a quiet
  OS-thread spawner for non-async work.
- **Never starves** — quiet work uses weighted-fair low priority, so it always
  crawls forward under load (not run-only-when-idle).
- **Cross-platform** — macOS, Windows, Linux, including big.LITTLE / P+E CPUs.

> **Status:** the library (QoS backends, runtime wrapper, quiet-thread spawner)
> and the measurement harness are implemented and tested; macOS is run-verified
> (see the M1 numbers below), Linux/Windows are cross-compiled and lint-clean
> pending CI on real hardware. Design: [`docs/DESIGN.md`](docs/DESIGN.md); plan:
> [`docs/ROADMAP.md`](docs/ROADMAP.md); state: [`CHANGELOG.md`](CHANGELOG.md).

## Usage

```rust
use bgrt::QosClass;

// A quiet async executor (wraps a tokio runtime).
let rt = bgrt::RuntimeBuilder::new()
    .qos(QosClass::Background)
    .worker_threads(1)
    .build()?;
rt.spawn(async { /* quiet async work */ });

// A quiet OS thread (non-async path).
let jh = bgrt::spawn_thread(QosClass::Background, || { /* CPU-bound loop */ });

// Or classify the current thread directly.
bgrt::apply(QosClass::Utility)?;
```

Run some work normally and other work quietly by keeping a `Default`-class
runtime *and* a `Background`-class `bgrt` runtime in the same process, then
spawning onto the right one.

Runnable examples (`cargo run --example <name> -p bgrt`):

- [`background_task`](crates/bgrt/examples/background_task.rs) — a quiet async task.
- [`mixed_runtimes`](crates/bgrt/examples/mixed_runtimes.rs) — foreground + background runtimes together.
- [`quiet_threads`](crates/bgrt/examples/quiet_threads.rs) — `spawn_thread` and `ThreadBuilder`.

## QoS classes

| Class | macOS | Windows | Linux |
|---|---|---|---|
| `Background` | `QOS_CLASS_BACKGROUND` (efficiency cores) | EcoQoS + below-normal | `nice(19)` + opt-in E-core affinity |
| `Utility` | `QOS_CLASS_UTILITY` | EcoQoS + normal | `nice(10)` |
| `Default` | none | none | `nice(0)` |

## Benchmarking (`bgrt-bench`)

`bgrt-bench` runs the same CPU-bound workload on each executor and compares
execution time, core placement, frequency, and energy:

```bash
cargo run --release -p bgrt-bench -- --duration 3
# executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
# default                  3000      15707        5235    n/a       n/a      n/a       n/a
# utility                  3000      15930        5310    n/a       n/a      n/a       n/a
# background               3000       4925        1641    n/a       n/a      n/a       n/a
# background-threads       3000       4965        1655    n/a       n/a      n/a       n/a
```

The headline signal is **`work/s` (throughput)**: the runs are duration-bounded,
so a quieter executor completes *less* work in the same wall time. The example
above (macOS, no sudo) shows Background doing ~31% of Default's work — the
efficiency-core confinement, measured without any privileged telemetry.

Flags: `--executors default,utility,background,background-threads` (subset/order),
`--workers <n>`, `--format json`, `--interval <ms>` (sampling), `--pin` (Linux
E-core affinity), `--mac-power` (macOS `%E`/frequency/power via `powermetrics`).

### Measured on an Apple M1 (with `sudo … --mac-power`)

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                  3000      15865        5288   37.6      2124     2751     3.482
utility                  3000      15676        5225   36.3      2095     2719     3.335
background               3000       5106        1702   99.8      1028     1029     0.275
background-threads       3000       5213        1737   98.7      1124     1132     0.491
verdict: background peak frequency ≤ (stayed cool) default
```

`Background` work ran **99.8% on efficiency cores** (vs 37.6% for `Default`),
peaked at **1029 MHz vs 2751 MHz**, and drew **~12× less CPU power** (0.275 J vs
3.482 J over the same 3 s) — at ~32% of the throughput. Per unit of work that's
still ~4× less energy. This is the "stay on the efficiency cores, keep the clocks
and fans down" goal, measured.

> `--mac-power` figures (`%E`, frequency, energy) come from `powermetrics`, which
> reports **system-wide** CPU state, not per-thread — so they reflect total CPU
> activity during the run (the right lens for fans/battery, but noisier if other
> apps are busy). The privilege-free **`work/s`** column is the cleanest
> per-executor signal.

> **macOS note (QoS promotion):** synchronously waiting on a background thread
> from a higher-QoS thread promotes it *off* the efficiency cores
> (priority-inversion avoidance), whereas an async `await` on a background
> runtime does not. The harness accounts for this in its `background-threads`
> runner. The practical takeaway: fire-and-forget background threads stay quiet,
> but if a foreground thread blocks waiting on one, macOS may speed it up.

**What's measurable per platform** (anything unavailable shows `n/a` / `null`,
never an error):

| Signal | Linux | Windows | macOS |
|---|---|---|---|
| wall-clock, samples | ✅ | ✅ | ✅ |
| core placement / %E | ✅ sysfs | cpu only (E/P deferred) | needs `powermetrics` |
| frequency | ✅ sysfs | ✅ `CallNtPowerInformation` | needs `powermetrics` |
| energy | ✅ RAPL (often root) | — | `--mac-power` (needs `sudo`) |

So on **Linux** you get the full picture unprivileged (energy may need root for
RAPL); on **macOS** core/frequency/power need `sudo powermetrics` (use
`--mac-power`); on **Windows** you get frequency + placement (E/P labelling is a
TODO). The library itself never needs privileges — only this measurement tool does.

## Limitations & notes

- **Frequency isn't directly controllable** from userspace — `bgrt` *biases*
  against clocking up (chiefly by keeping work off performance cores); it can't
  *guarantee* the clock never rises, especially under other system load.
- **Classification is once-per-thread, by design.** QoS is applied when a runtime
  worker or thread starts; there's no per-task re-classification. Pick the right
  runtime/thread for the work. (This also sidesteps that, on Linux, an
  unprivileged thread can lower its priority but **cannot raise it back**.)
- **macOS join-promotion:** synchronously waiting on a background thread from a
  higher-QoS thread can promote it off the efficiency cores (see the benchmarking
  note above). Async `await` on a background runtime does not.
- **Telemetry availability varies** (see the table above): Linux is fullest
  unprivileged; macOS frequency/power/residency need `sudo powermetrics`; Windows
  reports frequency + CPU index but not yet E/P classification. Linux RAPL energy
  is often root-only. The *library* never needs privileges — only measurement does.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -Dwarnings
cargo run --release -p bgrt-bench -- --duration 3
```

Requires Rust ≥ 1.85 (edition 2024). See [`CLAUDE.md`](CLAUDE.md) for
architecture and conventions.
