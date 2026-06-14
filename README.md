# bgrt

**bgrt** ("background runtime") runs your async tasks and threads at the lowest
energy footprint the OS allows — on **efficiency cores**, at **low clock
frequency**, **without spinning up the fans** — while still making forward
progress when the machine is busy. Write ordinary async/sync Rust and schedule
it onto a quiet executor.

- **Per-thread energy QoS** — classify work as `Background`, `Utility`, or
  `Default`, mapped to each OS's native facility: macOS QoS classes, Windows
  EcoQoS, Linux `nice` (+ opt-in efficiency-core affinity and `uclamp` frequency cap).
- **No admin required** — the library only ever *lowers* its own threads' demands.
- **Wraps tokio, integrates with rayon** — an energy-classified async runtime,
  a quiet rayon thread pool for parallel iterators, and a quiet OS-thread spawner
  for the non-async path.
- **Never starves** — quiet work uses weighted-fair low priority, so it always
  crawls forward under load (not run-only-when-idle).
- **Cross-platform** — macOS, Windows, Linux, including big.LITTLE / P+E CPUs.

> **Status:** the library (QoS backends, runtime wrapper, quiet-thread spawner,
> rayon pool) and the measurement harness are implemented and tested. macOS (M1)
> and Linux (Threadripper, AMD x86) are run-verified; Windows is cross-compiled
> and lint-clean pending a run on real hardware. Design:
> [`docs/DESIGN.md`](docs/DESIGN.md); plan: [`docs/ROADMAP.md`](docs/ROADMAP.md);
> state: [`CHANGELOG.md`](CHANGELOG.md).

## Usage

```rust
use bgrt::QosClass;

// A quiet async executor (wraps a tokio runtime; feature "tokio", on by default).
let rt = bgrt::RuntimeBuilder::new()
    .qos(QosClass::Background)
    .worker_threads(1)
    .build()?;
rt.spawn(async { /* quiet async work */ });

// A quiet rayon thread pool (feature "rayon", opt-in).
let pool = bgrt::RayonBuilder::new()
    .qos(QosClass::Background)
    .build()?;
pool.install(|| data.par_iter().for_each(|x| process(x)));

// A quiet OS thread (no features needed).
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

## Feature flags

| Feature | Default | Adds |
|---------|---------|------|
| `tokio` | ✅ on | `RuntimeBuilder`, `Runtime` (async executor) |
| `rayon` | ❌ off | `RayonBuilder`, `RayonPool` (parallel iterators) |
| `telemetry` | ❌ off | measurement primitives used by `bgrt-bench` |

`default-features = false` gives a minimal dep tree: just `QosClass`, `apply`,
`spawn_thread`, and `ThreadBuilder` — no tokio, no rayon.

## QoS classes

| Class | macOS | Windows | Linux |
|---|---|---|---|
| `Background` | `QOS_CLASS_BACKGROUND` (efficiency cores) | EcoQoS + below-normal | `nice(19)` + opt-in E-core affinity + opt-in `uclamp` frequency cap |
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
E-core affinity), `--clamp-frequency` (Linux `uclamp` frequency cap on background
work), `--mac-power` (macOS `%E`/frequency/power via `powermetrics`).

### Running on each platform

The privilege-free **`work/s`** comparison runs the same way everywhere; the
extra flags unlock platform-specific placement/frequency/energy detail.

```bash
# Any platform — throughput comparison, no privileges, no extra flags:
cargo run --release -p bgrt-bench -- --duration 3

# macOS — add %E / frequency / CPU power from powermetrics (needs sudo).
# Build first, then run the *binary* under sudo: `cargo run` as root would
# rebuild as root and may not find your toolchain.
cargo build --release -p bgrt-bench
sudo ./target/release/bgrt-bench --duration 3 --mac-power

# Linux, hybrid CPU (P+E, e.g. Alder/Raptor/Meteor Lake) — pin to E-cores:
cargo run --release -p bgrt-bench -- --duration 3 --pin

