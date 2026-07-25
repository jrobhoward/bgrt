# Changelog

All notable changes to **bgrt** are recorded here. This file tracks the
project's running state; the phased plan lives in [`docs/ROADMAP.md`](docs/ROADMAP.md).

Format loosely follows [Keep a Changelog](https://keepachangelog.com/);
the project is pre-1.0 and not yet released.

## [Unreleased]

### Road to 1.0 — API freeze decisions — 2026-07-25
- **Errors now preserve their cause.** `Error::Backend` became a struct variant
  `{ syscall: &'static str, source: std::io::Error }`; `Error::Runtime` carries
  tokio's `io::Error` directly; `Error::ThreadPool` boxes its cause as
  `Box<dyn Error + Send + Sync>` — deliberately *not* typed as
  `rayon::ThreadPoolBuildError`, so rayon's version is not part of `bgrt`'s
  public API and a rayon major release is not a breaking change here. Every
  construction site already had the structured cause (`io::Error::last_os_error`,
  `GetLastError`, pthread's returned errno) and was discarding it into a string.
  - **New `Error::raw_os_error() -> Option<i32>`**, so callers can distinguish
    "kernel lacks the feature" (`ENOSYS`) from a real failure without parsing
    messages — the case that actually matters for the best-effort `uclamp` and
    affinity paths. Display strings are now short and stable; the OS detail lives
    in the source chain.
- **`telemetry` is documented semver-exempt.** It exists to serve `bgrt-bench`;
  its surface may change in any release, including a patch. The rest of the crate
  carries the usual guarantees.
- **`Runtime::shutdown_timeout` / `shutdown_background`.** Dropping a runtime
  waits for blocking tasks indefinitely, which for a *background* runtime can be
  a very long time — quiet work is slow by design. These bound that wait.
- **No current-thread runtime — and it is now documented why.** Measured: a Tokio
  current-thread runtime fires `on_thread_start` only for blocking-pool threads,
  so async tasks run on the `block_on` caller's thread at its *unmodified* QoS
  (probe: task observed `QOS_CLASS_DEFAULT` while the blocking thread observed
  `QOS_CLASS_BACKGROUND`). Classifying the caller is not an option either: on
  Linux niceness is a one-way trip for unprivileged threads. `worker_threads(1)`
  is the single-quiet-worker configuration and costs exactly one thread (measured:
  `+1`; Tokio drives I/O and timers on the worker, with no extra driver thread).
- **Documented two standing decisions:** blocking-pool threads intentionally share
  the runtime's `QosClass` (CPU-bound work is exactly what lands there), and
  `QosClass::default()` is `Default` while the *builders* default to `Background`
  — the enum's default is its neutral member, whereas constructing a `bgrt`
  builder is already a request for quiet execution.
- **Verified:** `cargo test --workspace`, `--all-features` (44 lib + 9 doctests),
  `--no-default-features`, clippy `-Dwarnings` on host plus the Linux and Windows
  cross-targets, MSRV 1.85.0 `cargo check --all-features`, and `cargo doc` with
  `RUSTDOCFLAGS=-Dwarnings` — all clean.

### CI, licensing, and packaging — 2026-07-25
- **`.github/workflows/ci.yml`** — first CI for the project. `test` job matrixed
  over macOS/Linux/Windows (clippy `-Dwarnings`, `cargo test --workspace`, four
  feature permutations each), `msrv` job matrixed on 1.85.0, and a `lint` job
  (`cargo fmt --check`, `cargo doc` with `RUSTDOCFLAGS=-Dwarnings`). This is what
  finally *executes* the Windows and Linux backends, which until now were only
  ever cross-compiled. Uses rustup plus first-party actions only; `bash` forced
  on all three runners. Clippy's `-Dwarnings` is passed after `--` so it applies
  to workspace members and not dependencies, keeping upstream warnings from
  breaking the build.
- **`LICENSE-MIT` + `LICENSE-APACHE`** added, matching the long-declared
  `MIT OR Apache-2.0`. `cargo package --list` showed workspace-root licenses were
  **not** included in the publishable tarball; both are symlinked into
  `crates/bgrt/` and verified present with dereferenced content.
- **Packaging:** `[package.metadata.docs.rs] all-features = true` (without it the
  `rayon` and `telemetry` APIs would be absent from docs.rs entirely) and a
  `readme` field.
- **Windows test gap closed:** added a `Background` → `Default` transition test
  covering the EcoQoS *clear* path (`set_eco_qos(false)`); the pre-existing tests
  each start on a fresh thread and so only ever *set* it. EcoQoS state has no
  documented read-back, so the throttling bit remains asserted indirectly through
  thread priority.

### Opt-in Linux `uclamp` frequency clamp — 2026-06-13
- **New `clamp_frequency(bool)` builder option** on `RuntimeBuilder`,
  `RayonBuilder`, and `ThreadBuilder` (opt-in, default off). On Linux it caps a
  `Background` thread's `util_max` via `sched_setattr`
  (`SCHED_FLAG_KEEP_ALL | SCHED_FLAG_UTIL_CLAMP_MAX`, ~20% of
  `SCHED_CAPACITY_SCALE`), biasing the cpufreq governor toward a lower clock —
  the only lever that lowers frequency on homogeneous CPUs, where `nice` has no
  frequency effect. `Utility`/`Default` are left unclamped.
- **`backend/uclamp.rs`:** declares `struct sched_attr` (no `libc` wrapper) and
  calls `sched_setattr` through `libc::syscall`. Best-effort: ENOSYS/EINVAL/E2BIG/
  EPERM/EOPNOTSUPP (pre-uclamp or pre-5.8 kernels, sandboxes) degrade to a no-op.
  Only ever *lowers* `util_max`, so it stays unprivileged. No-op off Linux.
- **Caveat (documented):** effect requires the `schedutil` governor (or
  `intel_pstate=passive`); fixed governors and HWP bypass the util signal.
- **`bgrt-bench --clamp-frequency`:** new flag that routes the clamp through the
  runtime/thread runners (warns it's a no-op off Linux). README gains a
  "Running on each platform" command cheat-sheet (privilege-free throughput
  everywhere; `--mac-power` + sudo on macOS; `--pin` for hybrid, `--clamp-frequency`
  for homogeneous, sudo for RAPL on Linux) and a homogeneous-CPU clamp note.
- **Run-verified on Intel i7-2720QM (Sandy Bridge, homogeneous) with `schedutil`
  + `--clamp-frequency`:** background mean clock 840 MHz vs 3192 (~3.8× lower) and
  ~3.2× less package energy over a fixed 10 s window, at ~26% throughput — the
  first measured frequency effect on a homogeneous CPU (where `nice` shows
  nothing). Results table added to the README. Finding worth keeping: clamping is
  a *stay-cool / low-power-draw* lever, not a per-unit-work efficiency win — on this
  old silicon background spends slightly *more* energy per work-unit (~88 vs ~74 µJ)
  because fixed/leakage power dominates at low clocks (race-to-idle). Documented in
  the README and `docs/DESIGN.md`.
- **Tests:** `backend/uclamp_tests.rs` covers the per-class cap mapping and reads
  back `uclamp.max` from `/proc/thread-self/sched` (new `current_uclamp_max()`
  helper), tolerating kernels without `CONFIG_UCLAMP_TASK` / `SCHED_FLAG_KEEP_ALL`.
- **Verified:** `cargo clippy -p bgrt --all-features --tests` clean on host
  (macOS) and on the `x86_64-unknown-linux-gnu` / `x86_64-pc-windows-msvc`
  targets; `cargo test -p bgrt --all-features` passes on macOS (the Linux-only
  uclamp tests compile under the Linux target but execute on Linux/CI).

### Optional features, rayon integration, Linux run — 2026-06-13
- **Optional `tokio` feature (default on):** `RuntimeBuilder`/`Runtime` now live
  behind `features = ["tokio"]` (enabled by default). Users who only need
  `spawn_thread`/`ThreadBuilder`/`apply` can opt out with `default-features = false`
  for a lean dep tree with no tokio. `bgrt-bench` now declares
  `features = ["telemetry", "tokio"]` explicitly.
- **New `rayon` feature (opt-in, default off):** `RayonBuilder`/`RayonPool` wrap
  a `rayon::ThreadPool` with the configured `QosClass` applied to every thread at
  start, mirroring the `RuntimeBuilder`/`Runtime` pattern. `pool.install(|| …)`
  routes rayon `par_iter`/`join`/`scope` work through the quiet threads. `RayonPool`
  derefs to `rayon::ThreadPool` for full API access; `pool.qos()` returns the
  configured class.
- **Error variants gated by feature:** `Error::Runtime` behind
  `#[cfg(feature = "tokio")]`; new `Error::ThreadPool` behind
  `#[cfg(feature = "rayon")]`.
- **Clippy fix (`power.rs`):** `PowerStats::parse` and its private helpers
  (`Cluster`, `Acc`, `freq_acc`, `cluster_kind`, `metric_after`, `leading_number`)
  gated `#[cfg(any(target_os = "macos", test))]` — they are only called by the
  macOS-only `power_macos` sampler, but the unit tests still exercise them on all
  platforms. Fixes `cargo clippy --all-targets -- -Dwarnings` on Linux.
- **Linux run-verified on AMD Threadripper (16-core, homogeneous, no E-cores):**
  all tests pass; benchmark shows flat throughput and frequency across executors —
  expected, since `nice(19)` only deprioritizes under CPU contention and there are
  no E-cores for affinity pinning. RAPL energy is available under `sudo` but
  variance (<3%) is measurement noise across whole-package readings on an otherwise-
  idle 16-core machine, not a per-thread signal. Meaningful Linux results require
  either a heterogeneous (P+E) CPU or a CPU-loaded machine.
- **Verified:** `cargo test --workspace`, `cargo test -p bgrt --no-default-features`,
  `cargo test -p bgrt --features rayon`, and `cargo clippy --workspace --all-targets
  -- -Dwarnings` all clean on Linux (Threadripper).

### Review pass — refactor, tests, docs — 2026-06-13
- **Renamed** `bgrt::Builder` → `bgrt::RuntimeBuilder` (symmetry with
  `ThreadBuilder`; clearer at the crate root). Updated lib, tests, bench,
  examples, README.
- **Testability:** extracted the Linux E-core selection into a pure
  `topology::select_efficiency_cores`, with unit tests (hybrid / homogeneous /
  empty / three-tier) that run on any platform.
- **De-duplicated test helpers** into a `#[cfg(test)] test_support` module
  (`current_qos` / `current_nice` / QoS constants), removing three copies of the
  read-back FFI across the backend/runtime/thread test files.
- **Rustdoc examples (doctests):** added to `RuntimeBuilder`, `spawn_thread`, and
  a crate-level `# Example`; now 4 doctests run as part of the suite.
- **Docs:** new [`docs/DESIGN.md`](docs/DESIGN.md) distilling the durable design
  (mechanism, per-OS mapping, anti-starvation, two-runtime pattern, telemetry
  matrix, findings, non-goals); README gained a "Limitations & notes" section and
  links to the design doc; CLAUDE.md cross-links it.
- **Verified:** `cargo test --workspace` (33 lib + 16 bench + 1 integration + 4
  doctests), `clippy --workspace --all-targets -- -Dwarnings` (macOS) and
  `--tests` cross-target (Linux, Windows), examples run, `cargo doc` clean.

### Phase 6 — Docs, examples, polish — 2026-06-13
- Added runnable examples: `background_task`, `mixed_runtimes`, `quiet_threads`
  (`cargo run --example <name> -p bgrt`); all run and pass
  `clippy --all-targets -- -Dwarnings`.
- README: "early development" → real status; "Intended usage" → "Usage" with an
  examples list; folded in the **Apple M1** measured results (Background ran 99.8%
  on E-cores at ~1029 MHz and drew ~12× less CPU power than Default) plus a
  system-wide-telemetry caveat.
- CLAUDE.md: refreshed architecture (all modules: runtime/thread/topology/
  telemetry + bench workload/runner/report/power) and commands (telemetry tests,
  cross-target clippy, examples, sudo `--mac-power`).
- **Verified:** `cargo clippy --workspace --all-targets -- -Dwarnings`, examples
  run, `cargo test --workspace`, `cargo doc` all clean.

### Phase 5.1 — Throughput, powermetrics, macOS finding — 2026-06-13
- **Throughput metric:** the workload now counts work units completed and reports
  `work` + `work/s`. This makes the energy/perf tradeoff visible **unprivileged on
  macOS** (where placement/freq need powermetrics): e.g. a Background runtime
  measured ~2050 work/s vs ~5165 for Default — the E-core confinement, quantified.
- **macOS `powermetrics` telemetry:** `--mac-power` now samples `powermetrics`
  *per executor run* (cluster freq, E/P residency, CPU power) via a `power_macos`
  `Sampler`; parsing is a pure, cross-platform, unit-tested `power::PowerStats`.
  When present it fills the `%E` / frequency / energy columns (and a verdict)
  that are otherwise `n/a` on macOS. Requires running under `sudo`.
- **Finding (macOS QoS override):** a higher-QoS thread that synchronously
  `join`s a background thread **promotes it off the efficiency cores**
  (priority-inversion avoidance); tokio's `await` does not. The harness's
  `background-threads` runner now matches the waiting thread's QoS to the workers
  on macOS so the measurement reflects the executor, not the join — confirmed by
  the throughput dropping from ~5175 to ~2180 work/s. Documented in the README as
  a real macOS behavior users should know.
- Removed the old one-shot `--mac-power` end-of-run reading (superseded by
  per-run sampling).
- **Verified:** macOS — `cargo test --workspace` (29 lib + 16 bench + doctest +
  integration), harness shows the throughput gap; clippy `-Dwarnings` all three
  targets.

### Phase 5 — Comparison harness — 2026-06-13
- `bgrt-bench` now compares executors end to end:
  - `workload` — CPU-bound loop that self-samples telemetry (placement attributed
    to the worker actually running it).
  - `runner` — runs the workload on Default / Utility / Background runtimes and on
    Background OS threads → `RunResult` (wall-clock, `Aggregate`, energy).
  - `report` — aligned text table, pretty JSON (`serde`), and the
    `background_not_hotter` verdict (background peak freq ≤ default).
  - CLI (`clap`): `--duration`, `--workers`, `--interval`, `--executors`,
    `--format table|json`, `--pin` (Linux E-core affinity), `--mac-power`.
- macOS `powermetrics` reader as a defensive `--mac-power` opt-in (parser unit-
  tested; graceful no-sudo → "unavailable" verified). Windows E/P classification
  still deferred.
- `tests/comparison.rs` integration test runs the built binary, parses its JSON,
  and asserts background ≤ default peak frequency — tolerant (skips where
  frequency telemetry is unavailable, e.g. macOS).
- `clippy.toml`: added `allow-expect-in-tests = true` (the integration test uses
  `expect`; `expect_used` needs its own opt-out alongside `allow-unwrap-in-tests`).
- **Verified:** macOS — `cargo test --workspace` (29 lib + 12 bench + 1 doctest +
  1 integration), harness runs (table/JSON/verdict; honest macOS `n/a`); clippy
  `-Dwarnings` + `cargo doc` clean. Linux & Windows — clippy `-Dwarnings` clean
  cross-target.

### Phase 4 — Telemetry primitives — 2026-06-13
- New `telemetry` module behind an off-by-default `telemetry` feature (no extra
  deps; uses the platform `libc`/`windows-sys` already present). Every signal
  degrades gracefully to `None`/`Unknown` — never errors or panics.
  - `sample()` → `Sample { cpu, core_type, freq_mhz }`: current CPU
    (Linux `sched_getcpu` / Windows `GetCurrentProcessorNumber` / macOS none),
    E/P classification (Linux via `topology`), frequency (Linux sysfs `cpufreq`
    / Windows `CallNtPowerInformation`).
  - `energy_uj()` + `EnergyMeter` — Linux RAPL package energy (when readable),
    else `None`.
  - `Aggregate` — folds samples into residency (% E vs P), distinct CPUs, and
    mean/max frequency.
- `bgrt-bench` enables the feature and prints a telemetry smoke probe.
- **Scope (vs. plan):** threaded Sampler orchestration, Windows E/P
  classification, and the macOS `powermetrics` (root) path all move to Phase 5.
- **Verified:** macOS — `cargo test --workspace` (29 tests incl. 10 telemetry;
  pure aggregation/classification/energy-delta logic fully covered), clippy
  `-Dwarnings` (default *and* `--features telemetry`), `cargo doc`, bench runs
  (shows honest macOS degradation). Linux & Windows — clippy `-Dwarnings` clean
  cross-target with `--features telemetry`.

### Phase 3 — Quiet thread spawn API — 2026-06-13
- `spawn_thread(class, f)` — classified OS thread, infallible like
  `std::thread::spawn` (no pinning; panics on OS failure, as std does).
- `ThreadBuilder` (qos / name / stack_size / pin_efficiency_cores) with
  `spawn(f) -> io::Result<JoinHandle<T>>`, mirroring `std::thread::Builder` — the
  non-panicking path; supports opt-in Linux E-core pinning. Both apply QoS at the
  top of the thread body via a shared best-effort `classify` helper.
- `spawn_blocking` classification was already covered by Phase 2's runtime hook;
  this phase adds the standalone `std::thread` path.
- **Verified:** macOS — `cargo test -p bgrt` (19 unit + 1 doctest), incl. QoS
  read-back on `spawn_thread` and `ThreadBuilder` threads; clippy `-Dwarnings`;
  `cargo doc`. Linux & Windows — clippy `-Dwarnings` clean cross-target;
  nice-19 thread test runs on CI / Linux hardware.

### Phase 2 — Runtime (tokio wrapper) — 2026-06-13
- `Builder` → `Runtime`: wraps a multi-thread tokio runtime, applying the chosen
  `QosClass` to every runtime thread via `on_thread_start` (best-effort: warns on
  failure, never aborts). `Builder` knobs: `qos` (default `Background`),
  `worker_threads` (default 1, clamps 0→1 so tokio can't panic), `thread_name`,
  `pin_efficiency_cores` (opt-in). `Runtime`: `spawn`, `spawn_blocking`,
  `block_on`, `handle`, `qos`.
- New `topology` module: Linux E-core detection via sysfs `cpu_capacity`
  (empty when unavailable/homogeneous) + `sched_setaffinity` pinning of the
  current thread; no-op on macOS/Windows (their QoS/EcoQoS places work).
- Added `Error::Runtime`. tokio added as a `bgrt` dependency (rt-multi-thread, time).
- **Resolved open question:** tokio's `on_thread_start` *does* cover the blocking
  pool — a macOS test confirms `spawn_blocking` work is classified. No workaround.
- **Verified:** macOS — `cargo test -p bgrt` (15 unit + 1 doctest), incl. worker-
  and blocking-pool QoS read-back; clippy `-Dwarnings`; `cargo doc`. Linux &
  Windows — clippy `-Dwarnings` clean cross-target; their runtime read-back tests
  (`getpriority`, etc.) run on CI / native hardware.

### Review & hardening — 2026-06-13
- Confirmed production code is panic-free (no `unwrap`/`expect`/`panic!`/indexing;
  every FFI return code is checked into `Result`). `unwrap` remains test-only.
- Enabled `#![warn(missing_docs)]` on the `bgrt` crate (passes under `-Dwarnings`).
- Refreshed stale docs (crate-level + `backend` module no longer say "Phase 0 /
  no-op"; `QosClass` table marks Linux E-core affinity as opt-in / not-yet-wired).
- Added a runnable doctest on `apply` and an `Error` Display test (`error_tests.rs`).
- Verified across all three targets (macOS run; Linux/Windows cross-check): tests
  (8 unit + 1 doctest on macOS), clippy `-Dwarnings`, and `cargo doc` (broken-link
  check) all clean.

### Phase 1 — QoS backends — 2026-06-13
- `apply(QosClass)` now does real work per OS (was no-op):
  - **macOS** — `pthread_set_qos_class_self_np`: Background→`QOS_CLASS_BACKGROUND`
    (0x09), Utility→`QOS_CLASS_UTILITY` (0x11), Default→`QOS_CLASS_DEFAULT` (0x15).
  - **Linux** — `setpriority`: nice 19 / 10 / 0 (weighted-fair, not `SCHED_IDLE`).
  - **Windows** — EcoQoS via `SetThreadInformation` + `SetThreadPriority`
    (below-normal / normal); Default clears EcoQoS.
- Read-back tests per OS (macOS `pthread_get_qos_class_np`, Linux `getpriority`,
  Windows `GetThreadPriority`), each on a dedicated thread.
- **Scope change:** efficiency-core affinity deferred to Phase 2 (with the
  opt-in `pin_efficiency_cores` builder option + `topology` module); `apply`
  stays the always-unprivileged nice/QoS/priority part.
- **Verified:** macOS — `cargo test -p bgrt` (7 tests) + clippy `-Dwarnings`
  clean. Linux & Windows — `cargo check --tests` + clippy `-Dwarnings` clean
  cross-target (`x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`); runtime
  behavior on those OSes still pending CI / real hardware.

### Phase 0 — Workspace scaffold — 2026-06-13
- Cargo workspace (edition 2024, `rust-version = 1.85.0`, resolver 3): `bgrt`
  library + `bgrt-bench` binary; `clippy.toml` (`allow-unwrap-in-tests`),
  workspace clippy lints, release profile (strip/lto/codegen-units=1).
- `bgrt`: `QosClass { Background, Utility, Default }`; `error::Error` (thiserror);
  `apply(QosClass)` dispatching to `cfg`-gated per-OS `backend/` modules
  (macOS/Linux/Windows + a no-op fallback) — all **no-ops** this phase, FFI lands
  in Phase 1. `bgrt-bench`: placeholder `main`.
- Conventions wired: separate `*_tests.rs` (registered via `#[path]`), the
  `subject____condition____result` naming with `#![allow(non_snake_case)]` per
  test file, `tracing`. Decision: affinity is **opt-in**.
- Docs: `CLAUDE.md`, `README.md`.
- **Verified (macOS):** `cargo build --workspace`, `cargo clippy --workspace
  --tests -- -Dwarnings`, and `cargo test --workspace` (4 tests) all clean;
  `bgrt-bench` runs. Linux/Windows not buildable on this host (cfg-gated no-ops).

### Planning — 2026-06-13
- Project named **bgrt** ("background runtime").
- Decisions locked in: Cargo **workspace** (`bgrt` lib + `bgrt-bench` harness
  bin); **all three** OS backends (macOS / Windows / Linux) from the start;
  schedule both **async tasks and low-priority threads**; comparison harness
  measures **wall-clock time + core placement + CPU frequency + power**.
- Approach: **wrap tokio** via `Builder::on_thread_start` (no fork); express
  per-thread energy QoS (`Background` / `Utility` / `Default`); unprivileged;
  weighted-fair low priority so quiet work never starves.
- Phased plan written to `docs/ROADMAP.md` (Phases 0–6). No code yet.
