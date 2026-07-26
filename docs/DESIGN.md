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
- Setting an explicit CPU frequency or switching the governor (firmware/governor
  territory, system-wide). We do express a per-thread *frequency bias*: on Linux
  the opt-in `uclamp` cap (`clamp_frequency`) lowers the clock the governor picks
  for a `Background` thread — a hint, not a set point, and unprivileged.
- A hard CPU-time quota (e.g. "30% of a core"). That's a separate axis; on Linux
  it belongs to cgroups, and it isn't the "stay cool" goal. Deliberately omitted.
- Dynamic per-*task* re-classification. Classification is per-thread, set once.
- Being a general tokio replacement — `bgrt` *configures* a tokio runtime.
- **GPU work of any kind** — see
  [Scope](#scope-cpu-and-io-now-gpu-probably-never). Not on the roadmap and
  unlikely ever to be, but the objections are about the current state of the
  platforms rather than about principle, so they are written as conditions that
  could change.

`bgrt` is a **CPU and block-I/O** scheduling-hint library — one `QosClass`
governs both. GPU is out of scope for the foreseeable future. The reasoning for
both is below, because "why don't you do X" is a question worth answering once in
writing.

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
  runtime thread (workers + blocking pool). `current_thread(true)` selects tokio's
  current-thread scheduler instead — see below.
- `rayon_pool` (feature `rayon`, opt-in) — `RayonBuilder` → `RayonPool`, wrapping
  `rayon::ThreadPool` with a `start_handler` applying QoS to every rayon thread.
  `RayonPool` derefs to `rayon::ThreadPool`; `pool.install(|| …)` routes all
  `par_iter`/`join`/`scope` work through the quiet threads.
- `thread` — `spawn_thread` / `ThreadBuilder` for the non-async, non-rayon path.
  Available with no feature flags.
- `topology` — efficiency-core detection (Linux sysfs `cpu_capacity`, Windows
  `GetSystemCpuSetInformation`) + Linux-only affinity.
- `telemetry` (feature `telemetry`, opt-in) — measurement primitives for the harness.

### The two-runtime pattern

"Run some work normally, other work quietly" is expressed as **which executor you
spawn onto**, not a mutable per-task flag: keep a `Default`-class `Runtime` and a
`Background`-class `Runtime` in the same process and `spawn` onto the right one.
This is the idiomatic shape and it sidesteps Linux's one-way `nice` (a thread
can't raise its own priority back unprivileged) by classifying each worker once
at creation.

### The current-thread runtime owns its driver thread

`RuntimeBuilder::current_thread(true)` gives single-threaded task semantics, but
**not** by driving tasks on the caller's thread the way tokio's current-thread
runtime does. `bgrt` spawns one OS thread (via `ThreadBuilder`, so it is
classified like every other `bgrt` thread), builds the current-thread runtime
*there*, and parks it in `Runtime::block_on` for the runtime's lifetime.

The indirection exists because the obvious implementation is unsound for this
crate, in two independent ways:

1. **The hook doesn't fire for the driver.** On a current-thread runtime,
   `on_thread_start` fires only for blocking-pool threads. Measured: the async
   task observed `QOS_CLASS_DEFAULT` while a `spawn_blocking` closure on the same
   runtime observed `QOS_CLASS_BACKGROUND`. The async work — the part users care
   about — would run entirely unclassified.
2. **Classifying the caller instead is not an option.** It is a thread `bgrt`
   does not own, and on Linux an unprivileged thread can lower its niceness but
   never raise it back, so `bgrt` would permanently deprioritize somebody else's
   thread. This is the same one-way-`nice` constraint that motivates the
   two-runtime pattern above.

Owning the thread resolves both. The costs are a shutdown path that must cross a
thread boundary (a `oneshot` carrying the desired teardown mode, so all three
tokio shutdown behaviours survive) and a `block_on` that goes through
`Handle::block_on` — sound only because the driver thread keeps the I/O and timer
drivers running, which a bare `Handle::block_on` on a current-thread runtime
cannot do for itself.

**It is not the default, and mostly should not be used.** `worker_threads(1)`
costs the same single thread (measured: both modes are `+1`) without the hazard
that one blocking task stalls every other task. Reach for `current_thread` only
when single-threaded task semantics are actually wanted.

### Efficiency-core detection vs. pinning

These are deliberately separate concerns, and they have different platform
support.

**Detection** answers "which CPUs are the little ones" and is used by telemetry
to label samples. Linux reads sysfs `cpu_capacity`; Windows reads
`EfficiencyClass` from `GetSystemCpuSetInformation`. Both scales are "higher is
faster", so both reduce to the same pure function — the CPUs at the minimum
value, with an all-equal machine reported as homogeneous (empty set) rather than
as "everything is an E-core". Keeping that decision in one testable function is
what lets it be verified on a machine with neither topology. macOS has no
unprivileged equivalent, so detection returns empty there.

**Pinning** is Linux only. `pin_efficiency_cores(true)` adds `sched_setaffinity`
to the detected E-core set; it is **opt-in and off by default**, because pinning
is a hard restriction that hurts when the E-cores are saturated. It stays a no-op
on macOS and Windows *even now that Windows detection works*: QoS and EcoQoS
already place work on efficient cores, and a hard affinity mask would fight the
scheduler's own hybrid placement rather than assist it. Knowing which cores are
efficient is not a reason to start overriding an OS that is already doing the
job.

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

## Scope: CPU and I/O now, GPU probably never

`bgrt` classifies **CPU and block-I/O** work. The two obvious "what about…"
questions ended with different answers, recorded here so they don't get
re-litigated — and, for the GPU one, so that the conditions under which the
answer *should* be revisited are written down rather than left to memory.

### I/O priority — in scope; a class covers disk as well as CPU

A `QosClass` governs a thread's *resource* demands, not only its CPU demands.
This was very nearly deferred to 1.1 as demand-gated, and two findings changed
the decision.

**Finding 1: two of three platforms already did it.** The feature was mostly
already present, through mechanisms the crate was using for other reasons:

| | I/O priority | via |
|---|---|---|
| macOS | already throttled | `QOS_CLASS_BACKGROUND` implies disk-I/O throttling — one call, both axes |
| Linux | probably already correct | with no explicit I/O priority the kernel derives a best-effort level from niceness, `(nice + 20) / 5` → `nice(19)` = level 7, `nice(10)` = level 6 |
| Windows | **the one real gap** (since closed) | EcoQoS and thread priority don't touch I/O priority |

That derivation lands on exactly the mapping we would have chosen by hand, which
made "add I/O priority" one platform's gap plus a documentation commitment rather
than a three-platform feature.

**Finding 2: it is a semantic change, so deferring it is the risky option.** If
1.0 had shipped saying "`QosClass` is a CPU knob" and 1.1 then said "it also
governs I/O", that would be a silent behaviour change for existing users —
technically not a semver break, which makes it worse rather than better.
Committing the meaning up front costs nothing (it already described macOS and
probably Linux) and turns the eventual Windows change into a move *toward*
documented behaviour.

So Linux now sets I/O priority explicitly (`backend/ioprio.rs`), and Windows is
documented as the outstanding gap.

**Best-effort, never `IOPRIO_CLASS_IDLE`.** Idle-class I/O only gets the disk
when nothing else wants it — the I/O equivalent of `SCHED_IDLE`, which this
project rejects for CPU because quiet work must still crawl forward. Best-effort
level 7 is the weighted-fair choice, exactly parallel to `nice(19)`.

**Explicit, even though `nice` probably implies it.** One syscall buys
independence from a kernel-internal derivation, intent visible in `strace`, and a
value directly assertable via `ioprio_get` in tests — this project's convention
is to measure rather than assume, and the implicit version cannot be asserted.

**Inert on some configurations, like `uclamp`.** Whether the priority is honoured
is the I/O scheduler's business: BFQ fully, `mq-deadline` since 5.18, `none` — a
common NVMe default — not at all. Documented rather than hidden.

**Windows, resolved by research rather than hardware.** The gap is now closed
with `SetThreadPriority(THREAD_MODE_BACKGROUND_BEGIN)` for `Background`. Three
questions had been blocking it, and all three turned out to be answerable from
documentation and from what Chromium ships:

1. *Does it compose with the existing EcoQoS call?* **Yes** — Chromium applies
   `THREAD_MODE_BACKGROUND_BEGIN` and `THREAD_POWER_THROTTLING_EXECUTION_SPEED`
   to the same threads. They are independent mechanisms.
2. *Is it still weighted-fair — does it starve?* Microsoft documents that such a
   thread "may not be scheduled promptly, but it will never be starved". That
   satisfies the hard requirement, but note the hedge: never-starved is weaker
   than Linux's proportional share, delivered by periodically boosting a thread
   denied the CPU for too long. Poor throughput under sustained foreground load
   is expected and intended.
3. *Does the begin/end pairing matter?* Only in that Windows reports
   "already in that state" as an **error** — `ERROR_THREAD_MODE_ALREADY_BACKGROUND`
   / `ERROR_THREAD_MODE_NOT_BACKGROUND`. Since `apply` may legitimately run more
   than once on a thread, both codes are swallowed; that is what makes `apply`
   idempotent on Windows. Thread exit cleans up, so no explicit `END` is needed.

**Never the process-wide sibling.** `PROCESS_MODE_BACKGROUND_BEGIN` carries an
undocumented hard 32 MiB cap on the process working set, measured making real
programs 250–800× slower. Mozilla investigated it and closed the idea WONTFIX;
Chromium dropped it; both recommended the per-thread flag instead. Because `bgrt`
classifies threads and not processes, the dangerous variant is not merely avoided
— it is unreachable by design. This distinction is the whole reason the Windows
half looked riskier than it was: nearly every horror story about "Windows
background mode" is about the process API.

**Memory priority is deliberately undone.** Background mode also drops the thread
to `MEMORY_PRIORITY_VERY_LOW`, so its pages are trimmed first. That is a latency
hazard rather than an energy win — trimmed pages fault back in, costing the very
disk I/O the class is trying to avoid — so `bgrt` resets it to normal right
afterwards. Chromium does the same.

**Unmeasured, and labelled as such.** CI executes the behavioural tests on
`windows-latest` (the mode is entered, memory priority is lowered by it and then
restored, re-application is idempotent), but nobody has measured *throughput*
under contention on real Windows hardware. Same standing as the hybrid-Linux
pinning path: implemented, behaviour-tested, performance unverified.

#### Why one knob and not two

`qos()` covers both axes; there is no separate `io_class()`. The reasons, in
order of weight:

1. **Two knobs cannot be honoured on two of three platforms.** Windows has no
   documented thread-scope I/O-priority API — the only documented path bundles
   CPU, I/O, and memory. (`NtSetInformationThread(ThreadIoPriority)` is
   undocumented, so out of bounds for this crate.) macOS bundles them too. So
   `qos(Background).io(Default)` and its inverse would be Linux-only truths. **An
   API that cannot honour its own combinations is worse than a coarser one that
   always does what it says.**
2. **macOS already behaved this way**, so the bundled semantics are documentation
   catching up with shipped behaviour rather than a new invention. Had macOS
   treated the axes independently, a split API would have been defensible; because
   it bundles them, a split API would have been contradicted by the reference
   platform on day one.
3. **It keeps the builder trio intact** — no fourth knob across `RuntimeBuilder`,
   `RayonBuilder`, and `ThreadBuilder`. Windows reinforces this after the fact:
   its I/O lever is all-or-nothing, so even `Utility` cannot get quiet I/O
   without also surrendering CPU priority. `Utility` therefore gets no I/O
   reduction there — a coverage gap that is honest, where a split API would have
   been a lie.
4. **The escape hatch stays additive.** If split control is ever genuinely needed,
   `io_class(Option<QosClass>)` can be added as an *override* documented "no
   effect on Windows" — exactly the mould of `pin_efficiency_cores` and
   `clamp_frequency`, both already platform-specific opt-ins. Nothing here
   forecloses it.

### GPU — probably never, but here's what would change that

Not on the roadmap, and it would take a real shift in the platforms to get there.
Five reasons, each sufficient on its own **as things stand today**:

1. **There is no OS-level per-thread GPU QoS on any target platform.** The CPU
   design rests on one primitive that macOS, Windows, and Linux all expose with
   the same meaning. No such primitive exists for GPUs. What exists is per-API
   and mutually incompatible: Vulkan `VK_KHR_global_priority`, CUDA stream
   priorities, D3D12 command-queue priority, and Metal (which has no queue
   priority API at all). There is no common concept to wrap.
2. **Those APIs arbitrate contention; they are not energy levers.** Lowering a
   GPU queue's priority makes your work *wait*. It does not downclock the GPU,
   does not move work to lower-power units, and does not reduce power draw. A GPU
   idling at high clocks while your deprioritized work waits can burn **more**
   energy for the same result — the precise opposite of this library's goal.
3. **On Windows there is no low tier to ask for.** D3D12 offers `NORMAL`, `HIGH`,
   and `GLOBAL_REALTIME`. There is nothing *below* normal, so the central
   operation — "ask for less" — has no expression.
4. **The threading model doesn't transfer.** `bgrt` classifies a thread once at
   start. GPU work is not owned by a thread; it is submitted to a queue owned by
   a device context, and the submitting thread's class says nothing about how the
   work executes. There is no thread to classify.
5. **For AI workloads specifically, the real levers are a different kind of
   thing.** Energy is saved by choosing the low-power compute unit (the Apple
   Neural Engine via CoreML's `MLComputeUnits`, an NPU via DirectML, integrated
   over discrete), by reducing batch size and concurrency, or by quantizing.
   Those are model- and framework-level decisions. A thread-QoS crate is not
   positioned to make any of them, and pretending otherwise would ship a knob
   that looks like it saves energy without doing so.

GPU energy obviously matters. The point is that today the lever is not a
scheduling hint, so it does not belong in a scheduling-hint library.

**What would change this.** Every objection above is contingent — four of the
five describe what the platforms currently expose, not a principle. Concretely,
reopen the question if any of these happen:

- **An OS ships a per-thread or per-context GPU energy QoS** with the property
  that makes the CPU design work: lowering it is unprivileged, it is set once,
  and it means "use less power", not "go later in the queue". A Windows EcoQoS
  for GPU contexts, or a Darwin QoS class that propagates to Metal submissions,
  would be the shape to watch for.
- **The graphics APIs converge on an eco tier that actually affects power** —
  clocks, or placement onto lower-power units — rather than only arbitrating
  contention. Vulkan's `VK_KHR_global_priority` is the nearest existing thing and
  is explicitly *not* that; if a successor were, the calculus changes.
- **Inference runtimes expose a portable low-power mode.** CoreML's
  `MLComputeUnits`, DirectML device selection, and CUDA's knobs all express
  something like "prefer the efficient unit", but with no common vocabulary. If a
  cross-platform abstraction over compute-unit selection emerged, "run this model
  quietly" would become expressible — though even then it may belong in an
  inference wrapper rather than here.
- **GPU work acquires a thread-like owner.** If a runtime lets a submission
  inherit the classification of the thread that queued it, the existing model
  would extend naturally instead of needing a parallel one.

Until at least one of those is true, adding a GPU knob would mean shipping
something that looks like an energy control and is not one. That is the actual
objection, and it is the thing to re-test — not the conclusion.

## Findings worth remembering

- **Linux on homogeneous CPUs: `nice(19)` is a null result without contention.**
  On a homogeneous CPU (no `cpu_capacity` sysfs, e.g. Threadripper, Xeon), `nice`
  only deprioritizes when other threads compete for the same core. A single active
  thread at nice 19 gets full CPU time and full clock speed — throughput, frequency,
  and RAPL energy are indistinguishable from nice 0. Meaningful Linux results require
  a heterogeneous (P+E) CPU (Alder Lake, Raptor Lake) — where E-core affinity via
  `sched_setaffinity` is the real lever — or a CPU-loaded machine.
- **`uclamp` is the homogeneous-CPU frequency lever.** Where `nice` gives the
  cpufreq governor no input, the opt-in `clamp_frequency` (`sched_setattr` with
  `SCHED_FLAG_UTIL_CLAMP_MAX`) caps a `Background` thread's `util_max` (~20%), so
  `schedutil` selects a lower OPP even at 100% busy. Caveats: needs the
  `schedutil` governor (or `intel_pstate=passive`) — fixed governors and HWP
  bypass the util signal; needs kernel ≥ 5.8 for `SCHED_FLAG_KEEP_ALL`; and the
  effective cap is bounded by `/proc/sys/kernel/sched_util_clamp_max`. Lowering
  one's own `util_max` is unprivileged. It biases frequency, not CPU-time share,
  so it composes with `nice` rather than replacing it. **Run-verified on an Intel
  i7-2720QM (Sandy Bridge, homogeneous) with `schedutil`:** background mean clock
  840 MHz vs 3192 (≈3.8× lower), ≈3.2× less package energy over a fixed 10 s
  window, at ≈26% throughput.
- **Frequency-clamping is a stay-cool lever, not a per-work efficiency win (on old
  silicon).** In the i7-2720QM run above, dividing energy by work shows background
  spending *slightly more* per work-unit (~88 vs ~74 µJ): at low clocks, fixed and
  leakage power dominate, so "race to idle" is marginally more efficient for a
  fixed batch. The win is lower *instantaneous* power (cooler, quieter, doesn't
  steal thermal/power budget from foreground work), not a smaller battery bill per
  unit of work. This differs from macOS efficiency-core *placement*, which does cut
  energy ~4× per unit work — placement and frequency are distinct levers with
  distinct economics.
- **Linux RAPL is whole-package on workstation/server CPUs.** On a 16-core
  Threadripper, `energy_uj` reflects the entire package (all cores + memory
  controller + I/O die). Per-thread power attribution is not possible: variance
  between executors (<3%) is measurement noise, not a real signal.
- **Classification does not follow threads spawned by a classified thread —
  except on Linux.** Measured, and the platforms invert their usual roles:
  Linux inherits (`nice`, I/O priority, affinity and `uclamp` live in
  `task_struct` and are copied by `clone()`), while macOS and Windows do not — a
  child of a `QOS_CLASS_BACKGROUND` (0x09) thread reports `QOS_CLASS_DEFAULT`
  (0x15), and Windows starts every thread at `THREAD_PRIORITY_NORMAL`.
  - **This bounds what the crate can promise.** A `QosClass` covers the thread
    `bgrt` created, not the work graph beneath it. Handing a `Background` thread
    to a library that manages its own pool — RocksDB's compaction and flush
    threads being the motivating case — leaves that pool at full priority on two
    of three platforms, silently.
  - **It cannot be repaired from outside.** `pthread_set_qos_class_self_np` and
    `THREAD_MODE_BACKGROUND_BEGIN` both act only on the *calling* thread, so even
    enumerating a library's threads would not help. Only Linux can target another
    task by tid. This is why the two-runtime pattern and the builders' thread
    hooks are the load-bearing part of the design: classification has to happen
    *at thread creation*, by whoever creates the thread.
  - The honest guidance is therefore: prefer libraries that accept a thread
    factory or run on a `bgrt` runtime/pool; otherwise isolate the work in its
    own process and classify the process. Recorded in the README with the full
    table, and asserted per-platform in CI so the table stays true.
- **On macOS you cannot have both an explicit I/O policy and a QoS class.**
  Setting a thread-scope disk policy with `setiopolicy_np` **permanently opts the
  thread out of QoS**. Measured: `pthread_get_qos_class_np` drops from
  `QOS_CLASS_BACKGROUND` (0x9) to `QOS_CLASS_UNSPECIFIED` (0x0) the moment the
  I/O call lands, and re-applying the QoS class afterwards does *not* restore it
  — neither ordering yields both.
  - This was found by trying it. The motivation was good: `getiopolicy_np`
    reports only a thread's *explicit override*, so a background-QoS thread reads
    `IOPOL_DEFAULT` and the disk half of the class is not directly assertable on
    macOS — the one platform where a test could run on the author's own hardware.
    Setting it outright would have fixed that, mirroring why Linux calls
    `ioprio_set` rather than trusting the nice-derived priority.
  - **The price is the entire feature.** Losing the QoS class means losing E-core
    confinement, the DVFS bias, and the ~12× power result that is this crate's
    headline evidence — in exchange for a readable I/O field. Not a trade worth
    making, so the backend deliberately never calls `setiopolicy_np`.
  - Consequence: macOS's I/O coverage rests on Apple's documentation and is
    *structurally* unassertable, a weaker footing than Linux (`ioprio_get`) or
    Windows (memory-priority side effect). Recorded rather than papered over, and
    guarded by a regression test that asserts the QoS class survives
    classification and the I/O override stays unset.
  - It also explains *why* Darwin bundles the two axes, which is the observation
    the whole single-`QosClass` API rests on: the QoS class **is** the I/O
    mechanism there, not merely correlated with it.
- **`Background` throttles disk I/O on macOS for free — which is what set the
  I/O design.** `QOS_CLASS_BACKGROUND` is not purely a CPU hint: Darwin applies
  I/O throttling to threads in that class, in the same call. Discovering that is
  what turned "should I/O be a second knob?" into "it already isn't one on the
  reference platform". Linux now matches explicitly via `ioprio_set`; Windows is
  the remaining gap. Before that work, the same file-heavy background task was
  measurably quieter on macOS than elsewhere through a mechanism `bgrt` never
  asked for — an undocumented asymmetry, now closed on two of three platforms and
  documented on the third.
- **macOS QoS promotion / priority inversion.** A higher-QoS thread that
  synchronously `join`s (or otherwise blocks on) a background thread *promotes it
  off the efficiency cores*. An async `await` on a background runtime does not.
  Practical effect: fire-and-forget background threads stay quiet, but blocking a
  foreground thread on one can speed it up. (The harness matches the waiter's QoS
  to measure the executor, not the join.)
- **tokio's `on_thread_start` covers the blocking pool**, so `spawn_blocking`
  work is classified too — verified by test. No `spawn_blocking`-side workaround.
  **But only on the multi-thread scheduler:** on a current-thread runtime the
  same hook fires *only* for blocking-pool threads, never for the thread driving
  the async tasks. Hence the owned driver thread described above.
- **Efficiency-core pinning on Linux is unverified on real hardware.** The
  selection logic (`topology::select_efficiency_cores`) is unit-tested against
  hybrid and three-tier capacity layouts, but the sysfs `cpu_capacity` read and
  `sched_setaffinity` have never executed against a non-empty core set — every
  available Linux machine is homogeneous, where the feature correctly does
  nothing, which is precisely the result that cannot distinguish working code
  from silently broken code. Recorded as a caveat in the README rather than
  papered over; treat it as untested code until run on Alder/Raptor/Meteor Lake.
- **Frequency can be biased, not guaranteed.** Keeping work off P-cores is the
  effective lever; E-cores can still clock up, but at far lower thermal cost.
