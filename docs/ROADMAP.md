# bgrt — Roadmap & Implementation Plan

> **bgrt** ("background runtime") is a Rust library that runs units of work —
> async tasks *and* spawned threads — at the lowest energy footprint the OS
> allows: on efficiency cores, at low clock frequency, without spinning up the
> fans, while still guaranteeing *some* forward progress under load. Developers
> write ordinary async/sync Rust and schedule it onto a quiet executor.

## Context & goals

The aim is **energy-efficient background execution** that a developer opts into
per-unit-of-work, as a regular (non-admin) user, on macOS, Windows, and Linux —
including heterogeneous (big.LITTLE / P+E) and DVFS (SpeedStep / Turbo) CPUs.

Hard requirements that shape the design:

1. **No admin / root privileges** for the library. We only ever *lower* our own
   threads' demands, which every OS permits unprivileged. (The optional
   *measurement* harness may need elevation for power readings — see Phase 4/5.)
2. **Per-thread / per-executor control.** The same process runs some work
   normally and other work in the quiet class. QoS is a property of *which
   executor / spawn you choose*, classified once at thread start — not a mutable
   per-task flag (this also sidesteps Linux's one-way `nice`).
3. **Don't trigger P-core turbo / fans.** Express low-energy *intent* to the OS;
   let the kernel route to efficiency cores and bias DVFS down. We do **not** set
   CPU frequency directly (not possible unprivileged / per-thread on any OS).
4. **No indefinite starvation.** Quiet work must still crawl forward when the
   system is busy — so we use weighted-fair low priority (Linux `nice 19`,
   not `SCHED_IDLE`; Windows EcoQoS + `BELOW_NORMAL`, not `IDLE`; macOS
   background QoS, which time-shares E-cores rather than running idle-only).

We **wrap** tokio (via `Builder::on_thread_start`); we do **not** fork/patch it.

## The mechanism (per-OS QoS mapping)

A single per-OS knob expresses "optimize this thread for energy, not speed,"
which the kernel turns into *both* efficiency-core placement *and* lower DVFS.

| `QosClass`     | macOS                          | Windows                                              | Linux                                                            |
|----------------|--------------------------------|------------------------------------------------------|------------------------------------------------------------------|
| **`Background`** | `QOS_CLASS_BACKGROUND` (E-core-confined, time-shared) | EcoQoS (`THREAD_POWER_THROTTLING_EXECUTION_SPEED`) **+** `THREAD_PRIORITY_BELOW_NORMAL` | `nice(19)` (SCHED_OTHER) **+** affinity to detected E-cores      |
| **`Utility`**    | `QOS_CLASS_UTILITY` (quiet, all cores) | EcoQoS **+** `THREAD_PRIORITY_NORMAL`                | `nice(10)`, no affinity restriction                              |
| **`Default`**    | no hint (passthrough)          | clear power throttling, normal priority              | `nice(0)`, no affinity                                           |

Why these specifics:
- **`Background` is the "fans never" class.** On Apple Silicon it is hard-confined
  to E-cores; that confinement is what actually avoids the P-core turbo/fan spike.
- **`Utility` is "quiet but progresses faster"** — useful on macOS where
  `Background` can be *very* slow (LLVM/clangd hit this and switched to Utility).
- **Anti-starvation:** every `Background` mapping is weighted-fair, not run-only-
  when-idle — guaranteeing a small but nonzero share under contention.

Honest limitation: we can strongly *bias* against frequency increase (chiefly by
keeping work off P-cores), but cannot *guarantee* zero clock-up from userspace.

## Workspace layout

```
bgrt/
├── Cargo.toml                 # workspace: members, lints, shared deps, release profile
├── clippy.toml                # allow-unwrap-in-tests = true
├── CLAUDE.md                  # architecture + conventions for contributors
├── README.md                  # usage
├── CHANGELOG.md               # running project-state log (we maintain this)
├── docs/
│   └── ROADMAP.md             # this file — the phased plan + status table
└── crates/
    ├── bgrt/                  # the library (QoS backends, Runtime wrapper, thread spawn, telemetry)
    └── bgrt-bench/            # binary: the time/placement/frequency/power comparison harness
```

## Public API (target shape)

```rust
// Classification, chosen per executor / per spawn.
pub enum QosClass { Background, Utility, Default }

// Quiet async executor — wraps a tokio runtime.
let rt = bgrt::Builder::new()
    .qos(QosClass::Background)
    .worker_threads(1)
    .pin_efficiency_cores(true)   // Linux affinity; no-op where the OS handles it
    .build()?;
rt.spawn(async { /* quiet async work */ });
rt.spawn_blocking(|| { /* quiet blocking work; pool threads also classified */ });
let handle = rt.handle();         // hand to libraries expecting a tokio Handle

// Quiet OS thread (non-tokio path).
let jh = bgrt::spawn_thread(QosClass::Background, || { /* CPU-bound loop */ });

// Apply to the current thread directly (escape hatch).
bgrt::apply(QosClass::Utility)?;
```

"Run some tasks normally, others quietly" = keep a normal tokio runtime **and** a
`bgrt` runtime in the same process and `spawn` onto the appropriate one.

## Architecture

- **`qos`** — `QosClass`; the `Backend` trait (`fn apply(QosClass) -> Result<(), Error>`
  acting on the *current* thread) with `cfg`-gated impls: `backend/macos.rs`
  (`pthread_set_qos_class_self_np` via `libc`), `backend/linux.rs`
  (`setpriority` + `sched_setaffinity` + sysfs E-core detection),
  `backend/windows.rs` (`SetThreadInformation` + `SetThreadPriority` via
  `windows-sys`). Unprivileged on all three.
- **`runtime`** — `Builder` + `Runtime`, wrapping `tokio::runtime` with
  `.on_thread_start(move || qos::apply(class))`. `spawn` / `spawn_blocking` /
  `handle`. (Verify at impl that `on_thread_start` covers the blocking pool; if
  not, classify inside the `spawn_blocking` closure.)
- **`thread`** — `spawn_thread` / `ThreadBuilder` (name / stack / affinity) for
  the non-async path; applies QoS at the top of the thread closure.
- **`topology`** — E-core / P-core detection, shared by Linux affinity and the
  harness's core-type labeling (Linux sysfs `cpu_capacity`; Windows
  `GetSystemCpuSetInformation` efficiency class; macOS `hw.perflevel*` sysctls).
