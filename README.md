# bgrt

**bgrt** ("background runtime") runs your async tasks and threads at the lowest
energy footprint the OS allows — on **efficiency cores**, at **low clock
frequency**, **without spinning up the fans** — while still making forward
progress when the machine is busy. Write ordinary async/sync Rust and schedule
it onto a quiet executor.

- **Per-thread energy QoS** — classify work as `Background`, `Utility`, or
  `Default`, mapped to each OS's native facility: macOS QoS classes, Windows
  EcoQoS, Linux `nice` + `ioprio` (+ opt-in efficiency-core affinity and `uclamp`
  frequency cap).
- **CPU *and* disk** — one class covers both, on all three platforms. Quiet work
  doesn't thrash the disk either.
- **No admin required** — the library only ever *lowers* its own threads' demands.
- **Wraps tokio, integrates with rayon** — an energy-classified async runtime,
  a quiet rayon thread pool for parallel iterators, and a quiet OS-thread spawner
  for the non-async path.
- **Never starves** — quiet work uses weighted-fair low priority, so it always
  crawls forward under load (not run-only-when-idle).
- **Cross-platform** — macOS, Windows, Linux, including big.LITTLE / P+E CPUs
  (though the hybrid-Linux path is
  [unverified on real P+E hardware](#hybrid-linux-is-implemented-but-unmeasured)).

> **Status:** the library (QoS backends, runtime wrapper, quiet-thread spawner,
> rayon pool) and the measurement harness are implemented and tested. macOS (M1)
> and Linux (Threadripper, AMD x86 — both *homogeneous*) are run-verified;
> Windows runs in CI. Efficiency-core pinning on hybrid Linux is
> [implemented but unmeasured](#hybrid-linux-is-implemented-but-unmeasured).
> Design:
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

// Single-threaded task semantics: tokio's current-thread scheduler, driven on
// one OS thread that bgrt spawns and classifies (never the caller's thread).
let rt = bgrt::RuntimeBuilder::new().current_thread(true).build()?;

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
| `Background` | `QOS_CLASS_BACKGROUND` (efficiency cores, throttled I/O) | background mode (throttled I/O) + EcoQoS + below-normal | `nice(19)` + I/O best-effort 7 + opt-in E-core affinity + opt-in `uclamp` frequency cap |
| `Utility` | `QOS_CLASS_UTILITY` | EcoQoS + normal | `nice(10)` + I/O best-effort 6 |
| `Default` | none | none | `nice(0)`, I/O untouched |

A class governs **CPU and block I/O together**, not CPU alone — one knob, because
two of three platforms bundle the axes and an API offering combinations it can't
honour would be worse than a coarser one that always means what it says. The
`Background` I/O mapping is weighted-fair (Linux best-effort 7, deliberately
*not* `IOPRIO_CLASS_IDLE`), for the same anti-starvation reason `nice(19)` is
used over `SCHED_IDLE`.

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

# Linux, hybrid CPU (P+E, e.g. Alder/Raptor/Meteor Lake) — pin to E-cores.
# Unverified on real P+E hardware; see "Hybrid Linux is implemented but
# unmeasured" below. If you have such a machine, this is the run to send us.
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

### Measured on Linux / AMD Threadripper (homogeneous, with `sudo … --clamp-frequency`)

Same machine, `schedutil` governor confirmed, 10 s run with `sudo` so RAPL energy
is available:

```text
executor              wall_ms       work      work/s     %E  mean_mhz  max_mhz  energy_j
default                 10000    2898328      289830    n/a      3685     3692   605.257
utility                 10000    2898107      289809    n/a      3685     3692   614.536
background              10000    1717806      171779    n/a      2188     2200   492.676
background-threads      10000    1723344      172331    n/a      2195     2200   485.833
verdict: background peak frequency ≤ (stayed cool) default
```

`uclamp` does bite on Threadripper too: `Background` ran at **2188 MHz mean vs 3685 MHz**
for `Default` (~1.7× lower clock) and drew **~1.2× less package energy** (493 J vs 605 J
over 10 s), at ~59% of the throughput. The frequency drop is shallower than on the Sandy
Bridge i7 below — Threadripper's 16-core package has much higher fixed power, so the
per-core clock reduction moves the package needle less. Per-unit-work energy is modestly
*worse* for `Background` on this machine for the same reason as the i7 (see caveat below).

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
| core placement / %E | ✅ sysfs | ✅ `GetSystemCpuSetInformation` | needs `powermetrics` |
| frequency | ✅ sysfs | ✅ `CallNtPowerInformation` | needs `powermetrics` |
| energy | ✅ RAPL (often root) | — | `--mac-power` (needs `sudo`) |

So on **Linux** and **Windows** you get placement and frequency unprivileged
(Linux energy may need root for RAPL; Windows has no energy counter at all); on
**macOS** core/frequency/power need `sudo powermetrics` (use `--mac-power`). The
library itself never needs privileges — only this measurement tool does.

## Limitations & notes

### Hybrid Linux is implemented but unmeasured

**`pin_efficiency_cores` has never been run on a heterogeneous (P+E) Linux
machine.** Every Linux measurement in this README is from a homogeneous CPU — an
AMD Threadripper and an Intel Sandy Bridge i7 — neither of which has efficiency
cores or exposes the `cpu_capacity` sysfs entries the detection relies on. On
those machines the feature correctly does nothing, which is exactly the result
that cannot distinguish "works" from "silently broken".

Concretely, what is and isn't verified on Linux:

| Piece | Status |
|---|---|
| `nice` mapping per QoS class | ✅ run-verified (tests assert `nice 19`) |
| `uclamp` frequency cap | ✅ run-verified, two machines (tables above) |
| `topology::select_efficiency_cores` (the selection logic) | ✅ unit-tested, incl. hybrid and three-tier layouts |
| Reading `cpu_capacity` from sysfs on a real hybrid CPU | ❌ never executed — no such hardware available |
| `sched_setaffinity` pinning to detected E-cores | ❌ never executed against a non-empty core set |

So on an Alder Lake / Raptor Lake / Meteor Lake box, `--pin` and
`pin_efficiency_cores(true)` should work — the syscall path is straightforward
and the selection logic is tested — but treat them as **untested code, not a
measured feature**, until someone runs `cargo run --release -p bgrt-bench --
--duration 3 --pin` on real P+E silicon. Reports welcome. macOS needs none of
this: `QOS_CLASS_BACKGROUND` is E-core-confined by the kernel, and that path *is*
measured.

The same caveat applies more narrowly to **Windows E/P telemetry**. The
`GetSystemCpuSetInformation` read runs in CI on every push, so the API call and
the homogeneous case are verified — but CI runners are homogeneous VMs, so the
branch that actually labels a core "efficiency" has only ever been exercised by
unit tests of the selection logic. Windows *placement* is unaffected either way:
that is EcoQoS's job, not ours.

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
### Disk I/O: what each platform actually does

A `QosClass` covers CPU *and* block I/O everywhere, but through three different
mechanisms with three different caveats:

| | Mechanism | Caveat |
|---|---|---|
| macOS | `QOS_CLASS_BACKGROUND` implies disk-I/O throttling — one call, both axes | not directly observable — see below |
| Linux | explicit `ioprio_set` to best-effort 7 (`Background`) / 6 (`Utility`) | **inert under some I/O schedulers** — see below |
| Windows | background processing mode (`THREAD_MODE_BACKGROUND_BEGIN`) | `Background` only; weaker scheduling guarantee — see below |

**macOS: the coverage is real but not directly assertable.** `getiopolicy_np`
reports only a thread's *explicit* I/O override, so a background-QoS thread reads
`IOPOL_DEFAULT` even while Darwin is throttling it. Setting the policy outright
would make it readable — but measurably **opts the thread out of QoS entirely**,
losing efficiency-core confinement and the energy win that is the whole point.
So `bgrt` takes Darwin's bundled behaviour and leaves the override alone. The
disk half of a class on macOS therefore rests on Apple's documentation rather
than on a measurement.

**Linux: whether it bites depends on your I/O scheduler.** BFQ honours it fully;
`mq-deadline` since kernel 5.18; `none` — a common default for NVMe — ignores it
entirely, making the call a no-op. Same shape of caveat as `uclamp` needing
`schedutil`. Check with `cat /sys/block/<dev>/queue/scheduler`.

**Windows: `Background` gets quiet I/O, `Utility` does not.** Background
processing mode is the only documented per-thread I/O lever, and it is
all-or-nothing — taking it for `Utility` would drag CPU priority down too, which
is exactly what distinguishes the two classes.

Microsoft documents that a thread in background mode *"may not be scheduled
promptly, but it will never be starved"*. That satisfies this crate's
never-starve rule, but it is a **weaker promise than Linux's weighted-fair
share**: Windows delivers it by periodically boosting a thread that has been
denied the CPU for too long, so expect poor throughput under sustained
foreground load. That is the quiet end of the range, by design — but it has not
been *measured* on real Windows hardware, only reasoned from the documentation
and from what Chromium ships.

Two further Windows notes:

- **`bgrt` never uses `PROCESS_MODE_BACKGROUND_BEGIN`**, the process-wide
  sibling. It carries an undocumented hard 32 MiB working-set cap that has been
  measured making programs 250–800× slower; Mozilla investigated it and closed
  the idea WONTFIX, and Chromium dropped it. Both recommended the per-thread flag
  `bgrt` uses instead. Since `bgrt` classifies threads rather than processes, the
  dangerous variant is unreachable by design.
- **Background mode also lowers memory priority**, so the thread's pages get
  trimmed first. `bgrt` puts memory priority back to normal immediately — trimmed
  pages get faulted back in, costing the very disk I/O this class is trying to
  avoid. Chromium does the same thing for the same reason.

- **`bgrt` classifies CPU and disk work, not GPU work** — and GPU is very
  unlikely to ever be in scope; see
  [Scope: what `bgrt` is not](#scope-what-bgrt-is-not) below.
- **Telemetry availability varies** (see the table above): Linux is fullest
  unprivileged; macOS frequency/power/residency need `sudo powermetrics`; Windows
  reports frequency, CPU index, and E/P classification, but has no energy
  counter. Linux RAPL energy is often root-only. The *library* never needs
  privileges — only measurement does.

## Scope: what `bgrt` is not

**I/O priority — in scope and shipped on all three platforms.** A `QosClass`
governs disk demands as well as CPU; see the section above for the per-platform
mechanisms and caveats. Split CPU/I/O control (asking for quiet CPU but normal
I/O, or the reverse) is *not* offered, because two of three platforms can't
express it — reasoning in
[`docs/DESIGN.md`](docs/DESIGN.md#scope-cpu-and-io-now-gpu-probably-never).

**GPU — probably never.** Not on the roadmap, and it would take a shift in the
platforms to get there. As things stand today:

- There is **no OS-level per-thread GPU QoS** on macOS, Windows, or Linux. The
  whole library rests on one primitive all three expose with the same meaning,
  and no GPU equivalent exists. What exists is per-API and mutually incompatible
  (Vulkan, CUDA, D3D12, Metal — the last has no queue priority at all).
- Those APIs **arbitrate contention; they are not energy levers.** Lowering GPU
  priority makes your work wait. It doesn't downclock the GPU or move work to
  lower-power units — and a GPU idling at high clocks while your work waits can
  burn *more* energy for the same result.
- On Windows there's **no tier below normal** to ask for (D3D12 offers normal,
  high, global-realtime), so the central operation — "ask for less" — has no
  expression.
- **The threading model doesn't transfer.** `bgrt` classifies a thread once at
  start; GPU work is submitted to a queue owned by a device context. There's no
  thread to classify.
- **For AI workloads the real levers are elsewhere:** choosing a low-power
  compute unit (ANE via CoreML, NPU via DirectML, integrated over discrete),
  cutting batch size and concurrency, or quantizing. Those are model- and
  framework-level decisions a thread-QoS crate can't make, and shipping a GPU
  knob here would look like it saves energy without doing so.

GPU energy matters — it's that *today* the lever isn't a scheduling hint, so it
doesn't belong in a scheduling-hint library. Four of those five objections
describe what the platforms currently expose, not a principle, so the question is
worth reopening if any of them change: an OS shipping a per-context GPU energy
QoS that's unprivileged to lower; graphics APIs growing an eco tier that affects
clocks or unit placement rather than only queue order; inference runtimes
converging on a portable low-power mode; or GPU submissions inheriting the
classification of the thread that queued them. The conditions are spelled out in
[`docs/DESIGN.md`](docs/DESIGN.md#gpu--probably-never-but-heres-what-would-change-that).

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -Dwarnings
cargo run --release -p bgrt-bench -- --duration 3
```

Requires Rust ≥ 1.85 (edition 2024). See [`CLAUDE.md`](CLAUDE.md) for
architecture and conventions.
