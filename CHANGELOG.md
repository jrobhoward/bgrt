# Changelog

All notable changes to **bgrt** are recorded here. This file tracks the
project's running state; the phased plan lives in [`docs/ROADMAP.md`](docs/ROADMAP.md).

Format loosely follows [Keep a Changelog](https://keepachangelog.com/);
the project is pre-1.0 and not yet released.

## [Unreleased]

### Efficiency-core detection was blind to every Intel hybrid CPU — 2026-07-26

- **Fixed: `topology::efficiency_cores()` read only sysfs `cpu_capacity`, which
  appears not to exist on x86.** That attribute arrived in 2016 as an arm/arm64
  feature (`arch_topology.c`, `CONFIG_GENERIC_ARCH_TOPOLOGY`); Intel explicitly
  proposed a *different* interface in 2020 rather than adopting it; and while
  `intel_pstate` has fed asymmetric capacity to the scheduler since 2024, it does
  so through an x86-specific per-CPU variable, not the generic topology code that
  publishes the sysfs file. So `pin_efficiency_cores(true)` was almost certainly
  a **silent no-op on Alder Lake and every later hybrid part** — indistinguishable
  from the correct no-op on a homogeneous CPU, which is precisely the failure this
  project flags as the dangerous one.
- **Second source added: the hybrid perf PMUs.** The kernel registers `cpu_core`
  and `cpu_atom` PMUs on hybrid x86, each exposing a `cpus` file; on an
  i9-12900K, `cpu_atom/cpus` reads `16-23`. Unprivileged, absent on non-hybrid
  machines (so the homogeneous case still yields empty), and *authoritative* —
  `cpu_atom` is the E-core list outright, needing no minimum-wins inference.
  `efficiency_cores()` now tries `cpu_capacity` first, then the PMU.
- **New pure `parse_cpulist`**, covering ranges, comma lists, and mixed forms,
  with malformed fields skipped rather than fatal. Nine tests, including the
  published 12900K values, a reversed range, and a `0-99999999` range that must
  not allocate unboundedly. Being pure is what makes the decision testable on
  macOS, where the sysfs read can never run.
- **AMD hybrid (Zen 4c / Zen 5c) is deliberately not detected.** The dense cores
  share a PMU with the classic ones, so there is no `cpu_atom` equivalent, and
  the kernel exposes core type (CPUID `0x80000026`, `X86_FEATURE_AMD_HTR_CORES`)
  only via root-only debugfs. Returning empty disables pinning rather than
  pinning to a wrong set. **Not** inferred from `cpufreq/cpuinfo_max_freq`:
  per-core boost binning makes homogeneous CPUs report differing maximums there,
  so minimum-wins over that field would mistake a binned Threadripper for a
  hybrid and confine background work to one arbitrary core.
- **Still unverified on real hybrid silicon**, and labelled as such. The parser
  is tested off-hardware; the sysfs read and `sched_setaffinity` against a
  non-empty set have still never executed. The README and `ROADMAP.md` rows,
  which previously read as purely hardware-blocked, now say plainly that this was
  partly a software gap.

### Pre-0.9 API commitments and a documentation reconciliation — 2026-07-26

- **`QosClass` is now `#[non_exhaustive]`.** Matching it from outside `bgrt`
  needs a `_` arm. The three classes cover what all three platforms express in
  common, and the crate's promise to only ever *lower* demands rules out the
  obvious additions (macOS's `USER_INITIATED`/`USER_INTERACTIVE` both raise
  priority) — so a fourth class is unlikely, but this keeps it a minor release
  rather than a major one. No internal changes needed: every `match class` site
  is inside the defining crate, and `bgrt-bench` only constructs values.
- **MSRV policy stated:** an increase is a **minor** bump, never a patch. Written
  into the crate-level rustdoc and the README's new *Stability* section, next to
  the existing `telemetry` exemption and wrapped-dependency policy.
- **Rustdoc refinements.** `doc(cfg(...))` feature badges behind `--cfg docsrs`
  (docs.rs builds `--all-features`, so gated items were rendering as though
  always available); a crate-level *Limitations* section, since a docs.rs reader
  never sees the README; the `current_thread` drop-deadlock hazard documented
  (dropping such a `Runtime` from inside its own blocking pool joins the driver
  thread, where plain Tokio panics); `#[must_use]` on the remaining getters.
- **Documentation reconciled against the code.** Scope changed twice during
  development (I/O added, GPU ruled out) and the docs recorded it inconsistently.
  Fixed: `CLAUDE.md` claimed **there was no CI** while a 3-OS matrix has been
  running since Phase 7; `DESIGN.md`'s QoS table omitted I/O and `uclamp`
  entirely, its telemetry table still said Windows E/P was a TODO, and its I/O
  section called Windows an open gap 16 lines before calling it closed;
  `ROADMAP.md` documented a `Backend` trait, a `Telemetry` trait, a
  `bgrt::Builder`, `proptest`, and macOS `hw.perflevel*` detection — none of
  which exist. README's *Scope: what `bgrt` is not* opened by explaining what
  `bgrt` **is** (I/O priority, in scope); it is now just *Scope*, with the
  in/out split made explicit.
- **Deduplicated.** The QoS mapping table had five copies and had drifted in
  three; the README's is now canonical and `docs/` links to it. Measured results
  moved to `docs/BENCHMARKS.md`. `ROADMAP.md` dropped the pre-implementation API,
  architecture, and conventions sections that duplicated `CLAUDE.md` and had gone
  stale (325 → 170 lines); README 487 → 340.
- **Stated plainly that the benchmarks measure CPU only.** The crate's second
  headline bullet is "CPU *and* disk", but `bgrt-bench`'s workload is a CPU-bound
  loop — the disk half is behaviour-tested per platform and never benchmarked.
  Noted in the README, `DESIGN.md`, and `BENCHMARKS.md`, and tracked in
  `ROADMAP.md` as the one item blocking 0.9.
- **0.9 release plan recorded** in `ROADMAP.md`: repository public + crates.io
  publish, then **1.0 sixty days later** absent an issue or request calling for a
  breaking change. No version bump in this change.
- **README badges** — CI status, crates.io, docs.rs, MSRV, and license — plus the
  **License section** the crate was missing. The crates.io and docs.rs badges
  read "not found" until publication, so the release steps are now explicitly
  ordered: publish first, make the repo public second.

### Windows memory-priority restore assumed a normal baseline — 2026-07-25
- **Fixed: `apply(Background)` wrote `MEMORY_PRIORITY_NORMAL` unconditionally
  after leaving background mode**, on the assumption that normal is where every
  thread starts. It is the system *default*, but a process can lower its own with
  `SetProcessInformation(ProcessMemoryPriority)` and new threads inherit that. On
  such a process `bgrt` would have *raised* a `Background` thread's memory
  priority above the policy its process chose — the one thing a classifier that
  only ever lowers must not do. `apply` now samples the thread's memory priority
  before entering the mode and puts that value back; the read is a new
  `memory_priority()` helper shared with the tests.
- **Found by CI, not by reasoning.** GitHub Actions `windows-latest` runners start
  threads at `MEMORY_PRIORITY_LOW` (2), which tripped the
  `assert_eq!(before, MEMORY_PRIORITY_NORMAL, "unexpected starting state")` guard
  in `set_background_mode____entered____lowers_memory_priority_below_normal`. The
  guard was doing its job: the surprising starting state was real, and the
  production code was the thing that was wrong.
- **Tests now assert against the observed baseline** rather than the constant —
  `..._lowers_memory_priority` (renamed, no longer "below_normal") checks the
  value dropped from wherever it started, with the already-at-`VERY_LOW` floor
  handled, and the restore/idempotence tests check it came back to that same
  starting value. Comparing to a constant could not have caught this bug: a
  backend that hard-codes normal satisfies "equals normal" by construction.
- Rationale recorded in `docs/DESIGN.md` → *Windows*; README and module docs
  updated to say "puts memory priority back" rather than "back to normal".

### Semver policy for wrapped dependencies, stated — 2026-07-25
- **`Error::ThreadPool`'s doc claimed the boxing kept rayon out of `bgrt`'s
  public API. It never did.** `RayonPool` derefs to `rayon::ThreadPool`, and
  `Runtime::spawn`/`handle` return Tokio's `JoinHandle`/`Handle` — a wrapper
  crate cannot hide the thing it wraps without taking the ecosystem with it. The
  comment now states the boxing's real purpose (keeping *this enum's* shape
  stable when rayon reshapes its error type) and points at the policy.