- **`telemetry`** (feature-gated, used by the harness) — `Telemetry` trait:
  `sample_core() -> CoreSample`, `sample_freq_mhz() -> Option<u32>`,
  `energy_uj() -> Option<u64>`, with per-OS impls that **degrade gracefully**
  (report a signal as unavailable rather than failing).
- **`error`** — `thiserror` hierarchy (`Error` / `BackendError` /
  `TelemetryError`).

## Conventions (inherited from prior projects — apply to every phase)

- Workspace, **edition 2024**, `rust-version` pinned, license `MIT OR Apache-2.0`.
- `thiserror` errors; `parking_lot` over `std::sync`; `tracing` logging.
- **No `.unwrap()` / `.expect()` in production code** — use `?`. Allowed only in
  `#[cfg(test)]` (`clippy.toml` → `allow-unwrap-in-tests = true`).
- Workspace lints (`unwrap_used`, `expect_used`, `cognitive_complexity` = warn);
  `cargo clippy --workspace --tests -- -Dwarnings` clean before a phase is done.
- **Tests in separate `*_tests.rs` files**, registered via
  `#[cfg(test)] #[path = "x_tests.rs"] mod x_tests;`. Integration tests in
  `crates/<crate>/tests/`.
- **Test naming:** `subject____condition____result` — exactly four underscores
  between segments. `rstest` for parameterized, `proptest` for property tests,
  `tempfile::TempDir` for fs tests.
- **Each phase compiles cleanly on all three OSes and ships tests** before it's
  considered done; record the outcome in `CHANGELOG.md`.

## Dependencies (initial)

