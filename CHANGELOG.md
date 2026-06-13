# Changelog

All notable changes to **bgrt** are recorded here. This file tracks the
project's running state; the phased plan lives in [`docs/ROADMAP.md`](docs/ROADMAP.md).

Format loosely follows [Keep a Changelog](https://keepachangelog.com/);
the project is pre-1.0 and not yet released.

## [Unreleased]

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
