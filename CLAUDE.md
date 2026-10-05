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
cargo run --example low_priority_threads -p bgrt

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
  - `backend/` — per-OS dispatch (`macos.rs`, `linux.rs`, `windows.rs`), each exposing `apply(QosClass)` acting on the *current* thread. macOS = `pthread_set_qos_class_self_np` (which also throttles disk I/O, for free); Linux = `setpriority` **+ `backend/ioprio.rs`** (`ioprio_set` → best-effort 7 for `Background`, 6 for `Utility`, untouched for `Default` — best-effort, never `IOPRIO_CLASS_IDLE`, same anti-starvation rule as `nice` over `SCHED_IDLE`); Windows = `THREAD_MODE_BACKGROUND_BEGIN` background mode (the only per-thread I/O lever; `Background` only, since it's all-or-nothing and would sink `Utility`'s CPU priority too) + memory-priority restore + EcoQoS via `SetThreadInformation` + `SetThreadPriority`. **Never `PROCESS_MODE_BACKGROUND_BEGIN`** — undocumented 32 MiB working-set cap, 250–800× slowdowns; Mozilla and Chromium both rejected it. "Already in that state" errors are swallowed so `apply` stays idempotent. A no-op fallback covers other platforms. `backend/uclamp.rs` adds an opt-in Linux `sched_setattr` utilization clamp (`clamp_current_thread`): for `Background`, caps `util_max` (~20%) so the cpufreq governor picks a lower clock even on homogeneous CPUs where `nice` has no frequency effect. Best-effort (no-op on old kernels/non-schedutil governors), unprivileged (only lowers), no-op off Linux. `note_clamp_governor` logs at debug, once per build from each builder, when no policy runs `schedutil`.
  - `runtime` *(feature `tokio`, on by default)* — `RuntimeBuilder` → `Runtime` wrapping a multi-thread tokio runtime; applies `QosClass` to every runtime thread (workers + blocking pool) via `on_thread_start`. `spawn` / `spawn_blocking` / `block_on` / `handle` / `qos` / `shutdown_timeout` / `shutdown_background`. `current_thread(true)` switches to tokio's current-thread scheduler, driven on **one `bgrt`-spawned, classified OS thread** — never the caller's, because `on_thread_start` fires only for the blocking pool in that mode and reclassifying a foreign thread is unsound (one-way `nice` on Linux). That mode makes `Runtime::inner` a private `MultiThread | Dedicated` enum, routes `block_on` through `Handle::block_on`, and signals teardown to the driver over a `oneshot<ShutdownMode>`. See `docs/DESIGN.md` for the rationale.
  - `rayon_pool` *(feature `rayon`, off by default)* — `RayonBuilder` → `RayonPool` wrapping `rayon::ThreadPool`; applies `QosClass` in `start_handler`. `RayonPool` derefs to `rayon::ThreadPool`; use `pool.install(|| …)` to run `par_iter`/`join`/`scope` work on the low-priority threads.
  - `thread` — `spawn_thread` (infallible, like `std::thread::spawn`) and `ThreadBuilder` (`io::Result`, like `std::thread::Builder`); applies QoS at the top of the thread body. Available with no feature flags.
  - `topology` — private module. **Detection** (`efficiency_cores()`) works on Linux (sysfs `cpu_capacity`) *and* Windows (`GetSystemCpuSetInformation` → `EfficiencyClass`), both funnelled through the pure `select_efficiency_cores` (min value wins; all-equal ⇒ empty, i.e. homogeneous, never "all cores are E-cores"). **Pinning** (`pin_current_thread`, `sched_setaffinity`) stays Linux-only on purpose — EcoQoS already places Windows work, and a hard mask would fight it. Reached via the builders' `pin_efficiency_cores` knob and by `telemetry::sample()`.
  - `telemetry` *(feature `telemetry`, off by default)* — measurement primitives: `sample()` (cpu/core-type/freq), `energy_uj()`/`EnergyMeter`, `Aggregate`. Graceful `None`/`Unknown` where unavailable.
  - `error` — `thiserror` `Error` (`Backend`; `Runtime` gated on `tokio`; `ThreadPool` gated on `rayon`).
  - `examples/` — `background_task`, `mixed_runtimes` (require feature `tokio`), `low_priority_threads`.

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
  - `report` — `Summary`/`IoSummary`, aligned tables, JSON, the three-way `Verdict` from `frequency_verdict` (mean MHz, 5% band) and `disk_verdict` (`fg_prot%`, 5-point band), plus `device_saturated` (an unsaturated device makes `Default` look as polite as `Background`). Bands exist because equal rows used to print a win or a loss decided by noise.
  - `cpufreq` — Linux governor probe printed above the CPU table; `--clamp-frequency` under anything but `schedutil` gets a note, since the clamp cannot act there.
  - `power` — pure, cross-platform `PowerStats` parser for `powermetrics` output.
  - `power_macos` *(macOS only)* — `Sampler` that runs `powermetrics` per executor run (needs sudo).