`tokio` (rt-multi-thread, time), `thiserror`, `parking_lot`, `tracing`;
`clap` (harness CLI), `serde`/`serde_json` (harness JSON output);
`[target.'cfg(unix)'] libc`; `[target.'cfg(windows)'] windows-sys` (feature
sets: `Win32_System_Threading`, `Win32_System_Power`, `Win32_System_SystemInformation`).
Dev: `rstest`, `tempfile`. Release profile: `strip`, `lto`, `codegen-units = 1`.

---

## Phased plan & status

| Phase | Title                                   | Status |
|------:|-----------------------------------------|--------|
| 0 | Workspace scaffold + conventions            | ✅ Done (macOS verified; Linux/Windows backends are cfg-gated no-ops) |
| 1 | QoS backends (macOS / Windows / Linux)      | ✅ Done (macOS run-verified; Linux/Windows cross-check + clippy clean). Affinity deferred to Phase 2. |
| 2 | `Runtime` — tokio wrapper                    | ✅ Done (macOS run-verified incl. blocking pool; Linux/Windows cross-check clean) |
| 3 | Quiet thread + blocking spawn API           | ✅ Done (macOS run-verified; Linux/Windows cross-check clean) |
| 4 | Telemetry (core / frequency / power)        | ✅ Done (primitives; Sampler orchestration + macOS powermetrics → Phase 5) |
| 5 | Comparison harness (`bgrt-bench`)           | ✅ Done (table/JSON + verdict + integration test; macOS run, Linux/Windows cross-check) |
| 6 | Docs, examples, polish                      | ✅ Done (examples run/lint clean; README incl. M1 results; CLAUDE.md refreshed) |
| 7 | Road to 1.0 (CI, licensing, API freeze)     | ✅ Done — CI matrix, licenses/packaging, error causes, shutdown control, `current_thread`, Windows E/P telemetry. **Only hardware-blocked item left: running `--pin` on a real P+E Linux box, now documented as unmeasured rather than implied.** |

Legend: ⬜ not started · 🔶 in progress · ✅ done. Update this table **and**
`CHANGELOG.md` as each phase lands.

### Phase 0 — Workspace scaffold + conventions
- Workspace `Cargo.toml` (members, `[workspace.lints.clippy]`, shared + per-target
  deps, release profile); `clippy.toml`; `CLAUDE.md`, `README.md`, `CHANGELOG.md`.
- Crates `bgrt` (lib) and `bgrt-bench` (bin, hello-world placeholder).
- `error.rs`, `QosClass` enum, `tracing` init helper; `Backend` trait + per-OS
  module skeletons returning `Ok(())` (no-op) so everything compiles on all three.
- **DoD:** `cargo build`/`clippy --tests` clean on macOS, Linux, Windows; trivial
  tests pass (e.g. `QosClass` debug/eq); CHANGELOG seeded.

### Phase 1 — QoS backends — ✅ Done
- **macOS:** `pthread_set_qos_class_self_np(BACKGROUND|UTILITY|DEFAULT, 0)` FFI
  (declared against libSystem). Read-back tests via `pthread_get_qos_class_np`.
- **Linux:** `setpriority(PRIO_PROCESS, 0, nice)` — `nice(19|10|0)`, weighted-fair
  (no `SCHED_IDLE`). Read-back tests via `getpriority`.
- **Windows:** EcoQoS (`SetThreadInformation(.., ThreadPowerThrottling,
  EXECUTION_SPEED)`) + `SetThreadPriority(BELOW_NORMAL|NORMAL)` via `windows-sys`;
  `Default` clears EcoQoS. Priority read-back tests via `GetThreadPriority`.
- `qos::apply(class)` acts on the current thread, unprivileged.
- **Scope note:** efficiency-core **affinity** moved to Phase 2 (it belongs with
  the opt-in `pin_efficiency_cores` builder option + the `topology` module), so
  `apply` stays the always-safe nice/QoS/priority part.
- **DoD met:** macOS run-verified (QoS read-back asserts 0x09/0x11/0x15); Linux &
  Windows cross-compiled (`cargo check --tests`) and clippy-clean on their
  targets; platform tests `cfg`-gated. *Linux/Windows runtime behavior pending
  CI / real hardware.*