- **New crate-level *Semver and wrapped dependencies* section**, in the same
  spirit as the existing `telemetry` exemption: a major release of Tokio or rayon
  is a major release of `bgrt`, and the majors cannot be mixed.
- **`pub use tokio;` / `pub use rayon;`** (each behind its own feature) so callers
  can name the exact versions `bgrt` resolved instead of declaring a dependency
  that might resolve differently. Two compile-level tests assert each re-export
  denotes the same type the API hands back — a mismatch is precisely the bug the
  re-export exists to prevent.

### Windows telemetry sized its buffer from the wrong processor count — 2026-07-25
- **Fixed: `available_parallelism()` reports threads available to the
  *process*** (affinity masks, job-object limits), while
  `CallNtPowerInformation(ProcessorInformation)` fills one record per processor
  in the *machine* and rejects anything shorter with `STATUS_BUFFER_TOO_SMALL`.
  Any process under a restricted affinity mask therefore reported no frequency at
  all — silently degrading the measurement the harness exists to take. Now uses
  `GetActiveProcessorCount(ALL_PROCESSOR_GROUPS)`, with the buffer-size multiply
  made checked.
- The group-relative index caveat (`GetCurrentProcessorNumber` vs. a machine-wide
  buffer) is now documented here too, pointing at the matching note in
  `topology::efficiency_cores`. Unchanged behaviour; it was simply undocumented.
- Unverified on Windows hardware, like the rest of the Windows backend.

### `#[must_use]` on the builder methods — 2026-07-25
- `RuntimeBuilder::new().qos(QosClass::Background);` compiled, did nothing, and
  warned about nothing. All 19 chainable methods across the three builders (16
  setters + 3 `new()`) are now `#[must_use]`. `build()` needs no attribute — it
  returns `Result`, which already carries one.
- **A `const fn` pass was considered and rejected**, having looked at the actual
  candidates: none are reachable from a const context (`new()` goes through
  `Default::default()`, and no `Runtime`/`RayonPool`/`Aggregate` can be
  const-constructed), so it buys nothing, while `const` on a public fn is a
  forward-compatibility promise that a later `tracing` call or non-const
  validation would break. Several candidates are also `cfg`-gated no-op twins,
  where accepting the lint would make the API `const` on one platform and not
  another.

### `Runtime::block_on` documents its panics — 2026-07-25
- Both scheduler modes pass Tokio's panic-on-nested-`block_on` through unchanged;
  a `#![warn(missing_docs)]` crate that documents `spawn_thread`'s panic should
  document this one too.

### Linux `Default` no longer fails in an already-niced process — 2026-07-25
- **Fixed: `apply(QosClass::Default)` returned an error whenever the calling
  thread's nice value was already above 0.** `setpriority` is refused with
  `EACCES` when asked to *lower* a nice value, because `RLIMIT_NICE` defaults to
  0 — the one-way `nice` this project already documents as the reason
  `RuntimeBuilder::current_thread` owns its driver thread. The `Default` mapping
  called it unconditionally anyway, so two ordinary situations produced a failure
  the caller could not act on:
  - a process started already niced (`nice -n 10 …`, systemd `Nice=`, a container
    with a nice offset), where a `Default`-class runtime errored on *every*
    worker and logged a warning per thread — `examples/mixed_runtimes.rs` is
    exactly that shape; and
  - applying `Default` to a thread previously classified `Background`, which
    cannot be undone at all.
