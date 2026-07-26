# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`bgrt` ("background runtime") runs async tasks and spawned threads at a low
energy footprint — efficiency cores, low clock frequency, no fan spin-up — while
still making forward progress under load, as a regular (non-admin) user on
macOS, Windows, and Linux. It **wraps** tokio rather than forking it.

**Scope is CPU + block I/O.** One `QosClass` governs both — deliberately not two
knobs, because Windows and macOS bundle the axes and an API can't honour
combinations the OS won't express. All three platforms covered.
**GPU is probably never** — no OS exposes a per-thread GPU QoS today, the per-API
priorities that exist arbitrate contention rather than save energy, and GPU work
belongs to a queue rather than to a classifiable thread. That answer is contingent
on the state of the platforms; `docs/DESIGN.md` → *Scope: CPU and I/O now, GPU
probably never* lists what would reopen it. Read it before adding either.

See `docs/DESIGN.md` for the durable design and rationale, `docs/ROADMAP.md` for
status and the release plan, `docs/BENCHMARKS.md` for measured results, and
`CHANGELOG.md` for running project state. The README is the canonical reference
for what each class does per OS.

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

# Cross-compile checks for the other OS backends (no linker needed for `check`).
# Cover both crates: `bgrt-bench`'s io_file has per-OS code of its own.
cargo clippy -p bgrt --tests --target x86_64-unknown-linux-gnu -- -Dwarnings
cargo clippy -p bgrt --tests --target x86_64-pc-windows-msvc -- -Dwarnings
cargo clippy -p bgrt-bench --all-targets --target x86_64-unknown-linux-gnu -- -Dwarnings
cargo clippy -p bgrt-bench --all-targets --target x86_64-pc-windows-msvc -- -Dwarnings

# Examples
cargo run --example background_task -p bgrt
cargo run --example mixed_runtimes -p bgrt
cargo run --example quiet_threads -p bgrt

# Comparison harness (sudo + --mac-power on macOS for %E/freq/power).
# Build first, then sudo the *binary*: `sudo cargo run` rebuilds as root and may
# not find the toolchain. Release profile is lto + codegen-units=1, so it's slow.
cargo run --release -p bgrt-bench -- --duration 3
cargo build --release -p bgrt-bench && sudo ./target/release/bgrt-bench --duration 3 --mac-power

# Disk half. Reads bypass the page cache and need no privileges anywhere.
# Saturate the device or the classes won't separate — one reader at QD1 leaves
# the SSD idle enough that `Default` looks as polite as `Background` (the
# harness prints a hint when that happens).
cargo run --release -p bgrt-bench -- --workload io --duration 3 --workers 4 --io-foreground 4
```

## Architecture

Cargo workspace, edition 2024, `rust-version = 1.85.0`.

- **`bgrt`** — the library.
  - `qos` — `QosClass { Background, Utility, Default }`, the energy class applied per thread. `#[non_exhaustive]`.
  - `backend/` — per-OS dispatch (`macos.rs`, `linux.rs`, `windows.rs`), each exposing `apply(QosClass)` acting on the *current* thread. macOS = `pthread_set_qos_class_self_np` (which also throttles disk I/O, for free); Linux = `setpriority` **+ `backend/ioprio.rs`** (`ioprio_set` → best-effort 7 for `Background`, 6 for `Utility`, untouched for `Default` — best-effort, never `IOPRIO_CLASS_IDLE`, same anti-starvation rule as `nice` over `SCHED_IDLE`); Windows = `THREAD_MODE_BACKGROUND_BEGIN` background mode (the only per-thread I/O lever; `Background` only, since it's all-or-nothing and would sink `Utility`'s CPU priority too) + memory-priority restore + EcoQoS via `SetThreadInformation` + `SetThreadPriority`. **Never `PROCESS_MODE_BACKGROUND_BEGIN`** — undocumented 32 MiB working-set cap, 250–800× slowdowns; Mozilla and Chromium both rejected it. "Already in that state" errors are swallowed so `apply` stays idempotent. A no-op fallback covers other platforms. `backend/uclamp.rs` adds an opt-in Linux `sched_setattr` utilization clamp (`clamp_current_thread`): for `Background`, caps `util_max` (~20%) so the cpufreq governor picks a lower clock even on homogeneous CPUs where `nice` has no frequency effect. Best-effort (no-op on old kernels/non-schedutil governors), unprivileged (only lowers), no-op off Linux.
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

