# Changelog

All notable changes to **bgrt** are recorded here. This file tracks the
project's running state; the phased plan lives in [`docs/ROADMAP.md`](docs/ROADMAP.md).

Format loosely follows [Keep a Changelog](https://keepachangelog.com/);
the project is pre-1.0 and not yet released.

## [Unreleased]

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