- **Now a graceful no-op:** `EACCES`/`EPERM` from `setpriority` degrade to a
  `debug!` and `Ok`, matching how `ioprio` and `uclamp` already handle a kernel
  declining a hint. The thread keeps the niceness it had. Only those two errnos
  are absorbed — with `who == 0` they can only mean "you asked to raise
  priority", so a genuine failure still surfaces.
- **This was already the crate's own reasoning, applied inconsistently.**
  `backend/ioprio.rs` deliberately writes nothing for `Default`, on the grounds
  that doing so "would be the one place this backend raised a priority instead of
  lowering it". The same argument governs `setpriority` and had not been carried
  across.
- **Three new Linux tests** covering reclassification in both directions:
  `Default` and `Utility` after `Background` degrade to a no-op and hold nice 19;
  `Background` after `Utility` still lowers, since lowering is always permitted.
  The pre-existing `Default` test only ever ran on a fresh thread already at nice
  0, where the call trivially succeeds — which is why this went unnoticed.
- **Documented** on `QosClass` (new "classification is not reversible on Linux"
  section), `apply`, the Linux backend module, and the README limitation that
  previously stated the constraint without saying what `bgrt` does about it. The
  `apply` doc claim that it "only ever *lowers* … so it never requires elevated
  privileges" was true of intent but false of behaviour; it now says `bgrt`
  declines rather than demands.
- Known consequence, recorded rather than papered over: best-effort I/O levels
  carry no such privilege check, so reclassifying `Background` → `Utility` moves
  the I/O half to level 6 while niceness stays at 19. Classify once at thread
  creation.

### Rayon pool shares `thread::classify` instead of copying it — 2026-07-25
- **`RayonBuilder`'s `start_handler` reimplemented the qos → pin → clamp
  sequence by hand**, against the stated invariant that it is "one function,
  three call sites". Behaviour is unchanged today; the risk was that a knob added
  to `classify` would reach `RuntimeBuilder` and `ThreadBuilder` and silently
  skip rayon pools — the exact failure the "three builders are deliberately
  parallel" rule exists to prevent. Now calls `classify` like the other two.

### Thread-classification inheritance: measured and documented — 2026-07-25
- **A `QosClass` does not follow threads spawned by a classified thread, except
  on Linux.** Previously undocumented, and the failure is silent. Measured:

  | | Child threads inherit? |
  |---|---|
  | Linux | ✅ yes — `nice`/ioprio/affinity/`uclamp` are copied by `clone()` |
  | macOS | ❌ no — child of a `QOS_CLASS_BACKGROUND` (0x09) thread reports `QOS_CLASS_DEFAULT` (0x15) |
  | Windows | ❌ no — "all threads initially start at `THREAD_PRIORITY_NORMAL`" |

- **This bounds what the crate can promise**, so it is now stated wherever a user
  would form the wrong expectation: a README limitation section with the table
  and workarounds, plus notes on `QosClass` and the `thread` module. The
  motivating case is handing a `Background` thread to a library that manages its
  own pool — RocksDB's compaction and flush threads — which stays at full
  priority on two of three platforms.
- **It cannot be corrected after the fact** on macOS or Windows: both
  `pthread_set_qos_class_self_np` and `THREAD_MODE_BACKGROUND_BEGIN` act only on
  the *calling* thread, so enumerating a library's threads would not help. Only
  Linux can target another task by tid. That is why classification has to happen
  at thread creation — which is what the builders' thread hooks are for.
- **Three new tests, one per platform**, asserting each OS's actual behaviour
  rather than leaving the table resting on documentation. Each carries a failure
  message pointing at the README table, so an OS behaviour change surfaces as
  "the docs are now wrong" rather than a puzzling assertion.
- `test_support` gained `QOS_CLASS_DEFAULT` and a shared
  `current_thread_priority()` for Windows (previously duplicated inside
  `windows_tests.rs`).

### I/O test coverage, and a negative result on macOS — 2026-07-25
- **Tried making macOS I/O explicit; it does not work, and now we know why.**
  The disk half of a class was assertable on Linux (`ioprio_get`) and indirectly
  on Windows (memory-priority side effect), but not on macOS: `getiopolicy_np`
  reports only a thread's *explicit override*, so a `QOS_CLASS_BACKGROUND` thread
  reads `IOPOL_DEFAULT` while Darwin throttles it anyway. Calling
  `setiopolicy_np` would have fixed that — the same reasoning that has Linux call
  `ioprio_set` instead of trusting the nice-derived priority.
  - **It costs the QoS class.** Measured: an explicit thread-scope I/O policy
    drops `pthread_get_qos_class_np` from `QOS_CLASS_BACKGROUND` (0x9) to
    `QOS_CLASS_UNSPECIFIED` (0x0), and re-applying the class afterwards does not
    restore it. Neither ordering yields both. That would trade efficiency-core
    confinement and the ~12× power result for a readable field — so the backend
    deliberately never makes the call, and the implementation was reverted.
  - The existing macOS QoS tests caught this on the first run, which is precisely
    why they assert the class rather than just that `apply` returns `Ok`.
  - **Now guarded by a regression test** asserting both that the QoS class
    survives classification and that the I/O override stays unset.
  - It also explains *why* Darwin bundles CPU and I/O, which is the observation
    the single-`QosClass` API rests on: the QoS class **is** the I/O mechanism
    there, not merely correlated with it.
  - **Honest consequence:** macOS I/O coverage is structurally unassertable and
    rests on Apple's documentation — a weaker footing than the other two
    platforms. Documented in the README, `QosClass`, and DESIGN findings.
- **Closed the I/O-vs-CPU test asymmetry.** `current_nice` was asserted at four
  levels (backend `apply`, `spawn_thread`, runtime worker, rayon pool);
  `current_ioprio` at one. Four new Linux tests bring the disk half level with
  the CPU half: `spawn_thread`, rayon pool threads, runtime workers, and the
  **blocking pool** — the last mattering most, since `spawn_blocking` is where
  disk-heavy work actually lands. Previously we proved the syscall worked but
  never that it reached the threads users touch.

