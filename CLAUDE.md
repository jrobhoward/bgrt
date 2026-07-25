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
cargo test -p bgrt                                  # single crate (default features: tokio on)
cargo test -p bgrt --features telemetry             # incl. telemetry module
cargo test -p bgrt --features rayon                 # incl. rayon_pool module
cargo test -p bgrt --no-default-features            # lean build: no tokio, no rayon
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

# Comparison harness (sudo + --mac-power on macOS for %E/freq/power).
# Build first, then sudo the *binary*: `sudo cargo run` rebuilds as root and may
# not find the toolchain. Release profile is lto + codegen-units=1, so it's slow.
cargo run --release -p bgrt-bench -- --duration 3
cargo build --release -p bgrt-bench && sudo ./target/release/bgrt-bench --duration 3 --mac-power
```

## Architecture

Cargo workspace, edition 2024, `rust-version = 1.85.0`.

- **`bgrt`** — the library.
  - `qos` — `QosClass { Background, Utility, Default }`, the energy class applied per thread.
  - `backend/` — per-OS dispatch (`macos.rs`, `linux.rs`, `windows.rs`), each exposing `apply(QosClass)` acting on the *current* thread. macOS = `pthread_set_qos_class_self_np`; Linux = `setpriority`; Windows = EcoQoS via `SetThreadInformation` + `SetThreadPriority`. A no-op fallback covers other platforms. `backend/uclamp.rs` adds an opt-in Linux `sched_setattr` utilization clamp (`clamp_current_thread`): for `Background`, caps `util_max` (~20%) so the cpufreq governor picks a lower clock even on homogeneous CPUs where `nice` has no frequency effect. Best-effort (no-op on old kernels/non-schedutil governors), unprivileged (only lowers), no-op off Linux.
  - `runtime` *(feature `tokio`, on by default)* — `RuntimeBuilder` → `Runtime` wrapping a multi-thread tokio runtime; applies `QosClass` to every runtime thread (workers + blocking pool) via `on_thread_start`. `spawn` / `spawn_blocking` / `block_on` / `handle` / `qos` / `shutdown_timeout` / `shutdown_background`. `current_thread(true)` switches to tokio's current-thread scheduler, driven on **one `bgrt`-spawned, classified OS thread** — never the caller's, because `on_thread_start` fires only for the blocking pool in that mode and reclassifying a foreign thread is unsound (one-way `nice` on Linux). That mode makes `Runtime::inner` a private `MultiThread | Dedicated` enum, routes `block_on` through `Handle::block_on`, and signals teardown to the driver over a `oneshot<ShutdownMode>`. See `docs/DESIGN.md` for the rationale.
  - `rayon_pool` *(feature `rayon`, off by default)* — `RayonBuilder` → `RayonPool` wrapping `rayon::ThreadPool`; applies `QosClass` in `start_handler`. `RayonPool` derefs to `rayon::ThreadPool`; use `pool.install(|| …)` to run `par_iter`/`join`/`scope` work on the quiet threads.
  - `thread` — `spawn_thread` (infallible, like `std::thread::spawn`) and `ThreadBuilder` (`io::Result`, like `std::thread::Builder`); applies QoS at the top of the thread body. Available with no feature flags.
  - `topology` — private module. **Detection** (`efficiency_cores()`) works on Linux (sysfs `cpu_capacity`) *and* Windows (`GetSystemCpuSetInformation` → `EfficiencyClass`), both funnelled through the pure `select_efficiency_cores` (min value wins; all-equal ⇒ empty, i.e. homogeneous, never "all cores are E-cores"). **Pinning** (`pin_current_thread`, `sched_setaffinity`) stays Linux-only on purpose — EcoQoS already places Windows work, and a hard mask would fight it. Reached via the builders' `pin_efficiency_cores` knob and by `telemetry::sample()`.
  - `telemetry` *(feature `telemetry`, off by default)* — measurement primitives: `sample()` (cpu/core-type/freq), `energy_uj()`/`EnergyMeter`, `Aggregate`. Graceful `None`/`Unknown` where unavailable.
  - `error` — `thiserror` `Error` (`Backend`; `Runtime` gated on `tokio`; `ThreadPool` gated on `rayon`).
  - `examples/` — `background_task`, `mixed_runtimes` (require feature `tokio`), `quiet_threads`.

**The three builders are deliberately parallel.** `RuntimeBuilder`, `RayonBuilder`,
and `ThreadBuilder` each expose the same trio — `qos(QosClass)`,
`pin_efficiency_cores(bool)`, `clamp_frequency(bool)` — and each resolves them the
same way at build time: capture the flags, look up E-cores once on the spawning
thread, then apply QoS → pin → clamp at the top of every worker thread
(`on_thread_start` / `start_handler` / thread body). That last step is the shared
`thread::classify(class, &e_cores, clamp)` — one function, three call sites; keep
it that way. Both extra knobs default to **off** and only have an effect on Linux.
`spawn_thread(class, f)` is the no-knobs shortcut. When adding an option, add it
to all three or explain why not.
- **`bgrt-bench`** — the comparison harness binary (enables `bgrt/telemetry`).
  - `workload` — CPU-bound, self-sampling loop; returns work units (throughput).
  - `runner` — `Executor` (Default/Utility/Background/BackgroundThreads) → `RunResult` (wall, work, aggregate, energy, powermetrics). On macOS the threads runner matches the waiter's QoS during `join` (avoids the kernel promoting background threads off E-cores).
  - `report` — `Summary`, aligned table, JSON, `background_not_hotter` verdict.
  - `power` — pure, cross-platform `PowerStats` parser for `powermetrics` output.
  - `power_macos` *(macOS only)* — `Sampler` that runs `powermetrics` per executor run (needs sudo).

## QoS mapping

| Class | macOS | Windows | Linux |
|---|---|---|---|
| `Background` | `QOS_CLASS_BACKGROUND` (E-core-confined) | EcoQoS + `BELOW_NORMAL` | `nice(19)` + opt-in E-core affinity + opt-in `uclamp` frequency cap |
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

**Test helpers:** `rstest` for parameterized tests. (`tempfile` is declared in
`[workspace.dependencies]` but is not wired into `bgrt`'s dev-deps and is unused —
sysfs-reading code is tested by extracting a pure function, e.g.
`topology::select_efficiency_cores`, rather than by faking a filesystem.)
Platform-specific FFI introspection helpers (e.g. `current_qos()` on macOS, `current_nice()` on Linux) live in `src/test_support.rs` and are shared across all `*_tests.rs` modules via `use crate::test_support::*`.

**No `.unwrap()` / `.expect()` in production code** — use `?`. `clippy.toml`
allows them in tests only (`allow-unwrap-in-tests = true`). Workspace lints also
warn on `cognitive_complexity`.

**Public docs:** the library is `#![warn(missing_docs)]` — every new public item
needs a doc comment. Doc examples that use a gated API must be `cfg`-gated too
(see the `# #[cfg(feature = "tokio")] fn main()` pattern in `lib.rs`);
`cargo test --workspace` runs them.

**Concurrency:** `parking_lot` mutexes over `std::sync` (currently only
`bgrt-bench` needs one — the `bgrt` library holds no locks).

**Errors:** `thiserror` hierarchy in `error.rs`.

**Logging:** `tracing` macros.

**Platform code:** keep OS-specific FFI behind the `backend/` modules,
`cfg`-gated; platform-specific tests are `cfg`-gated too. There is **no CI in
this repo** — the two cross-compile `clippy` commands above are the substitute,
and they are the only check Windows and Linux code gets on the author's macOS
hardware. Run them before calling a change done, and review FFI carefully.

**Docs are part of "done":** land a dated entry in `CHANGELOG.md` (running
project state), update the phase/status table in `docs/ROADMAP.md`, and put
durable rationale — including negative results and honest caveats — in
`docs/DESIGN.md`. The README carries the measured per-platform benchmark tables;
refresh them when behaviour changes.