# Linux, homogeneous CPU (no E-cores) — uclamp is the only frequency lever;
# needs the schedutil governor + kernel >= 5.8 to bite (see note below):
cargo run --release -p bgrt-bench -- --duration 3 --clamp-frequency

# Linux — RAPL energy fills in automatically when readable; if it shows n/a,
# the counters need root: build, then run the binary under sudo:
cargo build --release -p bgrt-bench
sudo ./target/release/bgrt-bench --duration 3

# Windows — frequency + placement are unprivileged; just run it:
cargo run --release -p bgrt-bench -- --duration 3
```

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

### Measured on Linux / AMD Threadripper (homogeneous, 16-core, with `sudo`)

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 15000    4350512      290033    n/a      3687     3692   960.534
utility                 15000    4350058      290003    n/a      3687     3692   982.798
background              15000    4348280      289885    n/a      3687     3692   969.635
background-threads      15000    4349948      289993    n/a      3687     3692   974.817
verdict: background peak frequency ≤ (stayed cool) default
```

All executors are identical — the expected null result on a homogeneous CPU. Two
reasons `nice(19)` shows nothing here:

1. **No contention:** one active thread, no competing load. `nice` only
   deprioritizes when other threads are competing for the same core.
2. **No E-cores:** Threadripper has no `cpu_capacity` sysfs entries, so
   efficiency-core affinity is a no-op and there's no DVFS difference from nice alone.

The `energy_j` variance (<3%) is measurement noise from RAPL reading the entire
16-core package, not per-thread power. Meaningful Linux results need a
heterogeneous (P+E) CPU (Alder Lake, Raptor Lake, Meteor Lake) or a CPU-loaded
machine where scheduling priority actually changes which threads run.

To get a *frequency* effect on a homogeneous CPU, add `--clamp-frequency`: it
applies a `uclamp` cap to background work so the governor picks a lower clock even
at 100% busy (`nice` alone gives the governor no frequency input). It only bites
with the `schedutil` governor (or `intel_pstate=passive`) on kernel ≥ 5.8 — under
a fixed governor or HWP the cap is inert. Check with
`cat /sys/devices/system/cpu/cpufreq/policy0/scaling_governor`. Like the library
itself, the clamp is unprivileged (it only ever *lowers* `util_max`).

### Measured on Linux / Intel i7-2720QM (homogeneous, with `--clamp-frequency`)

With the `schedutil` governor and `--clamp-frequency`, the homogeneous-CPU null
result flips to dramatic — here on a 2011 Sandy Bridge i7 (4C/8T, no E-cores):

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 10000    2499377      249935    n/a      3192     3289   184.103
utility                 10000    2545955      254592    n/a      3245     3289   169.710
background              10000     651358       65135    n/a       840     2990    57.497
background-threads      10000     629097       62906    n/a       802     1295    50.947
verdict: background peak frequency ≤ (stayed cool) default
```

`Background` ran at a **840 MHz mean clock vs 3192 MHz** for `Default` (~3.8× lower)
and drew **~3.2× less CPU energy over the 10 s window** (57 J vs 184 J), at ~26% of
the throughput. This is `uclamp` doing exactly what `nice` alone could not on a
homogeneous CPU. `Utility` is left unclamped by design and tracks `Default`.

**Honest caveat — this is a stay-cool / low-power-draw lever, not a per-unit-work
efficiency win.** Dividing energy by work, `Background` here actually spends
*slightly more* per work-unit (~88 vs ~74 µJ): at low clocks, fixed and leakage
power dominate, so on this old silicon "race to idle" would finish a fixed batch
for marginally less total energy. The payoff is lower *instantaneous* power, a
cooler and quieter machine, and not stealing thermal/power budget from foreground
work — not a smaller battery bill for a fixed amount of work. (Contrast the macOS
result above, where efficiency-core *placement* cuts energy ~4× **per unit of
work**: a different mechanism with a different trade-off.)

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