### Windows block-I/O priority — the last platform gap — 2026-07-25
- **`QosClass::Background` now lowers block-I/O priority on Windows**, via
  `SetThreadPriority(THREAD_MODE_BACKGROUND_BEGIN)` — the only documented
  per-thread I/O lever there. All three platforms now cover CPU *and* disk.
- **Unblocked by research, not hardware.** The three questions that had deferred
  this were all answerable from documentation and from what Chromium ships:
  - *Composes with EcoQoS?* **Yes.** Chromium applies background mode and
    `THREAD_POWER_THROTTLING_EXECUTION_SPEED` to the same threads; they are
    independent mechanisms, so the existing EcoQoS call is untouched.
  - *Starvation?* Microsoft documents that such a thread "may not be scheduled
    promptly, but it will never be starved" — satisfying the hard never-starve
    requirement. Recorded with its hedge: that is weaker than Linux's
    proportional share, so poor throughput under sustained foreground load is
    expected and intended.
  - *Begin/end pairing?* Windows reports "already in that state" as an **error**
    (`ERROR_THREAD_MODE_ALREADY_BACKGROUND` / `..._NOT_BACKGROUND`). Both are now
    swallowed, which is what keeps `apply` idempotent — it can legitimately run
    more than once on one thread.
- **`PROCESS_MODE_BACKGROUND_BEGIN` is never used, and that is the crux.** The
  process-wide sibling carries an undocumented hard 32 MiB working-set cap,
  measured making real programs 250–800× slower; Mozilla investigated it and
  closed the idea WONTFIX, Chromium dropped it, and both recommended the
  per-thread flag used here. Nearly every horror story about "Windows background
  mode" is about the process API. `bgrt` classifies threads, not processes, so
  the dangerous variant is unreachable by design.
- **Memory priority is deliberately restored.** Background mode also drops the
  thread to `MEMORY_PRIORITY_VERY_LOW`, so its pages are trimmed first — a
  latency hazard rather than an energy win, since trimmed pages fault back in at
  the cost of the very disk I/O this class avoids. Reset to normal immediately
  after; Chromium does the same.
- **`Utility` gets no I/O reduction on Windows, on purpose.** The mode is
  all-or-nothing and would drag CPU priority down with it, which is exactly what
  separates `Utility` from `Background`. An honest coverage gap rather than a
  misleading knob — and after-the-fact support for the single-`QosClass` design,
  since a split CPU/I/O API could not have been honoured here either.
- **Tests (4 new, CI-only):** the raw mode measurably lowers memory priority —
  which stands in for the un-readable I/O priority and proves the mode takes
  effect at all — `apply` then restores it, re-applying `Background` is
  idempotent, and `Utility`/`Default` on a fresh thread tolerate not having been
  in background mode. The first of these was added specifically because
  asserting "memory priority is normal" alone would pass just as happily if the
  mode had never been entered.
- **Still unmeasured:** throughput under contention on real Windows hardware.
  Behaviour is tested in CI; performance is reasoned, not observed. Same standing
  as the hybrid-Linux pinning path, and labelled the same way.

### A `QosClass` now covers block I/O, not just CPU — 2026-07-25
- **`QosClass` is a *resource* class.** Linux gained `backend/ioprio.rs`:
  `ioprio_set` to best-effort level **7** for `Background`, **6** for `Utility`,
  and untouched for `Default`. macOS already did this for free
  (`QOS_CLASS_BACKGROUND` implies disk-I/O throttling). Supersedes the
  "deferred to a possible 1.1" decision recorded below, for two reasons found
  while designing it.
  - **The feature was already ~2/3 present.** macOS bundles I/O into the QoS
    class, and on Linux the kernel derives a best-effort level from niceness —
    `(nice + 20) / 5`, mapping `nice(19)`/`nice(10)` onto exactly levels 7 and 6,
    the same values we would have picked by hand. So this was one platform's gap
    plus a documentation commitment, not a three-platform feature.
  - **Deferring was the riskier option.** This is a *semantic* change, not an
    additive one. Shipping 1.0 as "`QosClass` is a CPU knob" and then redefining
    it in 1.1 would be a silent behaviour change — technically not a semver
    break, which makes it worse, not better.
- **Best-effort, never `IOPRIO_CLASS_IDLE`.** Idle-class I/O only gets the disk
  when nothing else wants it — the I/O equivalent of `SCHED_IDLE`, which this
  project rejects for CPU. Best-effort 7 is the weighted-fair choice, exactly
  parallel to `nice(19)`, and preserves the never-starve guarantee on the disk
  axis too.
- **`Default` deliberately sets nothing.** An unset I/O priority already tracks
  the thread's niceness, which `Default` has just set to 0; writing a value would
  be the one place this backend *raised* a priority instead of lowering it.
- **One knob, not two — and macOS is why.** There is no `io_class()` to go with
  `qos()`. Windows has no documented thread-scope I/O-priority API (only
  `THREAD_MODE_BACKGROUND_BEGIN`, which bundles CPU + I/O + memory), and macOS
  bundles them as well, so `qos(Background).io(Default)` and its inverse would be
  Linux-only truths. An API that cannot honour its own combinations is worse than
  a coarser one that always means what it says. A split `io_class` override stays
  available later as a purely additive, platform-gated knob in the mould of
  `pin_efficiency_cores`.
- **Windows I/O is the outstanding gap, deferred on purpose.** Adopting
  `THREAD_MODE_BACKGROUND_BEGIN` would change shipped `Background` *CPU*
  behaviour rather than just adding an axis, and needs measuring on real hardware
  against the never-starve rule first. Documented as not-covered rather than
  quietly assumed. Tracked as Phase 8b.
- **Tests:** 5 portable ones for the mapping and the `IOPRIO_PRIO_VALUE`
  bit-packing (they run on every platform, since the decision is pure), plus 4
  Linux-only ones that read the value back with `ioprio_get` — which reports what
  was *set* regardless of whether the active I/O scheduler honours it, so the
  assertions are deterministic even on a `none`-scheduler CI runner.