### Phase 2 — `Runtime` (tokio wrapper)
- `Builder` (qos, worker_threads default 1, pin_efficiency_cores, thread_name) →
  `Runtime` built from `tokio::runtime::Builder` with the `on_thread_start` hook.
- `spawn`, `spawn_blocking`, `handle`, `block_on`.
- **`topology` + affinity (moved from Phase 1):** detect E-cores (Linux sysfs
  `cpu_capacity`; macOS `hw.perflevel*`; Windows cpu-set efficiency class); when
  `pin_efficiency_cores(true)`, the thread-start hook also restricts affinity to
  the E-core set (Linux `sched_setaffinity`). Opt-in and defaults off.
- Document + test the two-runtime pattern (normal + bgrt in one process).
- **DoD:** a task spawned on a `Background` runtime runs to completion; a test
  *inside* a spawned task asserts the worker thread carries the QoS (Linux:
  reads its own `nice` == 19). Blocking-pool classification verified or worked
  around.

### Phase 3 — Quiet thread + blocking spawn API
- `spawn_thread(class, f)` and `ThreadBuilder` (name / stack_size / affinity);
  QoS applied at the top of the thread body.
- Confirm `spawn_blocking` threads are classified (extend hook or classify in the
  closure if tokio's hook doesn't cover the blocking pool).
- **DoD:** a spawned quiet thread reports the expected nice/QoS from within;
  blocking tasks confirmed classified.