**Classification stops at the thread `bgrt` created.** Child threads inherit only
on Linux; macOS and Windows start them unclassified, and neither can be fixed
from outside (their APIs are current-thread-only). This is why the thread hooks
matter: classification must happen *at creation*, by whoever creates the thread.
Asserted per-platform in `thread_tests.rs`; don't weaken those tests without
updating the README table they point at.
- **`bgrt-bench`** — the comparison harness binary (enables `bgrt/telemetry`).
  - `workload` — CPU-bound, self-sampling loop; returns work units (throughput).
  - `runner` — `Executor` (Default/Utility/Background/BackgroundThreads) → `RunResult` (wall, work, aggregate, energy, powermetrics). On macOS the threads runner matches the waiter's QoS during `join` (avoids the kernel promoting background threads off E-cores).
  - `io_file` — scratch file + cache-bypassing reads (`O_DIRECT` / `F_NOCACHE` / `FILE_FLAG_NO_BUFFERING`), `AlignedBuf`, and the Linux `queue/scheduler` probe. Bypass failure is reported as `CacheBypass::Buffered`, never silently.
  - `io_workload` — the random-read loop and `PhaseStats`. **Warm-up reads rather than sleeps** — macOS defers timers for `QOS_CLASS_BACKGROUND`, so a sleeping background reader wakes after its window closes. Reads only: buffered writes are issued by the flusher thread, so they'd measure its priority.
  - `io_runner` — solo + contended phases per executor, against plain unclassified foreground threads. Contention is the point: on an idle device low-priority reads run near full speed everywhere, so a solo-only benchmark would read as "the I/O mapping does nothing".
  - `report` — `Summary`/`IoSummary`, aligned tables, JSON, `background_not_hotter` and `background_yields_disk` verdicts, plus `device_saturated` (an unsaturated device makes `Default` look as polite as `Background`).
  - `power` — pure, cross-platform `PowerStats` parser for `powermetrics` output.
  - `power_macos` *(macOS only)* — `Sampler` that runs `powermetrics` per executor run (needs sudo).

## QoS mapping

| Class | macOS | Windows | Linux |
|---|---|---|---|
| `Background` | `QOS_CLASS_BACKGROUND` (E-core-confined, throttled I/O) | background mode (throttled I/O) + EcoQoS + `BELOW_NORMAL` | `nice(19)` + ioprio BE 7 + opt-in E-core affinity + opt-in `uclamp` frequency cap |
| `Utility` | `QOS_CLASS_UTILITY` | EcoQoS + `NORMAL` | `nice(10)` + ioprio BE 6 |
| `Default` | passthrough | clear throttling | `nice(0)`, ioprio untouched |

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
Integration tests live in `crates/<crate>/tests/`. The `bgrt-bench` integration test (`crates/bgrt-bench/tests/comparison.rs`) runs the harness binary end-to-end via `CARGO_BIN_EXE_bgrt-bench` and asserts background peak MHz ≤ default. Its disk counterpart asserts **plumbing only** (well-formed report, no read errors, non-zero throughput) — a sub-second run on a shared virtualized CI disk cannot measure `fg_prot%` reliably, so the headline claim is evidenced in `docs/BENCHMARKS.md`, not in CI.

**Test naming:** `subject____condition____result` — exactly four underscores
between segments. Because consecutive underscores trip `non_snake_case`, every
`*_tests.rs` file carries `#![allow(non_snake_case)]` at the top.

**Test helpers:** `rstest` for parameterized tests. `tempfile` is a dev-dep of
**`bgrt-bench` only**, where the disk workload needs a real scratch file; `bgrt`
itself doesn't use it — sysfs-reading code is tested by extracting a pure
function, e.g. `topology::select_efficiency_cores` or `io_file::parse_scheduler`,
rather than by faking a filesystem.
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
`cfg`-gated; platform-specific tests are `cfg`-gated too. **CI
(`.github/workflows/ci.yml`) is where Linux and Windows code actually
executes** — a 3-OS matrix running clippy, the full suite, four feature
permutations, and an MSRV job. The two cross-compile `clippy` commands above are
the *local* pre-push check on the author's macOS hardware: they prove the other
backends type-check, not that they work. Run them before calling a change done,
review FFI carefully, and expect CI to be the real verdict.

**Semver:** `QosClass` and `Error` are `#[non_exhaustive]`. MSRV increases are
minor bumps, never patches. `telemetry` is exempt from semver entirely. A Tokio
or rayon major is a `bgrt` major — the crate wraps those runtimes rather than
hiding them. All four commitments are stated in the README's *Stability* section
and the crate-level rustdoc; keep them in sync.

**Feature-gated public items need a docs.rs badge:** add
`#[cfg_attr(docsrs, doc(cfg(feature = "…")))]` alongside the `#[cfg(feature =
"…")]`. Without it, docs.rs (which builds `--all-features`) renders gated items
as though they were always available.

**Docs are part of "done":** land a dated entry in `CHANGELOG.md` (running
project state), update `docs/ROADMAP.md` (status + release plan), and put durable
rationale — including negative results and honest caveats — in `docs/DESIGN.md`.
Measured results go in `docs/BENCHMARKS.md`; refresh them when behaviour changes.

**Don't restate the QoS mapping table.** The canonical copy is in the README
(*QoS classes*); this file's table is the contributor quick-reference and the only
sanctioned duplicate. It previously lived in five places and had drifted in
three. `docs/` links to the README rather than copying it — keep it that way.