- **Honest caveat:** whether the priority *bites* is the I/O scheduler's
  business. BFQ honours it fully, `mq-deadline` since 5.18, and `none` — a common
  NVMe default — ignores it entirely. Same shape of "inert on some
  configurations" as the `uclamp` frequency clamp, and documented the same way.
- **Open, unverified:** the `(nice + 20) / 5` derivation itself is from
  documentation, not measured here. It does not affect correctness — the explicit
  `ioprio_set` makes the result the same either way — only whether this change is
  a no-op or a real behaviour change on Linux.

### Scope decision: I/O deferred, GPU probably never — 2026-07-25
*(The I/O half was superseded the same day — see above. The GPU half stands.)*
- **`bgrt` is a CPU scheduling-hint library, and now says so.** Recorded in
  `docs/DESIGN.md` (*Scope: CPU now, I/O maybe, GPU probably never*), the README
  (*Scope: what `bgrt` is not*), the non-goals list, and the roadmap's open
  questions — so the "what about X" question is answered once rather than
  re-litigated. Both answers are written as **contingent**, with the conditions
  that should reopen them stated explicitly, rather than as permanent rulings.
- **I/O priority: deferred to a possible 1.1, gated on demand.** It is a genuine
  second axis — all three OSes expose an unprivileged, per-thread,
  set-once-at-thread-start I/O priority (`setiopolicy_np`, `ioprio_set`,
  `SetThreadPriority(THREAD_MODE_BACKGROUND_BEGIN)`), which is exactly the shape
  of the existing CPU knob. Held back because nobody has asked, and because two
  platforms need measurement first: on Linux `ioprio` is inert under the `none`
  scheduler that is a common NVMe default (another `nice`-on-homogeneous-CPU-type
  null result), and on Windows the mechanism also lowers CPU and memory priority,
  so it would change existing `Background` behaviour rather than just add an
  axis. If it lands it folds into `QosClass` instead of becoming a fourth builder
  knob.
- **GPU: probably never, and not on the roadmap.** No OS-level per-thread GPU QoS
  exists on any target platform *today*; the per-API priorities that do exist
  (Vulkan, CUDA, D3D12; Metal has none) arbitrate contention rather than reduce
  energy — a deprioritized GPU idling at high clocks can burn *more* energy for
  the same work; D3D12 has no tier below normal, so "ask for less" has no
  expression; and GPU work belongs to a queue owned by a device context, not to a
  classifiable thread. For AI workloads the real energy levers (compute-unit
  selection, batch size, quantization) are framework-level and outside what a
  thread-QoS crate can reach.
  - **Stated as contingent, not final.** Four of those five objections describe
    what the platforms currently expose rather than a principle, so DESIGN now
    lists what would reopen the question: an OS shipping a per-context GPU energy
    QoS that is unprivileged to lower and set once; graphics APIs growing an eco
    tier that moves clocks or unit placement rather than only queue order;
    inference runtimes converging on a portable low-power mode; or GPU
    submissions inheriting the classification of the thread that queued them. The
    objection to re-test is "a GPU knob here would look like an energy control
    without being one" — not the conclusion drawn from it.
- **Newly documented existing behaviour: `Background` throttles disk I/O on
  macOS, and only there.** `QOS_CLASS_BACKGROUND` implies I/O throttling on
  Darwin, which `bgrt` never asked for and never disclosed; Linux `nice(19)` and
  Windows EcoQoS do not. So a file-heavy background task is quieter on macOS than
  on the other two platforms. Now called out in the README limitations and the
  DESIGN findings — it is also the reason the I/O axis above would *equalize* the
  platforms rather than add something new.

### Windows E/P core classification — 2026-07-25
- **`telemetry::CoreType` is no longer `Unknown` on Windows.** `topology::
  efficiency_cores()` gained a Windows implementation over the CPU Sets API
  (`GetSystemCpuSetInformation` → `SYSTEM_CPU_SET_INFORMATION.EfficiencyClass`),
  closing the last deferral from Phase 4/5. Unprivileged, like everything else
  the library does.
- **Both platforms share one decision function.** Windows' `EfficiencyClass` and
  Linux's sysfs `cpu_capacity` are both "higher is faster" scales, so both now
  feed the existing pure `select_efficiency_cores`: minimum value wins, and an
  all-equal machine reports *homogeneous* (empty) rather than "every core is an
  efficiency core". That reuse is why the logic is already unit-tested on
  hardware that has neither topology.
- **Limited to processor group 0, deliberately.** `LogicalProcessorIndex` is
  group-relative and so is `GetCurrentProcessorNumber`, which telemetry compares
  it against; mixing groups would silently alias CPU 3 of group 0 with CPU 3 of
  group 1. Only >64-logical-processor systems are affected, and hybrid consumer
  CPUs — the entire point of the lookup — are single-group.
- **Detection ≠ pinning.** `pin_efficiency_cores` remains a Linux-only no-op even
  now that Windows detection works: EcoQoS already places work on efficient
  cores, and a hard affinity mask would fight the scheduler rather than help it.
  Knowing which cores are efficient is not a reason to start overriding an OS
  that is already doing the job.
