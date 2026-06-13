# bgrt — Design

This document captures the **durable design** of `bgrt`: the mechanism, the
decisions, and the things we learned. For the phased build plan and status see
[`ROADMAP.md`](ROADMAP.md); for the contributor module map see
[`../CLAUDE.md`](../CLAUDE.md).

## Purpose

Run units of work — async tasks and OS threads — at the lowest energy footprint
the OS allows (efficiency cores, low clock frequency, no fan spin-up), as a
regular (non-admin) user, on macOS, Windows, and Linux, including heterogeneous
(P+E / big.LITTLE) CPUs. Developers write ordinary async/sync Rust and schedule
it onto a quiet executor.

## Goals and non-goals

**Goals**
- Per-thread / per-executor energy classification, unprivileged.
- Don't trigger performance-core turbo or the fans for background work.
- Never starve: quiet work still makes forward progress under load.
- Wrap tokio, don't fork it.

**Non-goals**
- Direct CPU frequency/governor control (not possible per-thread, unprivileged).
- A hard CPU-time quota (e.g. "30% of a core"). That's a separate axis; on Linux
  it belongs to cgroups, and it isn't the "stay cool" goal. Deliberately omitted.
- Dynamic per-*task* re-classification. Classification is per-thread, set once.
- Being a general tokio replacement — `bgrt` *configures* a tokio runtime.

## The mechanism

Every modern OS exposes a per-thread **energy quality-of-service** hint. Setting
it tells the scheduler "optimize this thread for energy, not speed," which it
turns into *both* efficiency-core placement *and* a downward DVFS bias. We
express intent; the kernel does the right thing per machine. We never set
frequency directly (firmware/governor territory, and not per-thread).

The whole library is built on one operation — apply a [`QosClass`] to the
**current** thread — invoked from a runtime's thread-start hook or at the top of
a spawned thread. Classification is unprivileged because we only ever *lower* a
thread's own demands.

## QoS classes and per-OS mapping

| `QosClass`   | macOS                          | Windows                                  | Linux                                |
|--------------|--------------------------------|------------------------------------------|--------------------------------------|
| `Background` | `QOS_CLASS_BACKGROUND` (E-core-confined, time-shared) | EcoQoS + `THREAD_PRIORITY_BELOW_NORMAL` | `nice(19)` (+ opt-in E-core affinity) |
| `Utility`    | `QOS_CLASS_UTILITY`            | EcoQoS + `THREAD_PRIORITY_NORMAL`        | `nice(10)`                           |
| `Default`    | `QOS_CLASS_DEFAULT`            | clear EcoQoS + normal priority           | `nice(0)`                            |

Rationale:
- **`Background` is the "fans never" class.** On Apple Silicon it is hard-confined
  to efficiency cores — and keeping work *off the P-cores* is what actually avoids
  the turbo/fan spike.
- **`Utility` is "quiet but progresses faster."** On macOS, `Background` can be
  very slow (E-core jail); LLVM/clangd hit exactly this and switched to Utility.
  It's the middle ground: lower priority, but not core-confined.

### Platform notes for future backends

- **FreeBSD:** `setpriority(PRIO_PROCESS, 0, nice)` is **process-wide** on
  FreeBSD — do not copy the Linux backend. FreeBSD threads have kernel LWPs but
  `PRIO_PROCESS` targets the whole process. The correct call is
  `setpriority(PRIO_THREAD, 0, nice)`, a BSD-specific extension for per-LWP
  niceness. Until then, FreeBSD correctly hits the no-op fallback and the API is
  safe to call everywhere.

### Anti-starvation

Each `Background` mapping is **weighted-fair, not run-only-when-idle**, so quiet
work always gets a (small) share under contention:
- Linux uses `nice(19)`, **not `SCHED_IDLE`** (which can starve to ~0% forever).
- Windows uses EcoQoS + `BELOW_NORMAL`, **not `THREAD_PRIORITY_IDLE`**. EcoQoS is
  orthogonal to priority — it handles efficiency; priority handles fairness.
- macOS background QoS is priority-band time-sharing on the E-cores, not idle-only.

## Architecture

A `bgrt` library crate plus a `bgrt-bench` measurement binary.

- `qos` — `QosClass`. `backend/` does `cfg`-gated dispatch of `apply(QosClass)`
  to `macos` (`pthread_set_qos_class_self_np`), `linux` (`setpriority`), `windows`
  (`SetThreadInformation` EcoQoS + `SetThreadPriority`), with a no-op fallback.