## QoS mapping

| Class | macOS | Windows | Linux |
|---|---|---|---|
| `Background` | `QOS_CLASS_BACKGROUND` (E-core-confined, throttled I/O) | background mode (throttled I/O) + EcoQoS + `BELOW_NORMAL` | `nice(19)` + ioprio BE 7 + opt-in E-core affinity + opt-in `uclamp` frequency cap |
| `Utility` | `QOS_CLASS_UTILITY` | EcoQoS + `NORMAL` | `nice(10)` + ioprio BE 6 |
| `Default` | passthrough | clear throttling | `nice(0)`, ioprio untouched |

Every `Background` mapping is weighted-fair (not run-only-when-idle) so low-priority
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
Integration tests live in `crates/<crate>/tests/`. The `bgrt-bench` integration test (`crates/bgrt-bench/tests/comparison.rs`) runs the harness binary end-to-end via `CARGO_BIN_EXE_bgrt-bench` and asserts background peak MHz ≤ default — peaks, not the means the harness verdict uses, because at 0.3 s `default` runs first from an idle clock and its mean carries the ramp-up. Its disk counterpart asserts **plumbing only** (well-formed report, no read errors, non-zero throughput) — a sub-second run on a shared virtualized CI disk cannot measure `fg_prot%` reliably, so the headline claim is evidenced in `docs/BENCHMARKS.md`, not in CI.

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
executes** — a 5-entry matrix (Linux, macOS, Windows, plus `ubuntu-24.04-arm`
and `windows-11-arm`; `macos-latest` is already aarch64) running clippy, the full
suite, four feature permutations, and an MSRV job. Those runners are VMs: they
prove the backends *run* on aarch64, not that placement or frequency behave —
nothing measurable comes out of a homogeneous vCPU with no cpufreq or RAPL. The
two cross-compile `clippy` commands above are
the *local* pre-push check on the author's macOS hardware: they prove the other
backends type-check, not that they work. Run them before calling a change done,
review FFI carefully, and expect CI to be the real verdict.

**Supply chain:** `deny.toml` + the `deny` CI job (`cargo deny check`) enforce
two things against the committed lock file — no RustSec advisories, and a
**permissive-only licence allow-list**. Copyleft is excluded by omission, so
adding an MPL/LGPL/GPL dependency fails CI by design; the fix is a PR that edits
the allow-list and justifies it, never a silent `exceptions` entry. Run
`cargo deny check` locally before adding or updating a dependency.

