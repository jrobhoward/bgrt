# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`bgrt` ("background runtime") runs async tasks and spawned threads at a low
energy footprint — efficiency cores, low clock frequency, no fan spin-up — while
still making forward progress under load, as a regular (non-admin) user on
macOS, Windows, and Linux. It **wraps** tokio rather than forking it.

See `docs/ROADMAP.md` for the phased plan and current status, and `CHANGELOG.md`
for running project state.

## Commands

```bash
# Build
cargo build --workspace
cargo build --workspace --release

# Test
cargo test --workspace
cargo test -p bgrt                                  # single crate
cargo test -p bgrt -- some____test____name          # single test

# Lint (must be clean before any phase is considered done)
cargo clippy --workspace --tests -- -Dwarnings

# Run the comparison harness
cargo run -p bgrt-bench
```

## Architecture

Cargo workspace, edition 2024, `rust-version = 1.85.0`.

- **`bgrt`** — the library.
  - `qos` — `QosClass { Background, Utility, Default }`, the energy class applied per thread.
  - `backend/` — per-OS dispatch (`macos.rs`, `linux.rs`, `windows.rs`), each exposing `apply(QosClass)` acting on the *current* thread. macOS = `pthread_set_qos_class_self_np`; Linux = `setpriority` (+ optional E-core affinity); Windows = EcoQoS via `SetThreadInformation` + `SetThreadPriority`. A no-op fallback covers other platforms.
  - `error` — `thiserror` `Error`.
  - *(later phases)* `runtime` tokio wrapper, `thread` quiet-thread spawner, `topology` E/P-core detection, `telemetry` measurement.
- **`bgrt-bench`** — binary: the time / core-placement / frequency / power comparison harness (Phase 5).

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
Integration tests live in `crates/<crate>/tests/`.

**Test naming:** `subject____condition____result` — exactly four underscores
between segments. Because consecutive underscores trip `non_snake_case`, every
`*_tests.rs` file carries `#![allow(non_snake_case)]` at the top.

**Test helpers:** `rstest` for parameterized tests, `tempfile::TempDir` for
filesystem tests.

**No `.unwrap()` / `.expect()` in production code** — use `?`. `clippy.toml`
allows them in tests only (`allow-unwrap-in-tests = true`). Workspace lints also
warn on `cognitive_complexity`.

**Concurrency:** `parking_lot` mutexes over `std::sync`.

**Errors:** `thiserror` hierarchy in `error.rs`.

**Logging:** `tracing` macros.

**Platform code:** keep OS-specific FFI behind the `backend/` modules,
`cfg`-gated; platform-specific tests are `cfg`-gated too. Windows can't be tested
on the author's hardware — review FFI carefully and lean on CI.