### Phase 4 — Telemetry (measurement primitives) — ✅ Done
*Shipped as a `telemetry` feature-gated module in `bgrt` (off by default; no extra
deps). Point queries + pure aggregation; graceful `None`/`Unknown` everywhere.
`sample()` (cpu + core type + freq), `energy_uj()`/`EnergyMeter`, `Aggregate`.
Deviations from the original outline below: the threaded **Sampler** orchestration
moved to Phase 5 (it's tied to how workloads run); **Windows E/P** classification
and the **macOS `powermetrics`** privileged path are deferred to Phase 5. macOS
unprivileged returns `None`/`Unknown` (verified via the bench smoke probe); Linux
gives cpu+type+freq (and energy where RAPL is readable); Windows gives cpu+freq.*

#### Original outline
- `Telemetry` trait + per-OS impls, each signal optional & gracefully degrading:
  - **Core placement / type:** Linux `sched_getcpu` + `topology`; Windows
    `GetCurrentProcessorNumber` + cpu-set efficiency class; macOS — no per-thread
    core API, so cluster residency is read from `powermetrics` (harness-only).
  - **Frequency:** Linux `cpufreq/scaling_cur_freq` (sysfs, unprivileged);
    Windows `CallNtPowerInformation(ProcessorInformation)` (unprivileged);
    macOS via `powermetrics` (needs sudo).
  - **Power/energy:** Linux RAPL `/sys/class/powercap/.../energy_uj` or
    `perf` (often privileged); macOS `powermetrics` (sudo); Windows — typically
    unavailable → report `None`.
- A `Sampler` that polls during a workload and aggregates (cores used, %E vs %P,
  mean/max MHz, energy delta).
- **DoD:** on each OS the harness reads ≥ core placement + frequency unprivileged;
  power where the platform/privilege allows; missing signals reported cleanly.
  Privilege requirements documented in README.

### Phase 5 — Comparison harness (`bgrt-bench`) — ✅ Done
*Shipped: `workload` (CPU-bound, self-sampling), `runner` (Default/Utility/
Background runtimes + Background threads → `RunResult`), `report` (aligned table,
pretty JSON, and the `background_not_hotter` verdict). CLI: `--duration`,
`--workers`, `--interval`, `--executors`, `--format`, `--pin`, `--mac-power`.
Integration test (`tests/comparison.rs`) runs the built binary and asserts
background peak freq ≤ default, skipping where frequency telemetry is absent.
The macOS `powermetrics` reader is a tested, defensive `--mac-power` opt-in
(graceful no-sudo degradation verified); Windows E/P classification was deferred
here and landed later in Phase 7. Verified: macOS run (table/JSON/verdict, honest n/a; 12 bench tests +
integration test), Linux/Windows clippy clean cross-target.*

#### Original outline
- Defined CPU-bound workload(s) (e.g. sustained hashing / integer crunch) run for
  a fixed wall-clock budget or iteration count.
- Runners: `Default` tokio runtime · `Utility` runtime · `Background` runtime ·
  `Background` spawned threads. Each wrapped with the `Sampler`.
- Output a comparison table (and `--json`): executor × {wall-clock, distinct
  cores, %E / %P residency, mean/max MHz, energy (J)}.
- CLI: `--workload`, `--duration`/`--iters`, `--executors`, `--interval`,
  `--format`, `--sudo-power`.
- **Integration test** (`cfg`-gated, tolerant where telemetry is absent):
  `Background` max frequency ≤ `Default` max frequency, and on hybrid HW
  `Background` residency is predominantly E-cores.
- **DoD:** `cargo run -p bgrt-bench` prints the comparison on dev hardware
  (M1 + Linux/AMD); the assertions hold or skip cleanly where unsupported.

### Phase 6 — Docs, examples, polish — ✅ Done
*Shipped: `crates/bgrt/examples/{background_task,mixed_runtimes,quiet_threads}.rs`
(run + `clippy --all-targets` clean); README "Usage" + a real **Apple M1** results
table (Background: 99.8% E-core, ~12× less power); CLAUDE.md architecture/commands
refreshed for the full module set. Remaining at the time (both since addressed in
Phase 7): Windows E/P classification, and CI to execute the Linux/Windows paths.*

#### Original outline
- `README.md` (install, the QoS table, usage, privilege notes); `CLAUDE.md`
  (architecture, commands, conventions); `examples/` (`background_task.rs`,
  `mixed_runtimes.rs`, `quiet_threads.rs`).
- Final `CHANGELOG.md` pass; note any deviations from this plan.
- **DoD:** examples build & run on all three OSes; docs match the shipped API.

---

## Open questions / risks (revisit during implementation)

- ~~**tokio `on_thread_start` & the blocking pool**~~ — ✅ Resolved (Phase 2): the
  hook fires for blocking-pool threads too; a macOS test confirms `spawn_blocking`
  work carries the runtime's QoS. No `spawn_blocking`-side workaround needed.
  *Amended (Phase 7):* true for the **multi-thread** scheduler only. On a
  current-thread runtime the hook fires *only* for the blocking pool, never for
  the thread driving async tasks — which is why `current_thread(true)` runs the
  scheduler on a `bgrt`-owned, classified thread. See `docs/DESIGN.md`.
- ~~**No current-thread runtime**~~ — ✅ Resolved (Phase 7) as
  `RuntimeBuilder::current_thread(bool)`, with `bgrt` owning the driver thread.
- **Hybrid-Linux E-core pinning is unverified** — the selection logic is
  unit-tested, but the sysfs read and `sched_setaffinity` have never run against
  a non-empty core set (no P+E hardware available). Documented as a caveat in the
  README rather than implied to be measured. Needs Alder/Raptor/Meteor Lake.
- **macOS core placement** has no per-thread API → depends on `powermetrics`
  (root) for the harness; the *library* needs no privilege. (Phase 4.)
- **RAPL access on Linux** is frequently restricted post-CVE; the harness must
  fall back to "power: unavailable" without erroring. (Phase 4.)
- **Windows power telemetry** is likely unavailable without ETW; plan to report
  frequency only there. (Phase 4.)
- **Windows can't be tested on the author's hardware** — rely on CI / careful
  FFI review; keep the Windows backend behind the same trait and tests `cfg`-gated.
- **`Background` slowness on macOS** (E-core jail) — documented; `Utility` is the
  recommended middle ground. Consider a `Builder` default and clear guidance.
- **Affinity vs. the kernel scheduler on Linux hybrid** — pinning to E-cores is a
  hard restriction; if E-cores are saturated we still get a fair (nice-weighted)
  share of them. Make `pin_efficiency_cores` opt-in so users can defer to the
  scheduler's own hybrid placement instead.
