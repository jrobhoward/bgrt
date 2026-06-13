# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`bgrt` ("background runtime") runs async tasks and spawned threads at a low
energy footprint — efficiency cores, low clock frequency, no fan spin-up — while
still making forward progress under load, as a regular (non-admin) user on
macOS, Windows, and Linux. It **wraps** tokio rather than forking it.

See `docs/DESIGN.md` for the durable design and rationale, `docs/ROADMAP.md` for
the phased plan and current status, and `CHANGELOG.md` for running project state.

## Commands

```bash
# Build
cargo build --workspace
cargo build --workspace --release

# Test (telemetry tests run when the feature is on, e.g. via the bench)
cargo test --workspace
cargo test -p bgrt                                  # single crate
cargo test -p bgrt --features telemetry             # incl. telemetry module
cargo test -p bgrt -- some____test____name          # single test

# Lint (must be clean before any phase is considered done)
cargo clippy --workspace --tests -- -Dwarnings
cargo clippy --workspace --all-targets -- -Dwarnings   # also lints examples

# Cross-compile checks for the other OS backends (no linker needed for `check`)
cargo clippy -p bgrt --tests --target x86_64-unknown-linux-gnu -- -Dwarnings
cargo clippy -p bgrt --tests --target x86_64-pc-windows-msvc -- -Dwarnings

# Examples
cargo run --example background_task -p bgrt
cargo run --example mixed_runtimes -p bgrt
cargo run --example quiet_threads -p bgrt

# Comparison harness (sudo + --mac-power on macOS for %E/freq/power)
cargo run --release -p bgrt-bench -- --duration 3
sudo ./target/release/bgrt-bench --duration 3 --mac-power
```

## Architecture

Cargo workspace, edition 2024, `rust-version = 1.85.0`.

- **`bgrt`** — the library.
  - `qos` — `QosClass { Background, Utility, Default }`, the energy class applied per thread.
  - `backend/` — per-OS dispatch (`macos.rs`, `linux.rs`, `windows.rs`), each exposing `apply(QosClass)` acting on the *current* thread. macOS = `pthread_set_qos_class_self_np`; Linux = `setpriority`; Windows = EcoQoS via `SetThreadInformation` + `SetThreadPriority`. A no-op fallback covers other platforms.
  - `runtime` — `RuntimeBuilder` → `Runtime` wrapping a multi-thread tokio runtime; applies `QosClass` to every runtime thread (workers + blocking pool) via `on_thread_start`. `spawn` / `spawn_blocking` / `block_on` / `handle` / `qos`.
  - `thread` — `spawn_thread` (infallible, like `std::thread::spawn`) and `ThreadBuilder` (`io::Result`, like `std::thread::Builder`); applies QoS at the top of the thread body.
  - `topology` — E-core detection (Linux sysfs `cpu_capacity`) + `sched_setaffinity` pinning; no-op off Linux. `pin_efficiency_cores` is opt-in.
  - `telemetry` *(feature `telemetry`, off by default)* — measurement primitives: `sample()` (cpu/core-type/freq), `energy_uj()`/`EnergyMeter`, `Aggregate`. Graceful `None`/`Unknown` where unavailable.
  - `error` — `thiserror` `Error` (`Backend`, `Runtime`).
  - `examples/` — `background_task`, `mixed_runtimes`, `quiet_threads`.
- **`bgrt-bench`** — the comparison harness binary (enables `bgrt/telemetry`).
  - `workload` — CPU-bound, self-sampling loop; returns work units (throughput).
  - `runner` — `Executor` (Default/Utility/Background/BackgroundThreads) → `RunResult` (wall, work, aggregate, energy, powermetrics). On macOS the threads runner matches the waiter's QoS during `join` (avoids the kernel promoting background threads off E-cores).
  - `report` — `Summary`, aligned table, JSON, `background_not_hotter` verdict.
  - `power` — pure, cross-platform `PowerStats` parser for `powermetrics` output.
  - `power_macos` *(macOS only)* — `Sampler` that runs `powermetrics` per executor run (needs sudo).

## QoS mapping

| Class | macOS | Windows | Linux |
|---|---|---|---|
| `Background` | `QOS_CLASS_BACKGROUND` (E-core-confined) | EcoQoS + `BELOW_NORMAL` | `nice(19)` + opt-in E-core affinity |
| `Utility` | `QOS_CLASS_UTILITY` | EcoQoS + `NORMAL` | `nice(10)` |
| `Default` | passthrough | clear throttling | `nice(0)` |

Every `Background` mapping is weighted-fair (not run-only-when-idle) so quiet
work never starves. The **library is always unprivileged**; only the measurement
harness may need elevation (macOS `powermetrics`, Linux RAPL) for power/placement
readings.

## Conventions

**Test file layout:** tests live in separate `*_tests.rs` files, registered at
the bottom of the source file with:
```rust
#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;
```
Integration tests live in `crates/<crate>/tests/`. The `bgrt-bench` integration test (`crates/bgrt-bench/tests/comparison.rs`) runs the harness binary end-to-end via `CARGO_BIN_EXE_bgrt-bench` and asserts background peak MHz ≤ default.

**Test naming:** `subject____condition____result` — exactly four underscores
between segments. Because consecutive underscores trip `non_snake_case`, every
`*_tests.rs` file carries `#![allow(non_snake_case)]` at the top.

**Test helpers:** `rstest` for parameterized tests, `tempfile::TempDir` for
filesystem tests. Platform-specific FFI introspection helpers (e.g. `current_qos()` on macOS, `current_nice()` on Linux) live in `src/test_support.rs` and are shared across all `*_tests.rs` modules via `use crate::test_support::*`.

**No `.unwrap()` / `.expect()` in production code** — use `?`. `clippy.toml`
allows them in tests only (`allow-unwrap-in-tests = true`). Workspace lints also
warn on `cognitive_complexity`.

**Concurrency:** `parking_lot` mutexes over `std::sync`.

**Errors:** `thiserror` hierarchy in `error.rs`.

**Logging:** `tracing` macros.

**Platform code:** keep OS-specific FFI behind the `backend/` modules,
`cfg`-gated; platform-specific tests are `cfg`-gated too. Windows can't be tested
on the author's hardware — review FFI carefully and lean on CI.
