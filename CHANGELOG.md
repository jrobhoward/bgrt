# Changelog

Notable changes to **bgrt**, in the format of
[Keep a Changelog](https://keepachangelog.com/).

Nothing has been released yet, so everything below is the content of the first
release. Development ran from 2026-06-13 to 2026-07-26; the per-change reasoning
lives in [`docs/DESIGN.md`](docs/DESIGN.md) and the day-by-day sequence in the git
history, so this file records *what* shipped rather than how it got there.

## [Unreleased] — 0.9.0 candidate

### Added

- **`QosClass` (`Background` / `Utility` / `Default`) and `apply()`** — per-thread
  energy classification, unprivileged, on macOS, Windows, and Linux, with a no-op
  fallback elsewhere. Per-OS mapping in [the README](README.md#qos-classes).
- **A class covers block I/O as well as CPU**, on all three platforms: Linux
  `ioprio_set` (best-effort 7/6, never `IOPRIO_CLASS_IDLE`), Windows
  `THREAD_MODE_BACKGROUND_BEGIN` with memory priority restored afterwards, macOS
  free via the QoS class itself. One knob, not two — two of three platforms
  cannot express a CPU/I/O split.
- **`RuntimeBuilder` / `Runtime`** (feature `tokio`, on by default) — wraps a
  multi-thread tokio runtime and classifies **every** runtime thread, including
  the blocking pool, via `on_thread_start`. `spawn`, `spawn_blocking`, `block_on`,
  `handle`, `qos`, `shutdown_timeout`, `shutdown_background`.
- **`RuntimeBuilder::current_thread(bool)`** — single-threaded task semantics
  driven on one OS thread that `bgrt` spawns and classifies, never the caller's
  (tokio's own hook doesn't fire for that thread, and reclassifying a foreign
  thread is unsound on Linux).
- **`RayonBuilder` / `RayonPool`** (feature `rayon`, opt-in) — a rayon thread pool
  whose workers are classified at start; derefs to `rayon::ThreadPool`.
- **`spawn_thread` and `ThreadBuilder`** — the non-async path, available with no
  feature flags.
- **Two opt-in Linux knobs**, both off by default and no-ops elsewhere:
  `pin_efficiency_cores` (E-core affinity) and `clamp_frequency` (a `uclamp`
  `util_max` cap — the only frequency lever on homogeneous CPUs, where `nice`
  has no effect).
- **Efficiency-core detection** — Linux sysfs `cpu_capacity` plus the hybrid
  `cpu_atom` PMU; Windows `GetSystemCpuSetInformation`. Both platforms share one
  pure selector, so an all-equal machine reports homogeneous rather than "every
  core is an E-core".
- **`telemetry` module** (opt-in, and exempt from semver) — CPU/core-type/
  frequency sampling, Linux RAPL energy, aggregation. Degrades to `None`, never
  errors.
- **`bgrt-bench`** — the comparison harness. `--workload cpu` measures throughput
  per class; `--workload io` measures disk behaviour with cache-bypassing random
  reads, run solo and against a foreground competitor. Table or JSON output, with
  verdicts. Results in [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md).
- **`pub use tokio;` / `pub use rayon;`** (each behind its feature) so callers can
  name the exact versions `bgrt` resolved.

### Changed

- **I/O folded into `QosClass` rather than shipped as a separate `io_class()`
  knob.** Reversed a decision to defer it to 1.1: redefining the class after 1.0
  would have been a silent behaviour change, which is worse than a semver break.
- **`bgrt::Builder` renamed to `bgrt::RuntimeBuilder`**, for symmetry with
  `ThreadBuilder`.
- **Errors carry their cause.** `Error::Backend` is now `{ syscall, source }`,
  plus `Error::raw_os_error()` so callers can tell "kernel lacks the feature"
  (`ENOSYS`) from a real failure without parsing strings.
- **`#[must_use]` on all 19 chainable builder methods** — `.qos(…)` alone used to
  compile, do nothing, and warn about nothing.
- **`QosClass` and `Error` are `#[non_exhaustive]`**; MSRV increases are minor
  bumps; a Tokio or rayon major is a `bgrt` major. See
  [Stability](README.md#stability).
- **The rayon pool calls the shared `thread::classify`** instead of reimplementing
  the qos → pin → clamp sequence, so a future knob can't reach two builders and
  silently skip the third.

### Fixed

- **Efficiency-core detection was a silent no-op on every Intel hybrid CPU.**
  `cpu_capacity` is an arm64 interface that appears not to exist on x86 at all;
  detection now falls back to the `cpu_atom` PMU, which names the E-cores
  directly. The old behaviour was indistinguishable from the correct no-op on a
  homogeneous machine — the failure mode this project treats as the dangerous one.
- **Linux `apply(Default)` failed inside an already-niced process.** `setpriority`
  returns `EACCES` when asked to raise priority, so a `Default`-class runtime
  errored on every worker under `nice -n 10 …` or systemd `Nice=`. Now a graceful
  no-op, matching how `ioprio` and `uclamp` already handled a declined hint.
- **Windows restored memory priority to a hard-coded normal** after leaving
  background mode, which could *raise* a thread above the policy its process
  chose. It now samples the thread's actual baseline first. Found by CI: GitHub's
  `windows-latest` runners start threads at `MEMORY_PRIORITY_LOW`.
- **Windows telemetry sized its buffer from `available_parallelism()`**, which
  counts processors available to the *process*; any process under a restricted
  affinity mask silently reported no frequency at all. Now uses
  `GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)`.
- **The disk workload could record zero reads** when one slow read straddled a
  short measurement window, reporting an executor as broken rather than slow.
  Every worker now counts at least one read.
- **macOS: `setiopolicy_np` is never called.** Setting an explicit I/O policy
  permanently opts the thread out of QoS — measured — costing E-core confinement
  and the headline power result. Implementation reverted; a regression test guards
  it.

### Security

- **`crossbeam-epoch` 0.9.18 → 0.9.20** — RUSTSEC-2026-0204, an invalid pointer
  dereference in the `fmt::Pointer` impl for `Atomic`/`Shared`, reached
  transitively through `rayon`. Found by the first `cargo deny` run.

### Infrastructure

- **CI** — a 3-OS matrix (clippy, full suite, four feature permutations), an MSRV
  1.85.0 job, a fmt/docs job, a weekly `fresh-deps` job that re-resolves
  dependencies to catch upstream breakage the committed lock file hides, and
  `cargo deny` for RustSec advisories plus a permissive-only licence allow-list.
  CI is where the Linux and Windows backends actually execute.
- **Licensing and packaging** — `LICENSE-MIT` and `LICENSE-APACHE` symlinked into
  the publishable crate (workspace-root licences were not being included),
  `docs.rs` configured for `--all-features`, and `doc(cfg)` feature badges.

### Documentation

- The QoS mapping table had five copies and had drifted in three; the README's is
  now canonical and everything else links to it.
- `README.md` streamlined to what the crate does and how to use it;
  `docs/BENCHMARKS.md` became the measurement hub (results, per-OS commands, and
  the machines we still need); `docs/ROADMAP.md` reduced to the 0.9 → 1.0 plan;
  `docs/DESIGN.md` holds the durable rationale, the findings, and UML diagrams of
  the builder trio and the classification sequence.
- Caveats are documented rather than implied away: hybrid-Linux pinning and the
  Windows performance story are labelled untested; `uclamp` and Linux I/O priority
  are inert under some governors and I/O schedulers; classification does not
  follow threads your dependencies spawn, except on Linux.
