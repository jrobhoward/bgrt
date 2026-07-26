# bgrt

[![CI](https://github.com/jrobhoward/bgrt/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/jrobhoward/bgrt/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/bgrt.svg)](https://crates.io/crates/bgrt)
[![docs.rs](https://img.shields.io/docsrs/bgrt)](https://docs.rs/bgrt)
[![MSRV](https://img.shields.io/badge/MSRV-1.85.0-blue.svg)](#stability)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

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

> **Status:** feature-complete and tested; pre-1.0, heading for a 0.9 public
> preview (see [Stability](#stability)). All three backends execute in CI on
> their own OS. macOS (M1) and two homogeneous Linux machines are run-verified
> for CPU, and the M1 for [disk](#measuring-the-disk-half---workload-io) as well;
> efficiency-core pinning on hybrid Linux is
> [implemented but unmeasured](#hybrid-linux-is-implemented-but-unmeasured), and
> the disk numbers have not yet been reproduced on Linux or Windows.
>
> Design: [`docs/DESIGN.md`](docs/DESIGN.md) · plan:
> [`docs/ROADMAP.md`](docs/ROADMAP.md) · results:
> [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) · state:
> [`CHANGELOG.md`](CHANGELOG.md).

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

This table is canonical — `docs/` and the rustdoc link here rather than
restating it.

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

### Measuring the disk half (`--workload io`)

`QosClass` governs CPU *and* block I/O, so the harness measures both.
`--workload io` swaps the compute loop for random reads against a scratch file,
and `--workload both` runs the two in sequence.

```bash
# Saturate the device: the classes only separate once something is queueing.
cargo run --release -p bgrt-bench -- --workload io --duration 3 --workers 4 --io-foreground 4
```

Two design points make the numbers mean something:

- **The reads bypass the page cache** — `O_DIRECT` (Linux), `F_NOCACHE` (macOS),
  `FILE_FLAG_NO_BUFFERING` (Windows) — because a cached read measures `memcpy`,
  and an I/O class only applies to requests that reach the block layer. Where
  bypass isn't available (tmpfs, some filesystems) the run says `reads buffered`
  and warns, rather than passing cache numbers off as disk numbers.
- **Every executor runs twice: alone, then against a plain unclassified
  foreground reader.** I/O priority is a *contention* mechanism; on an idle
  device low-priority reads run at nearly full speed on all three platforms. So
  the headline column is **`fg_prot%`** — what the foreground keeps, as a
  percentage of its own uncontended baseline.

Reads only, deliberately: buffered writes are issued to the device by a flusher
thread, so they'd measure that thread's priority rather than the classified
thread's.

Disk flags: `--io-file-size-mib <n>` (default 512), `--io-block-kib <n>` (default
64, must be a multiple of 4), `--io-foreground <n>` (competing plain threads),
`--io-dir <path>`, `--io-keep` (reuse the scratch file between runs).

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

### Headline result — Apple M1 (with `sudo … --mac-power`)

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

### Headline result — disk, same M1 (`--workload io`, no privileges)

```text
disk: foreground baseline 1301.2 MiB/s, reads direct
executor              solo_mib/s  cont_mib/s    fg_mib/s  fg_prot%    p95_us
default                    988.5       996.6       998.7      76.8       394
utility                   1058.4        12.7      1267.9      97.4     22770
background                1003.0         7.9      1295.3      99.6     32870
background-threads        1021.6         8.0      1294.0      99.5     32872
verdict: background left the foreground ≥ (got out of the way) disk throughput than default did
```

Read `fg_prot%`: with a `Default`-class reader competing, the foreground app
keeps **76.8%** of its solo throughput — an even split, which is what fair
sharing looks like. With a `Background`-class reader it keeps **99.6%**: the
quiet work stepped aside, dropping from ~1000 MiB/s solo to ~8 MiB/s and taking
~33 ms per read instead of ~0.4 ms. That is the disk half of a `QosClass`,
measured, unprivileged.

Note `utility` throttles nearly as hard as `background` here — on macOS that is
Apple's mapping, not a `bgrt` choice, and it is a genuine surprise given how
close `Utility` is to `Default` on the CPU axis.

**[`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) has the full set** — three Linux
machines, with the analysis. The short version of what they show:

- **Homogeneous CPU, no contention: `nice(19)` is a null result.** All four
  executors are identical on a Threadripper. `nice` only deprioritizes when
  threads compete for a core, and with no E-cores there's no placement lever
  either.
- **`--clamp-frequency` is what bites there.** A `uclamp` cap gives the governor
  the frequency input `nice` doesn't: 2188 vs 3685 MHz on the Threadripper,
  840 vs 3192 MHz on a Sandy Bridge i7. It needs the `schedutil` governor (or
  `intel_pstate=passive`) and kernel ≥ 5.8 — under a fixed governor or HWP it's
  inert. Check with
  `cat /sys/devices/system/cpu/cpufreq/policy0/scaling_governor`. Like the
  library itself, it only ever *lowers*, so it needs no privileges.
- **Frequency clamping is a stay-cool lever, not a per-unit-work efficiency
  win.** On the i7, `Background` spends slightly *more* energy per work-unit
  (~88 vs ~74 µJ) — at low clocks, fixed and leakage power dominate. The payoff
  is lower *instantaneous* power and not stealing thermal budget from foreground
  work. macOS E-core *placement* is the opposite case: ~4× less energy per unit
  of work. Distinct levers, distinct economics.

> **macOS note (QoS promotion):** synchronously waiting on a background thread
> from a higher-QoS thread promotes it *off* the efficiency cores
> (priority-inversion avoidance), whereas an async `await` on a background
> runtime does not. The harness accounts for this in its `background-threads`
> runner. The practical takeaway: fire-and-forget background threads stay quiet,
> but if a foreground thread blocks waiting on one, macOS may speed it up.

### What's measurable per platform

Anything unavailable shows `n/a` / `null`, never an error:

| Signal | Linux | Windows | macOS |
|---|---|---|---|
| wall-clock, samples | ✅ | ✅ | ✅ |
| core placement / %E | ✅ sysfs | ✅ `GetSystemCpuSetInformation` | needs `powermetrics` |
| frequency | ✅ sysfs | ✅ `CallNtPowerInformation` | needs `powermetrics` |
| energy | ✅ RAPL (often root) | — | `--mac-power` (needs `sudo`) |
| disk throughput / `fg_prot%` | ✅ `O_DIRECT` † | ✅ `FILE_FLAG_NO_BUFFERING` | ✅ `F_NOCACHE` |

† Unprivileged everywhere, including macOS — the disk workload needs no
`powermetrics`. On Linux the *result* still depends on the I/O scheduler: `bfq`
honours priority fully, `mq-deadline` partially, and `none` (a common NVMe
default) not at all. The harness reads the active scheduler and says so, so a
null result there isn't mistaken for a broken class.

So on **Linux** and **Windows** you get placement and frequency unprivileged
(Linux energy may need root for RAPL; Windows has no energy counter at all); on
**macOS** core/frequency/power need `sudo powermetrics` (use `--mac-power`). The
library itself never needs privileges — only this measurement tool does.

## Limitations & notes

### What a class can and can't promise

- **Frequency isn't directly controllable** from userspace — `bgrt` *biases*
  against clocking up (chiefly by keeping work off performance cores); it can't
  *guarantee* the clock never rises, especially under other system load.
- **Classification is once-per-thread, by design.** QoS is applied when a runtime
  worker or thread starts; there's no per-task re-classification. Pick the right
  runtime/thread for the work. (This also sidesteps that, on Linux, an
  unprivileged thread can lower its priority but **cannot raise it back**.)
  Applying `Default` to an already-quiet thread — or building any runtime inside
  an already-niced process (`nice -n 10 …`, systemd `Nice=`) — therefore leaves
  Linux niceness where it is. `apply` reports success rather than surfacing an
  `EACCES` the caller could do nothing about; macOS and Windows do restore.
- **macOS join-promotion:** synchronously waiting on a background thread from a
  higher-QoS thread can promote it off the efficiency cores (see the
  benchmarking note above). Async `await` on a background runtime does not.
- **Telemetry availability varies** (see [the table
  above](#whats-measurable-per-platform)): Linux is fullest unprivileged; macOS
  frequency/power/residency need `sudo powermetrics`; Windows reports frequency,
  CPU index, and E/P classification, but has no energy counter. The *library*
  never needs privileges — only measurement does.

### Hybrid Linux is implemented but unmeasured

**`pin_efficiency_cores` has never been run on a heterogeneous (P+E) Linux
machine.** Every Linux measurement is from a homogeneous CPU — an
AMD Threadripper and an Intel Sandy Bridge i7 — neither of which has efficiency
cores or exposes the `cpu_capacity` sysfs entries the detection relies on. On
those machines the feature correctly does nothing, which is exactly the result
that cannot distinguish "works" from "silently broken".

Concretely, what is and isn't verified on Linux:

| Piece | Status |
|---|---|
| `nice` mapping per QoS class | ✅ run-verified (tests assert `nice 19`) |
| `uclamp` frequency cap | ✅ run-verified, two machines ([benchmarks](docs/BENCHMARKS.md)) |
| `topology::select_efficiency_cores` (capacity selection logic) | ✅ unit-tested, incl. hybrid and three-tier layouts |
| `topology::parse_cpulist` (hybrid-PMU CPU list) | ✅ unit-tested against published i9-12900K values + malformed input |
| Reading `cpu_atom`/`cpu_capacity` from sysfs on a real hybrid CPU | ❌ never executed — no such hardware available |
| `sched_setaffinity` pinning to detected E-cores | ❌ never executed against a non-empty core set |

**Detection uses two sources, because one doesn't cover x86.** `cpu_capacity` is
an arm64/riscv interface and appears not to exist on x86 at all — so until
2026-07-26 `pin_efficiency_cores` was, in all likelihood, a silent no-op on
*every* Intel hybrid CPU. There is now a fallback to the hybrid perf PMUs
(`/sys/bus/event_source/devices/cpu_atom/cpus`, unprivileged), which names the
E-cores directly. **AMD hybrid parts (Zen 4c / Zen 5c) are not detected**: their
dense cores share a PMU with the classic ones, and the kernel exposes the core
type only through root-only debugfs. Full reasoning in
[`docs/ROADMAP.md`](docs/ROADMAP.md).

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
  trimmed first. `bgrt` puts memory priority back immediately — trimmed pages get
  faulted back in, costing the very disk I/O this class is trying to avoid.
  Chromium does the same thing for the same reason. What goes back is whatever
  the thread had before the mode was entered, not a hard-coded "normal": a
  process can lower its own default memory priority and threads inherit that, so
  writing normal unconditionally would raise such a thread above the policy its
  process chose. `bgrt` only ever lowers.

### Classification does not follow threads your dependencies spawn

**A `QosClass` applies to the thread `bgrt` created — not to threads that code
running on it goes on to create.** If you hand a `Background` thread to a library
that spawns its own workers (RocksDB compaction and flush threads, an embedded
HTTP server, any pool with its own threads), whether those stay quiet depends
entirely on the OS:

| | Child threads inherit? | Why |
|---|---|---|
| Linux | ✅ yes | `nice`, I/O priority, affinity and `uclamp` live in `task_struct` and are copied by `clone()` |
| macOS | ❌ **no** | Darwin propagates QoS through dispatch queues and `pthread_attr_set_qos_class_np`, not plain `pthread_create` — a child reports `QOS_CLASS_DEFAULT` |
| Windows | ❌ **no** | *"All threads initially start at `THREAD_PRIORITY_NORMAL`"*; background mode and EcoQoS are per-thread |

All three rows are asserted by tests that run in CI, so this table is measured
rather than assumed.

**You usually cannot fix it after the fact.** macOS `pthread_set_qos_class_self_np`
and Windows `THREAD_MODE_BACKGROUND_BEGIN` are *current-thread only* — Windows
documents that the background flags "can be specified only if `hThread` is a
handle to the current thread". Even if you enumerated the library's threads, you
could not classify them. Only Linux lets you target another task by tid.

What does work, in order of preference:

1. **Give the library a thread hook.** Anything built on tokio or rayon is
   already covered — hand it a `bgrt` runtime or pool and every worker, including
   tokio's blocking pool, is classified at thread start. Some C libraries expose
   a thread-factory callback that can call `bgrt::apply`.
2. **Isolate it in its own process** and classify the process rather than a
   thread. The only approach that reliably catches threads you don't control.
   (On Windows use `SetPriorityClass(BELOW_NORMAL_PRIORITY_CLASS)` — *not*
   `PROCESS_MODE_BACKGROUND_BEGIN`, see the note above.)
3. **Measure before assuming it matters.** If the library's own threads do a
   small share of the work, classifying yours may still get most of the benefit —
   but verify, because the failure is silent.

## Scope

**CPU and block I/O are in scope, on all three platforms.** One `QosClass`
governs both — see [Disk I/O](#disk-io-what-each-platform-actually-does) for the
per-platform mechanisms and caveats. Split CPU/I/O control (quiet CPU but normal
I/O, or the reverse) is *not* offered, because two of three platforms can't
express it; if demand ever appears it can be added as an additive override.
Reasoning in
[`docs/DESIGN.md`](docs/DESIGN.md#scope-cpu-and-io-now-gpu-probably-never).

**GPU is probably never in scope.** Not on the roadmap, and it would take a shift
in the platforms to get there. In short: there is **no OS-level per-thread GPU
QoS** anywhere — the per-API equivalents (Vulkan, CUDA, D3D12; Metal has none)
are mutually incompatible and **arbitrate contention rather than save energy**, a
GPU idling at high clocks while your work waits can burn *more* energy for the
same result, D3D12 has **no tier below normal** to ask for, and GPU work belongs
to a queue rather than to a classifiable thread. For AI workloads the real levers
— picking a low-power compute unit, cutting batch size, quantizing — are
model-level decisions a thread-QoS crate can't make.

GPU energy matters; it's that *today* the lever isn't a scheduling hint, so it
doesn't belong in a scheduling-hint library. Most of those objections describe
what the platforms currently expose rather than a principle, so
[`docs/DESIGN.md`](docs/DESIGN.md#gpu--probably-never-but-heres-what-would-change-that)
spells out the specific developments that should reopen the question.

## Stability

The API is small, reviewed, and not expected to change — but the crate is
pre-1.0, and the release plan (0.9 public preview, then 1.0 after a 60-day
evaluation window) is in [`docs/ROADMAP.md`](docs/ROADMAP.md).

What is committed to now:

- **MSRV is 1.85.0** (edition 2024). An MSRV increase is a **minor** version
  bump, never a patch.
- **`QosClass` and `Error` are `#[non_exhaustive]`** — matching either from
  outside the crate needs a `_` arm, so a new class or error variant is a minor
  release rather than a major one.
- **The `telemetry` module is exempt from semver entirely.** It exists to serve
  `bgrt-bench` and may change or disappear in any release. Pin an exact version
  if you depend on it.
- **A Tokio or rayon major release is a `bgrt` major release.** `bgrt` wraps
  those runtimes rather than hiding them — `Runtime::spawn` returns Tokio's
  `JoinHandle`, `RayonPool` derefs to `rayon::ThreadPool` — so their types are
  part of the public API by design. Use the `bgrt::tokio` / `bgrt::rayon`
  re-exports to name the exact versions `bgrt` resolved.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -Dwarnings
cargo run --release -p bgrt-bench -- --duration 3
cargo deny check                                    # advisories + licences
```

Requires Rust ≥ 1.85 (edition 2024). See [`CLAUDE.md`](CLAUDE.md) for
architecture and conventions.

CI runs the full suite on Linux, macOS, and Windows — which is where the
per-OS backends actually execute, since each is `cfg`-gated to its own platform.
The two cross-compile checks in `CLAUDE.md` are the local pre-push substitute.

**Dependencies are permissive-licensed and advisory-checked.** `cargo deny`
runs in CI against the committed lock file, on every push and weekly: RustSec
advisories fail the build, and licences are checked against an allow-list of
permissive licences only. Copyleft — MPL, LGPL, GPL, AGPL, CDDL — is excluded by
omission, so a transitive dependency can't quietly impose source-disclosure
obligations on anything that links `bgrt`. The policy and the reasoning behind
each setting are in [`deny.toml`](deny.toml).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
