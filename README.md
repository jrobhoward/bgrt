# bgrt

[![CI](https://github.com/jrobhoward/bgrt/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/jrobhoward/bgrt/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/bgrt.svg)](https://crates.io/crates/bgrt)
[![docs.rs](https://img.shields.io/docsrs/bgrt)](https://docs.rs/bgrt)
[![MSRV](https://img.shields.io/badge/MSRV-1.85.0-blue.svg)](#stability)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**bgrt** ("background runtime") is a Rust library used to run async tasks or
threads at a reduced energy footprint (e.g. best-effort: favor efficiency cores & lower
clock speed, without spinning up fans).  Anti-starvation is a goal, so work
should continue making forward progress even when the machine is busy.

Features:

- **Per-thread energy QoS.** Work is classified (`Background`, `Utility`, or
  `Default`), with each classification mapping to OS capabilities: QoS classes
  on macOS, EcoQoS on Windows, `nice` plus `ioprio` on Linux (including optional
  efficiency-core affinity and a `uclamp` frequency cap).
- **CPU and I/O (disk).** One classification covers both, on all three platforms.
- **No admin rights.** The library only ever lowers a thread's priority, so
  it never needs to run with elevated privileges.
- **Wraps tokio, works with rayon.** A classified async runtime, plus a classified
  rayon pool and a plain thread spawner are available for code that isn't async.
- **Never starves.** Low-priority work gets a small share of resources rather than
  none, so it keeps crawling forward under load.
- **macOS, Windows, and Linux**, including big.LITTLE and P+E processors.

> Status: Currently feature-complete and tested on hardware I have available.
> It's pre-1.0, heading for a 0.9 preview. All three backends run in CI
> on their own OS. Current benchmark numbers come from an Apple M1
> (CPU and disk) and two homogeneous Linux machines (CPU). The hybrid-Linux and
> Windows paths are implemented and behaviour-tested, but nobody has measured
> their performance yet — see [Limitations](#limitations) and the
> [list of machines still wanted](docs/BENCHMARKS.md#data-points-still-wanted).
>
> Design: [`docs/DESIGN.md`](docs/DESIGN.md) · plan:
> [`docs/ROADMAP.md`](docs/ROADMAP.md) · results:
> [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) · state:
> [`CHANGELOG.md`](CHANGELOG.md).

## Usage

```rust
use bgrt::QosClass;

// A low-priority async executor (wraps a tokio runtime; feature "tokio", on by default).
let rt = bgrt::RuntimeBuilder::new()
    .qos(QosClass::Background)
    .worker_threads(1)
    .build()?;
rt.spawn(async { /* low-priority async work */ });

// Single-threaded task semantics: tokio's current-thread scheduler, driven on
// one OS thread that bgrt spawns and classifies (never the caller's thread).
let rt = bgrt::RuntimeBuilder::new().current_thread(true).build()?;

// A low-priority rayon thread pool (feature "rayon", opt-in).
let pool = bgrt::RayonBuilder::new()
    .qos(QosClass::Background)
    .build()?;
pool.install(|| data.par_iter().for_each(|x| process(x)));

// A low-priority OS thread (no features needed).
let jh = bgrt::spawn_thread(QosClass::Background, || { /* CPU-bound loop */ });

// Or classify the current thread directly.
bgrt::apply(QosClass::Utility)?;
```

To run some work normally and other work at low priority, use a
`Default`-class runtime in conjunction with a `Background`-class one in the same process, and
spawn onto whichever fits.

Runnable examples (`cargo run --example <name> -p bgrt`):

- [`background_task`](crates/bgrt/examples/background_task.rs) — a low-priority async task.
- [`mixed_runtimes`](crates/bgrt/examples/mixed_runtimes.rs) — foreground and background runtimes together.
- [`low_priority_threads`](crates/bgrt/examples/low_priority_threads.rs) — `spawn_thread` and `ThreadBuilder`.

## Feature flags

| Feature | Default | Adds |
|---------|---------|------|
| `tokio` | on | `RuntimeBuilder`, `Runtime` (async executor) |
| `rayon` | off | `RayonBuilder`, `RayonPool` (parallel iterators) |
| `telemetry` | off | measurement primitives used by `bgrt-bench` |

With `default-features = false` the dependency tree is just `QosClass`, `apply`,
`spawn_thread`, and `ThreadBuilder` — no tokio, no rayon.

## QoS classes

| Class | macOS | Windows | Linux |
|---|---|---|---|
| `Background` | `QOS_CLASS_BACKGROUND` (efficiency cores, throttled I/O) | background mode (throttled I/O) + EcoQoS + below-normal | `nice(19)` + I/O best-effort 7 + optional E-core affinity + optional `uclamp` frequency cap |
| `Utility` | `QOS_CLASS_UTILITY` | EcoQoS + normal | `nice(10)` + I/O best-effort 6 |
| `Default` | none | none | `nice(0)`, I/O untouched |

A QoS class covers CPU & block I/O together. It is implemented with one
knob instead of two because two of the three OS platforms bundle them together.
The `Background` I/O mapping is weighted-fair (Linux best-effort 7, not
`IOPRIO_CLASS_IDLE`), for the same anti-starvation reason `nice(19)` is used
instead of `SCHED_IDLE`.

## What it costs

Low-priority work is slower work. It's a tradeoff, and `bgrt-bench` measures it.

On an Apple M1, `Background` CPU work ran about 99% on efficiency cores in every
run, peaking below `Default`'s clock every time, and drew 3 to 13 times less CPU
power — 2 to 4 times less per unit of work — at 38% to 64% of the throughput.
The spread is the efficiency cluster's own clock, which the OS picks; the
placement is the part that held. Numbers in
[`docs/BENCHMARKS.md`](docs/BENCHMARKS.md).

On the disk side, a `Background` reader drops from ~1000 MiB/s to under 10 MiB/s when a foreground app is
reading too, and leaves that app 93% to 99% of its uncontended throughput; a
`Default`-class competitor leaves it 56% to 79%.

Two results are worth reading before picking a class:

- **`Background` can be very slow on macOS**, because the class is confined to
  the efficiency cores. `Utility` is the documented middle ground — LLVM and
  clangd hit this and switched. Note that `Utility` throttles disk nearly as hard
  as `Background` on macOS, so it is only a middle ground on the CPU side.
- **On homogeneous Linux, `nice(19)` alone does nothing** without contention. The
  optional `clamp_frequency` (`uclamp`) is the lever there — 840 MHz against
  3192 on a Sandy Bridge i7 — and it needs the `schedutil` governor to have any
  effect. Where cores share a clock, as on a Raspberry Pi, it holds the clock
  down only while nothing unclamped is busy on the other cores.

[`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) has every table, the commands to run
the harness on each OS, and the machines still missing from the set.

## Limitations

These are the caveats that change how the library should be used. The reasoning
and the measurements behind each are in [`docs/DESIGN.md`](docs/DESIGN.md).

- **Classification does not follow threads that dependencies spawn.** A class
  covers the thread `bgrt` created, not threads created later by code running on
  it. Child threads inherit on Linux only; macOS and Windows start them
  unclassified, and that cannot be fixed from outside, since both APIs only act
  on the calling thread. So handing a `Background` thread to a library that runs
  its own pool — RocksDB compaction, an embedded server — leaves that pool at
  full priority on two of the three platforms, with nothing to indicate it.
  Prefer libraries that accept a thread factory or run on a `bgrt` runtime or
  pool; failing that, put the work in its own process and classify the process.
- **Classification happens once per thread.** It is applied when a runtime worker
  or thread starts, and there is no per-task reclassification, so the executor is
  the thing to choose. This also sidesteps Linux's one-way `nice`: an
  unprivileged thread can lower its priority but cannot raise it again. Applying
  `Default` to a thread already lowered, or building a runtime inside a process
  that is already niced (`nice -n 10 …`, systemd `Nice=`), leaves Linux niceness
  where it is. macOS and Windows do restore it.
- **Some mappings do nothing on some configurations.** Linux `uclamp` needs the
  `schedutil` governor and kernel 5.8 or newer; stock Raspberry Pi images ship
  `ondemand`. Linux I/O priority needs `bfq`: `mq-deadline` ignores the
  best-effort level that `bgrt` sets, and `none` — a common NVMe default —
  ignores priority altogether. The builders log at debug level when a requested
  clamp cannot act, and the harness reports the active governor and scheduler so
  a flat result can be explained.
- **Hybrid-Linux E-core pinning is untested code rather than a measured
  feature.** `pin_efficiency_cores` has never run on real P+E silicon. Every
  Linux machine available has been homogeneous, where the feature correctly does
  nothing — which is the one result that cannot tell working code from silently
  broken code. AMD Zen 4c and 5c dense cores are not detected at all, because the
  kernel only exposes the core type through root-only debugfs. Windows has the
  same standing: behaviour-tested in CI, performance never measured.
- **macOS promotes a background thread that something waits on.** Blocking on
  such a thread from a higher-QoS thread pulls it off the efficiency cores, to
  avoid priority inversion. An async `await` on a background runtime does not.
  Fire-and-forget work stays low-priority; a foreground thread that blocks on
  one may speed it up.
- **Frequency is biased, not guaranteed.** The library states intent and lets the
  kernel place work and pick clocks. It never sets a frequency, and it cannot
  promise the clock stays down when the rest of the machine is busy.

## Scope

The scope of this library is CPU and block I/O, on all three platforms. One
`QosClass` governs both. Split
control — low-priority CPU with normal I/O, or the reverse — is not offered,
because two of the three platforms cannot express it. If it is ever needed it
can be added as an override without breaking anything; the reasoning is in
[`docs/DESIGN.md`](docs/DESIGN.md#why-one-knob-and-not-two).

GPU will probably never be in scope. No OS exposes a per-thread GPU QoS. The per-API
priorities that do exist arbitrate contention rather than save energy, and a GPU
idling at high clocks while deprioritized work waits can burn more energy for the
same result.

## Stability

The API is intended to be small and not expected to change, but the crate is pre-1.0. The
release plan — a 0.9 preview, then 1.0 after a 60-day evaluation window — is in
[`docs/ROADMAP.md`](docs/ROADMAP.md).

What is committed to now:

- **MSRV is 1.85.0** (edition 2024). Raising it is a minor version bump, never a
  patch.
- **`QosClass` and `Error` are `#[non_exhaustive]`.** Matching either from
  outside the crate needs a `_` arm, so adding a class or an error variant is a
  minor release rather than a major one.
- **The `telemetry` module is exempt from semver.** It exists to serve
  `bgrt-bench` and may change or disappear in any release. Pin an exact version
  to depend on it.
- **A tokio or rayon major release is a `bgrt` major release.** The crate wraps
  those runtimes rather than hiding them — `Runtime::spawn` returns tokio's
  `JoinHandle`, `RayonPool` derefs to `rayon::ThreadPool` — so their types are
  part of this API by design. The `bgrt::tokio` and `bgrt::rayon` re-exports name
  the exact versions `bgrt` resolved.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -Dwarnings
cargo deny check                                    # advisories + licences
cargo run --release -p bgrt-bench -- --duration 3
```

Requires Rust 1.85 or newer (edition 2024). [`CLAUDE.md`](CLAUDE.md) has the
module map and the conventions.

CI runs the suite on Linux, macOS, and Windows, which is where the per-OS
backends actually execute — each is `cfg`-gated to its own platform. A weekly job
re-resolves dependencies to catch upstream breakage that the committed lock file
would otherwise hide.

Dependencies are permissive-licensed and checked for advisories. `cargo deny`
runs in CI on every push and weekly: RustSec advisories fail the build, and
licences are checked against an allow-list of permissive licences, so copyleft —
MPL, LGPL, GPL, AGPL, CDDL — is excluded by omission. The policy and the
reasoning behind each setting are in [`deny.toml`](deny.toml).

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.