- `runtime` (feature `tokio`, default on) — `RuntimeBuilder` → `Runtime`, wrapping
  a multi-thread tokio runtime whose `on_thread_start` applies the class to **every**
  runtime thread (workers + blocking pool).
- `rayon_pool` (feature `rayon`, opt-in) — `RayonBuilder` → `RayonPool`, wrapping
  `rayon::ThreadPool` with a `start_handler` applying QoS to every rayon thread.
  `RayonPool` derefs to `rayon::ThreadPool`; `pool.install(|| …)` routes all
  `par_iter`/`join`/`scope` work through the quiet threads.
- `thread` — `spawn_thread` / `ThreadBuilder` for the non-async, non-rayon path.
  Available with no feature flags.
- `topology` — efficiency-core detection (Linux sysfs `cpu_capacity`) + affinity.
- `telemetry` (feature `telemetry`, opt-in) — measurement primitives for the harness.

### The two-runtime pattern

"Run some work normally, other work quietly" is expressed as **which executor you
spawn onto**, not a mutable per-task flag: keep a `Default`-class `Runtime` and a
`Background`-class `Runtime` in the same process and `spawn` onto the right one.
This is the idiomatic shape and it sidesteps Linux's one-way `nice` (a thread
can't raise its own priority back unprivileged) by classifying each worker once
at creation.

### Efficiency-core affinity (opt-in, Linux)

`pin_efficiency_cores(true)` adds `sched_setaffinity` to the detected E-core set
on Linux. It's **opt-in and off by default**: macOS/Windows already place work
via QoS/EcoQoS, and pinning is a hard restriction that can hurt if the E-cores
are saturated. E-cores are detected as the minimum-capacity CPUs in sysfs
`cpu_capacity` (homogeneous/unknown ⇒ no pinning).

## Measurement (telemetry + harness)

The harness compares executors on the same CPU-bound workload. Design choices:
- **Throughput is the primary signal.** Runs are duration-bounded and the workload
  self-counts work units, so a quieter executor visibly completes less work —
  measurable **unprivileged on every platform**, including macOS.
- **Self-sampling** attributes core placement to the worker actually running.
- **Graceful degradation:** any signal the OS/privilege can't provide is reported
  as `n/a`/`None`, never an error.

Availability:

| Signal      | Linux                | Windows                     | macOS                       |
|-------------|----------------------|-----------------------------|-----------------------------|
| throughput  | ✅                   | ✅                          | ✅                          |
| CPU / E-P   | ✅ sysfs             | CPU index only (E/P TODO)   | via `powermetrics` (sudo)   |
| frequency   | ✅ sysfs             | ✅ `CallNtPowerInformation` | via `powermetrics` (sudo)   |
| energy      | RAPL (often root)    | —                           | `powermetrics` (sudo)       |

## Privileges

The **library never needs privileges** — it only lowers its own threads. Only the
*measurement harness* may need elevation: macOS `powermetrics` (sudo) for
frequency/power/residency, and Linux RAPL energy (often root since CVE-2020-8694).

## Findings worth remembering

- **Linux on homogeneous CPUs: `nice(19)` is a null result without contention.**
  On a homogeneous CPU (no `cpu_capacity` sysfs, e.g. Threadripper, Xeon), `nice`
  only deprioritizes when other threads compete for the same core. A single active
  thread at nice 19 gets full CPU time and full clock speed — throughput, frequency,
  and RAPL energy are indistinguishable from nice 0. Meaningful Linux results require
  a heterogeneous (P+E) CPU (Alder Lake, Raptor Lake) — where E-core affinity via
  `sched_setaffinity` is the real lever — or a CPU-loaded machine.
- **Linux RAPL is whole-package on workstation/server CPUs.** On a 16-core
  Threadripper, `energy_uj` reflects the entire package (all cores + memory
  controller + I/O die). Per-thread power attribution is not possible: variance
  between executors (<3%) is measurement noise, not a real signal.
- **macOS QoS promotion / priority inversion.** A higher-QoS thread that
  synchronously `join`s (or otherwise blocks on) a background thread *promotes it
  off the efficiency cores*. An async `await` on a background runtime does not.
  Practical effect: fire-and-forget background threads stay quiet, but blocking a
  foreground thread on one can speed it up. (The harness matches the waiter's QoS
  to measure the executor, not the join.)
- **tokio's `on_thread_start` covers the blocking pool**, so `spawn_blocking`
  work is classified too — verified by test. No `spawn_blocking`-side workaround.
- **Frequency can be biased, not guaranteed.** Keeping work off P-cores is the
  effective lever; E-cores can still clock up, but at far lower thermal cost.