- **The FFI buffer walk is defensive:** size query first (records are
  variable-length, so the count isn't derivable from the CPU count), allocation
  typed as `SYSTEM_CPU_SET_INFORMATION` so it carries the struct's alignment,
  `read_unaligned` anyway since records are only guaranteed `Size` apart, the
  returned length clamped to what was actually allocated, and a bail-out on any
  record too short to advance the cursor.
- **Two new Windows tests, which CI is the only place that runs them:** the CPU
  set read reports each logical processor exactly once with indices dense from 0
  (what makes them comparable with `GetCurrentProcessorNumber`), and the derived
  E-core set is always a *strict* subset of all CPUs — the assertion that catches
  a homogeneous machine leaking through as "everything is an E-core".
  - **Honest limit:** CI runners are homogeneous VMs, so the branch that actually
    labels a core "efficiency" is still only covered by unit tests of the
    selector. Recorded in the README alongside the hybrid-Linux caveat.

### Current-thread runtime, on a thread bgrt owns — 2026-07-25
- **`RuntimeBuilder::current_thread(bool)`** — supersedes the "no current-thread
  runtime" decision recorded below. The objection there was never to the
  *scheduler*; it was to who drives it. So `bgrt` now spawns the driver: one OS
  thread, created through `ThreadBuilder` and therefore classified exactly like
  every other `bgrt` thread, which builds a Tokio current-thread runtime and
  parks in `Runtime::block_on` for the runtime's lifetime. Tasks get
  single-threaded semantics *and* the energy class actually applies.
  - **Naming caveat, documented prominently:** despite following Tokio's
    scheduler naming, tasks do **not** run on the calling thread. That is the
    entire point — reclassifying a thread `bgrt` did not create is unsafe (on
    Linux niceness is a one-way trip for unprivileged threads).
  - `Runtime` gained a private `Inner` enum (`MultiThread` | `Dedicated`) and now
    caches a `Handle`, since in the new mode the runtime itself lives on the
    driver thread. `spawn`/`spawn_blocking` go through the handle in both modes;
    `block_on` dispatches, using `Handle::block_on` for the dedicated case —
    sound only because the driver keeps the I/O and timer drivers running.
  - **Shutdown crosses the thread boundary** via a `tokio::sync::oneshot`
    carrying a `ShutdownMode` (`Wait` | `Timeout` | `Background`), so all three
    Tokio teardown behaviours survive the indirection. `Drop` sends `Wait` and
    joins. Adds tokio's `sync` feature to `bgrt` only.
  - **Build stays fallible:** the driver reports its `Handle` — or the runtime
    build error — back over an `mpsc::sync_channel`, so `build()` never returns a
    dead runtime.
  - **Measured, not assumed:** both modes cost exactly one thread
    (`multi_thread(1)=+1`, `current_thread=+1`), and the process returns to its
    baseline thread count after drop — the driver is joined, not leaked. On macOS
    a task spawned onto the current-thread runtime observes
    `QOS_CLASS_BACKGROUND`, the case a plain Tokio current-thread runtime gets
    wrong; that assertion is now a test, with the `nice 19` equivalent on Linux.
  - Prefer the default `worker_threads(1)` unless single-threaded task semantics
    are wanted: same one thread, no blocked-task-stalls-everything hazard.
- **`thread::classify` is now `pub(crate)`** and used by the runtime's
  `on_thread_start` hooks, so all three builders resolve qos/pin/clamp through
  one function instead of three copies.
- **README: hybrid Linux is called out as unmeasured.** A table now separates
  what is run-verified on Linux (`nice` mapping, `uclamp`, the pure E-core
  *selection* logic) from what has never executed on real P+E silicon (the sysfs
  `cpu_capacity` read, `sched_setaffinity` against a non-empty core set). Every
  Linux measurement in the README is from a homogeneous CPU, where the feature
  correctly does nothing — the one result that cannot distinguish "works" from
  "silently broken". Flagged as untested code, not a measured feature.

### Road to 1.0 — API freeze decisions — 2026-07-25
- **Errors now preserve their cause.** `Error::Backend` became a struct variant
  `{ syscall: &'static str, source: std::io::Error }`; `Error::Runtime` carries
  tokio's `io::Error` directly; `Error::ThreadPool` boxes its cause as
  `Box<dyn Error + Send + Sync>` — deliberately *not* typed as
  `rayon::ThreadPoolBuildError`, so rayon's version is not part of `bgrt`'s
  public API and a rayon major release is not a breaking change here. Every
  construction site already had the structured cause (`io::Error::last_os_error`,
  `GetLastError`, pthread's returned errno) and was discarding it into a string.
  - **New `Error::raw_os_error() -> Option<i32>`**, so callers can distinguish
    "kernel lacks the feature" (`ENOSYS`) from a real failure without parsing
    messages — the case that actually matters for the best-effort `uclamp` and
    affinity paths. Display strings are now short and stable; the OS detail lives
    in the source chain.
- **`telemetry` is documented semver-exempt.** It exists to serve `bgrt-bench`;
  its surface may change in any release, including a patch. The rest of the crate
  carries the usual guarantees.
- **`Runtime::shutdown_timeout` / `shutdown_background`.** Dropping a runtime
  waits for blocking tasks indefinitely, which for a *background* runtime can be
  a very long time — quiet work is slow by design. These bound that wait.
- **No current-thread runtime — and it is now documented why.** *(Superseded the
  same day by `RuntimeBuilder::current_thread`, above — the analysis here still
  holds and is exactly why that mode owns its driver thread.)* Measured: a Tokio
  current-thread runtime fires `on_thread_start` only for blocking-pool threads,
  so async tasks run on the `block_on` caller's thread at its *unmodified* QoS
  (probe: task observed `QOS_CLASS_DEFAULT` while the blocking thread observed
  `QOS_CLASS_BACKGROUND`). Classifying the caller is not an option either: on
  Linux niceness is a one-way trip for unprivileged threads. `worker_threads(1)`
  is the single-quiet-worker configuration and costs exactly one thread (measured:
  `+1`; Tokio drives I/O and timers on the worker, with no extra driver thread).
- **Documented two standing decisions:** blocking-pool threads intentionally share
  the runtime's `QosClass` (CPU-bound work is exactly what lands there), and
  `QosClass::default()` is `Default` while the *builders* default to `Background`
  — the enum's default is its neutral member, whereas constructing a `bgrt`
  builder is already a request for quiet execution.
- **Verified:** `cargo test --workspace`, `--all-features` (44 lib + 9 doctests),
  `--no-default-features`, clippy `-Dwarnings` on host plus the Linux and Windows
  cross-targets, MSRV 1.85.0 `cargo check --all-features`, and `cargo doc` with
  `RUSTDOCFLAGS=-Dwarnings` — all clean.

### CI, licensing, and packaging — 2026-07-25
- **`.github/workflows/ci.yml`** — first CI for the project. `test` job matrixed
  over macOS/Linux/Windows (clippy `-Dwarnings`, `cargo test --workspace`, four
  feature permutations each), `msrv` job matrixed on 1.85.0, and a `lint` job
  (`cargo fmt --check`, `cargo doc` with `RUSTDOCFLAGS=-Dwarnings`). This is what
  finally *executes* the Windows and Linux backends, which until now were only
  ever cross-compiled. Uses rustup plus first-party actions only; `bash` forced
  on all three runners. Clippy's `-Dwarnings` is passed after `--` so it applies
  to workspace members and not dependencies, keeping upstream warnings from
  breaking the build.
- **`LICENSE-MIT` + `LICENSE-APACHE`** added, matching the long-declared
  `MIT OR Apache-2.0`. `cargo package --list` showed workspace-root licenses were
  **not** included in the publishable tarball; both are symlinked into
  `crates/bgrt/` and verified present with dereferenced content.
- **Packaging:** `[package.metadata.docs.rs] all-features = true` (without it the
  `rayon` and `telemetry` APIs would be absent from docs.rs entirely) and a
  `readme` field.
- **Windows test gap closed:** added a `Background` → `Default` transition test
  covering the EcoQoS *clear* path (`set_eco_qos(false)`); the pre-existing tests
  each start on a fresh thread and so only ever *set* it. EcoQoS state has no
  documented read-back, so the throttling bit remains asserted indirectly through
  thread priority.

### Opt-in Linux `uclamp` frequency clamp — 2026-06-13
- **New `clamp_frequency(bool)` builder option** on `RuntimeBuilder`,
  `RayonBuilder`, and `ThreadBuilder` (opt-in, default off). On Linux it caps a
  `Background` thread's `util_max` via `sched_setattr`
  (`SCHED_FLAG_KEEP_ALL | SCHED_FLAG_UTIL_CLAMP_MAX`, ~20% of
  `SCHED_CAPACITY_SCALE`), biasing the cpufreq governor toward a lower clock —
  the only lever that lowers frequency on homogeneous CPUs, where `nice` has no
  frequency effect. `Utility`/`Default` are left unclamped.
- **`backend/uclamp.rs`:** declares `struct sched_attr` (no `libc` wrapper) and
  calls `sched_setattr` through `libc::syscall`. Best-effort: ENOSYS/EINVAL/E2BIG/
  EPERM/EOPNOTSUPP (pre-uclamp or pre-5.8 kernels, sandboxes) degrade to a no-op.
  Only ever *lowers* `util_max`, so it stays unprivileged. No-op off Linux.
- **Caveat (documented):** effect requires the `schedutil` governor (or
  `intel_pstate=passive`); fixed governors and HWP bypass the util signal.
- **`bgrt-bench --clamp-frequency`:** new flag that routes the clamp through the
  runtime/thread runners (warns it's a no-op off Linux). README gains a
  "Running on each platform" command cheat-sheet (privilege-free throughput
  everywhere; `--mac-power` + sudo on macOS; `--pin` for hybrid, `--clamp-frequency`
  for homogeneous, sudo for RAPL on Linux) and a homogeneous-CPU clamp note.
- **Run-verified on Intel i7-2720QM (Sandy Bridge, homogeneous) with `schedutil`
  + `--clamp-frequency`:** background mean clock 840 MHz vs 3192 (~3.8× lower) and
  ~3.2× less package energy over a fixed 10 s window, at ~26% throughput — the
  first measured frequency effect on a homogeneous CPU (where `nice` shows
  nothing). Results table added to the README. Finding worth keeping: clamping is
  a *stay-cool / low-power-draw* lever, not a per-unit-work efficiency win — on this
  old silicon background spends slightly *more* energy per work-unit (~88 vs ~74 µJ)
  because fixed/leakage power dominates at low clocks (race-to-idle). Documented in
  the README and `docs/DESIGN.md`.
- **Tests:** `backend/uclamp_tests.rs` covers the per-class cap mapping and reads
  back `uclamp.max` from `/proc/thread-self/sched` (new `current_uclamp_max()`
  helper), tolerating kernels without `CONFIG_UCLAMP_TASK` / `SCHED_FLAG_KEEP_ALL`.
- **Verified:** `cargo clippy -p bgrt --all-features --tests` clean on host
  (macOS) and on the `x86_64-unknown-linux-gnu` / `x86_64-pc-windows-msvc`
  targets; `cargo test -p bgrt --all-features` passes on macOS (the Linux-only
  uclamp tests compile under the Linux target but execute on Linux/CI).

### Optional features, rayon integration, Linux run — 2026-06-13
- **Optional `tokio` feature (default on):** `RuntimeBuilder`/`Runtime` now live
  behind `features = ["tokio"]` (enabled by default). Users who only need
  `spawn_thread`/`ThreadBuilder`/`apply` can opt out with `default-features = false`
  for a lean dep tree with no tokio. `bgrt-bench` now declares
  `features = ["telemetry", "tokio"]` explicitly.
- **New `rayon` feature (opt-in, default off):** `RayonBuilder`/`RayonPool` wrap
  a `rayon::ThreadPool` with the configured `QosClass` applied to every thread at
  start, mirroring the `RuntimeBuilder`/`Runtime` pattern. `pool.install(|| …)`
  routes rayon `par_iter`/`join`/`scope` work through the quiet threads. `RayonPool`
  derefs to `rayon::ThreadPool` for full API access; `pool.qos()` returns the
  configured class.
- **Error variants gated by feature:** `Error::Runtime` behind
  `#[cfg(feature = "tokio")]`; new `Error::ThreadPool` behind
  `#[cfg(feature = "rayon")]`.
- **Clippy fix (`power.rs`):** `PowerStats::parse` and its private helpers
  (`Cluster`, `Acc`, `freq_acc`, `cluster_kind`, `metric_after`, `leading_number`)
  gated `#[cfg(any(target_os = "macos", test))]` — they are only called by the
  macOS-only `power_macos` sampler, but the unit tests still exercise them on all
  platforms. Fixes `cargo clippy --all-targets -- -Dwarnings` on Linux.
- **Linux run-verified on AMD Threadripper (16-core, homogeneous, no E-cores):**
  all tests pass; benchmark shows flat throughput and frequency across executors —
  expected, since `nice(19)` only deprioritizes under CPU contention and there are
  no E-cores for affinity pinning. RAPL energy is available under `sudo` but
  variance (<3%) is measurement noise across whole-package readings on an otherwise-
  idle 16-core machine, not a per-thread signal. Meaningful Linux results require
  either a heterogeneous (P+E) CPU or a CPU-loaded machine.
- **Verified:** `cargo test --workspace`, `cargo test -p bgrt --no-default-features`,
  `cargo test -p bgrt --features rayon`, and `cargo clippy --workspace --all-targets
  -- -Dwarnings` all clean on Linux (Threadripper).

### Review pass — refactor, tests, docs — 2026-06-13
- **Renamed** `bgrt::Builder` → `bgrt::RuntimeBuilder` (symmetry with
  `ThreadBuilder`; clearer at the crate root). Updated lib, tests, bench,
  examples, README.
- **Testability:** extracted the Linux E-core selection into a pure
  `topology::select_efficiency_cores`, with unit tests (hybrid / homogeneous /
  empty / three-tier) that run on any platform.
- **De-duplicated test helpers** into a `#[cfg(test)] test_support` module
  (`current_qos` / `current_nice` / QoS constants), removing three copies of the
  read-back FFI across the backend/runtime/thread test files.
- **Rustdoc examples (doctests):** added to `RuntimeBuilder`, `spawn_thread`, and
  a crate-level `# Example`; now 4 doctests run as part of the suite.
- **Docs:** new [`docs/DESIGN.md`](docs/DESIGN.md) distilling the durable design
  (mechanism, per-OS mapping, anti-starvation, two-runtime pattern, telemetry
  matrix, findings, non-goals); README gained a "Limitations & notes" section and
  links to the design doc; CLAUDE.md cross-links it.
- **Verified:** `cargo test --workspace` (33 lib + 16 bench + 1 integration + 4
  doctests), `clippy --workspace --all-targets -- -Dwarnings` (macOS) and
  `--tests` cross-target (Linux, Windows), examples run, `cargo doc` clean.

### Phase 6 — Docs, examples, polish — 2026-06-13
- Added runnable examples: `background_task`, `mixed_runtimes`, `quiet_threads`
  (`cargo run --example <name> -p bgrt`); all run and pass
  `clippy --all-targets -- -Dwarnings`.
- README: "early development" → real status; "Intended usage" → "Usage" with an
  examples list; folded in the **Apple M1** measured results (Background ran 99.8%
  on E-cores at ~1029 MHz and drew ~12× less CPU power than Default) plus a
  system-wide-telemetry caveat.
- CLAUDE.md: refreshed architecture (all modules: runtime/thread/topology/
  telemetry + bench workload/runner/report/power) and commands (telemetry tests,
  cross-target clippy, examples, sudo `--mac-power`).
- **Verified:** `cargo clippy --workspace --all-targets -- -Dwarnings`, examples
  run, `cargo test --workspace`, `cargo doc` all clean.

### Phase 5.1 — Throughput, powermetrics, macOS finding — 2026-06-13
- **Throughput metric:** the workload now counts work units completed and reports
  `work` + `work/s`. This makes the energy/perf tradeoff visible **unprivileged on
  macOS** (where placement/freq need powermetrics): e.g. a Background runtime
  measured ~2050 work/s vs ~5165 for Default — the E-core confinement, quantified.
- **macOS `powermetrics` telemetry:** `--mac-power` now samples `powermetrics`
  *per executor run* (cluster freq, E/P residency, CPU power) via a `power_macos`
  `Sampler`; parsing is a pure, cross-platform, unit-tested `power::PowerStats`.
  When present it fills the `%E` / frequency / energy columns (and a verdict)
  that are otherwise `n/a` on macOS. Requires running under `sudo`.
- **Finding (macOS QoS override):** a higher-QoS thread that synchronously
  `join`s a background thread **promotes it off the efficiency cores**
  (priority-inversion avoidance); tokio's `await` does not. The harness's
  `background-threads` runner now matches the waiting thread's QoS to the workers
  on macOS so the measurement reflects the executor, not the join — confirmed by
  the throughput dropping from ~5175 to ~2180 work/s. Documented in the README as
  a real macOS behavior users should know.
- Removed the old one-shot `--mac-power` end-of-run reading (superseded by
  per-run sampling).
- **Verified:** macOS — `cargo test --workspace` (29 lib + 16 bench + doctest +
  integration), harness shows the throughput gap; clippy `-Dwarnings` all three
  targets.

### Phase 5 — Comparison harness — 2026-06-13
- `bgrt-bench` now compares executors end to end:
  - `workload` — CPU-bound loop that self-samples telemetry (placement attributed
    to the worker actually running it).
  - `runner` — runs the workload on Default / Utility / Background runtimes and on
    Background OS threads → `RunResult` (wall-clock, `Aggregate`, energy).
  - `report` — aligned text table, pretty JSON (`serde`), and the
    `background_not_hotter` verdict (background peak freq ≤ default).
  - CLI (`clap`): `--duration`, `--workers`, `--interval`, `--executors`,
    `--format table|json`, `--pin` (Linux E-core affinity), `--mac-power`.
- macOS `powermetrics` reader as a defensive `--mac-power` opt-in (parser unit-
  tested; graceful no-sudo → "unavailable" verified). Windows E/P classification
  still deferred.
- `tests/comparison.rs` integration test runs the built binary, parses its JSON,
  and asserts background ≤ default peak frequency — tolerant (skips where
  frequency telemetry is unavailable, e.g. macOS).
- `clippy.toml`: added `allow-expect-in-tests = true` (the integration test uses
  `expect`; `expect_used` needs its own opt-out alongside `allow-unwrap-in-tests`).
- **Verified:** macOS — `cargo test --workspace` (29 lib + 12 bench + 1 doctest +
  1 integration), harness runs (table/JSON/verdict; honest macOS `n/a`); clippy
  `-Dwarnings` + `cargo doc` clean. Linux & Windows — clippy `-Dwarnings` clean
  cross-target.

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