**`Cargo.lock` is committed and every CI job runs `--locked`** — that's what
makes the matrix reproducible and keeps a dependency's own MSRV bump from
breaking the 1.85 job on a day nothing changed. (Committing it doesn't affect
downstream users: a dependency's lock file is ignored.) The blind spot that
creates — nothing ever resolving fresh — is covered by the weekly `fresh-deps`
job, which runs `cargo update` in the runner and then clippy + tests. It is
**scheduled/manual only**, so it can never block a PR; a failure there is a
notification that an upstream release broke us, and the fix is a deliberate
lock-file update, not a CI change.

**Semver:** `QosClass` and `Error` are `#[non_exhaustive]`. MSRV increases are
minor bumps, never patches. `telemetry` is exempt from semver entirely. A Tokio
or rayon major is a `bgrt` major — the crate wraps those runtimes rather than
hiding them. All four commitments are stated in the README's *Stability* section
and the crate-level rustdoc; keep them in sync.

**Feature-gated public items need a docs.rs badge:** add
`#[cfg_attr(docsrs, doc(cfg(feature = "…")))]` alongside the `#[cfg(feature =
"…")]`. Without it, docs.rs (which builds `--all-features`) renders gated items
as though they were always available.

**Docs are part of "done".** Each file has one job; keep changes in the right one
rather than restating across them:

| File | Holds | Scope |
|---|---|---|
| `CHANGELOG.md` | What shipped, grouped Added/Changed/Fixed/… under the pending release | One line per change. Not a narrative — reasoning goes in `DESIGN.md` |
| `docs/DESIGN.md` | Why: rationale, negative results, caveats, findings worth remembering | Long-form. The only file that should grow |
| `docs/BENCHMARKS.md` | Measured results, how to run the harness per OS, data points still wanted | Refresh when behaviour changes; add a row to *Data points still wanted* when a gap appears |
| `docs/ROADMAP.md` | 0.9 to 1.0 only: release steps, evaluation window, stability commitments, open gaps | Short. Anything not gating 1.0 belongs elsewhere |
| `README.md` | What the crate does, how to use it, and the caveats that change how it should be used | Link out rather than expand |

**Writing style for `README.md` and `docs/*.md`.** These rules exist because the
docs had drifted into a generated-sounding register. Apply them to prose in those
files and to rustdoc; `CHANGELOG.md` follows them too, minus the tone notes.

- **No second person, no first person.** Not "your tasks", "you can", "we chose",
  "our design". Describe the library and what it does: "runs async tasks", "the
  class covers both axes", "the first version wrote X". Imperatives are fine in
  instructions ("Build first, then run the binary"). The dual-licence boilerplate
  at the end of the README is standard legal text and stays as it is.
- **Bold is for bullet lead-ins only** — the first word or phrase of a list item —
  plus the first `bgrt` in a document. No bold mid-sentence, none in table cells,
  none opening a paragraph. Italics are for genuine contrast (*more* per work
  unit), used sparingly. If a sentence needs bold to land, rewrite it.
- **No decorative icons.** Write "yes" and "no" in tables, not ✅ and ❌. The
  README's CI and version badges are fine.
- **Do not sell.** The reader decides whether the crate is worth using; the docs
  explain what it does and why it does it that way. Avoid "the whole point",
  "what it really sells", "the headline result", "load-bearing", "genuinely",
  "crucially", "precisely the failure". State the fact and stop.
- **Plain words, short sentences.** Prefer "use" over "utilize", "about" over
  "approximately", "needs" over "requires" where it reads naturally, "does
  nothing" over "is inert". Cut intensifiers — "deliberately", "notably",
  "critically" — unless the deliberateness is the actual point.
- **Understate the caveats.** They land harder plainly stated: "nobody has run it
  on that hardware" beats "a critical unverified gap".
- **Say "low-priority", not "quiet".** The library lowers scheduling priority;
  "quiet" is vague and overlaps with the fan-noise sense. Keep "quiet" or
  "quieter" only where the subject really is noise or heat ("a cooler, quieter
  machine"). Related: "low-priority work", "a lower-priority executor", "work at
  low priority".

**Don't restate the QoS mapping table.** The canonical copy is in the README
(*QoS classes*); this file's table is the contributor quick-reference and the only
sanctioned duplicate. It previously lived in five places and had drifted in
three. `docs/` links to the README rather than copying it — keep it that way.
