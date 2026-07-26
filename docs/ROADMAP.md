# bgrt — Roadmap & Status

Where the project is, and what stands between here and 1.0.

This file is **status and plan only**. It deliberately does not restate the
design or the API — those drift, and a stale copy is worse than no copy:

- **What each class does per OS** → [`../README.md`](../README.md) (canonical QoS table)
- **Why it does that** → [`DESIGN.md`](DESIGN.md)
- **Module map, commands, conventions** → [`../CLAUDE.md`](../CLAUDE.md)
- **What changed when** → [`../CHANGELOG.md`](../CHANGELOG.md)

## Goals

Energy-efficient background execution a developer opts into per unit of work, as
a regular (non-admin) user, on macOS, Windows, and Linux — including
heterogeneous (big.LITTLE / P+E) and DVFS CPUs. Four hard requirements shape
every decision; the rationale for each is in [`DESIGN.md`](DESIGN.md):

1. **No admin / root for the library.** It only ever *lowers* its own threads'
   demands. (The measurement harness may need elevation for power readings.)
2. **Per-thread / per-executor control**, classified once at thread start —
   not a mutable per-task flag. This also sidesteps Linux's one-way `nice`.
3. **Don't trigger P-core turbo or the fans.** Express intent; let the kernel
   place work and bias DVFS. Never set frequency directly.
4. **No indefinite starvation.** Weighted-fair low priority, never
   run-only-when-idle — on the I/O axis as much as the CPU axis.

We **wrap** tokio and rayon; we do not fork or patch them.

---

## Release plan

### 0.9 — public preview

The 1.0 candidate. Ships the API we expect to freeze, under a version number
that still permits a break if evaluation turns one up.

Steps **in order** — the README's crates.io and docs.rs badges read "not found"
until the crate is published, so publishing first avoids a window where the
newly-public repo looks broken:

| # | Step | Notes |
|---|---|---|
| 1 | Bump workspace `version` to `0.9.0` | Including the internal `bgrt = { path, version }` dependency |
| 2 | Add a dated `## [0.9.0]` section to `CHANGELOG.md` | Replaces "pre-1.0 and not yet released" |
| 3 | `cargo publish -p bgrt` | Confirm the docs.rs build succeeds and renders the `doc(cfg)` feature badges |
| 4 | Make the GitHub repository public | The CI badge only resolves once the repo is public; the other four resolve immediately after step 3 |
| 5 | Tag `v0.9.0` and push | Starts the 60-day clock below |

**Blocking 0.9:**

- [ ] **`bgrt-bench` has no I/O workload.** The crate's headline claim is that
      one `QosClass` covers *CPU and disk*, but every published benchmark
      measures a CPU-bound loop — so half the claim is documented and unmeasured.
      Either add an I/O workload (needs a metric, and care to avoid measuring the
      page cache) or state plainly in the README that the numbers are CPU-only.
      The README currently carries the caveat; replacing it with a measurement is
      the better outcome, and is the last substantive gap before going public.

**Done and not blocking:** CI matrix, licenses/packaging, error causes, telemetry
semver exemption, shutdown control, `current_thread`, Windows E/P telemetry,
block-I/O priority on all three platforms, `QosClass` marked `#[non_exhaustive]`,
MSRV policy stated, docs.rs feature badges.

### 0.9 → 1.0 — the evaluation window

**1.0 is tagged 60 days after the 0.9 release**, provided no issue or request in
that window calls for a breaking API change. If one does, the clock restarts from
the release that addresses it.

The window exists because no amount of internal review substitutes for outside
use. What it is *not* is an open-ended hold: 60 days with a stated end date is
what makes the window close.

Two things are already known not to block it:

- **The `io_class` override** (split CPU/I/O control), if demand ever appears, is
  designed to be **additive** — see [`DESIGN.md`](DESIGN.md) → *Why one knob and
  not two*. It can land in a 1.x minor.
- **Hardware-blocked `--pin` verification** on real P+E Linux does not change the
  API, and is documented as unmeasured rather than implied to work.

The genuine residual semver risk is not `bgrt`'s own surface: because the crate
wraps Tokio and rayon rather than hiding them, **a major release of either forces
a major release of `bgrt`**. Stated in the crate docs under *Semver and wrapped
dependencies*.

### Stability commitments made at 0.9

| Commitment | Where it lives |
|---|---|
| MSRV increase is a **minor** bump, never a patch | crate docs, README |
| `telemetry` is **exempt** from semver entirely | `telemetry` module docs |
| `Error` is `#[non_exhaustive]` | `error.rs` |
| `QosClass` is `#[non_exhaustive]` | `qos.rs` |
| A Tokio/rayon major is a `bgrt` major | crate docs |

---

## Phased plan & status

All eight phases are complete. Phases are kept as a record of what was built and
what each one verified — the detail behind them is in `CHANGELOG.md`.

| Phase | Title | Status |
|------:|-------|--------|
| 0 | Workspace scaffold + conventions | ✅ Done |
| 1 | QoS backends (macOS / Windows / Linux) | ✅ Done — affinity deferred to Phase 2 so `apply` stays the always-safe part |
| 2 | `Runtime` — tokio wrapper | ✅ Done — incl. blocking-pool classification |
| 3 | Quiet thread + blocking spawn API | ✅ Done |
| 4 | Telemetry (core / frequency / power) | ✅ Done — primitives; Sampler orchestration moved to Phase 5 |
| 5 | Comparison harness (`bgrt-bench`) | ✅ Done — table/JSON + verdict + integration test |
| 6 | Docs, examples, polish | ✅ Done |
| 7 | Road to 1.0 (CI, licensing, API freeze) | ✅ Done — CI matrix, licenses/packaging, error causes, shutdown control, `current_thread`, Windows E/P telemetry |
| 8 | Block-I/O priority (`QosClass` covers disk) | ✅ Done — macOS free via QoS, Linux via `backend/ioprio.rs`, Windows via `THREAD_MODE_BACKGROUND_BEGIN` + memory-priority restore. Never the process-wide variant |

Verification standard applied throughout: each phase compiles cleanly on all
three OSes, ships tests, and lands a `CHANGELOG.md` entry before it counts as
done. Since Phase 7 that includes a green CI matrix
([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)), which is where the
Linux and Windows backends actually *execute* — cross-compiling proves only that
they type-check.

---

## Open questions & known gaps

Resolved questions are kept because "why don't you do X" recurs; the full
reasoning for each is in [`DESIGN.md`](DESIGN.md).

- ~~**tokio `on_thread_start` & the blocking pool**~~ — ✅ Resolved (Phase 2): the
  hook fires for blocking-pool threads too. *Amended (Phase 7):* true for the
  **multi-thread** scheduler only. On a current-thread runtime it fires *only*
  for the blocking pool, never for the thread driving async tasks — which is why
  `current_thread(true)` runs the scheduler on a `bgrt`-owned, classified thread.
- ~~**No current-thread runtime**~~ — ✅ Resolved (Phase 7) as
  `RuntimeBuilder::current_thread(bool)`, with `bgrt` owning the driver thread.
- ~~**What about I/O?**~~ — ✅ **In scope and shipped on all three platforms**,
  folded into `QosClass` rather than added as a fourth builder knob, because
  Windows and macOS bundle the axes and a split API could not be honoured on
  either.
- **What about GPU?** — **Probably never.** No OS exposes a per-thread GPU QoS,
  the per-API priorities that exist arbitrate contention rather than save energy,
  and GPU work belongs to a queue rather than to a classifiable thread. Those are
  statements about the current platforms, not a principle: `DESIGN.md` lists the
  specific developments that should reopen it.

**Live gaps, all documented rather than implied away:**

| Gap | Standing |
|---|---|
| `bgrt-bench` measures CPU only, not disk | **Blocks 0.9** (above) |
| Hybrid-Linux E-core pinning never run on P+E silicon | **Partly a software gap, not purely hardware-blocked** — see below. Needs Alder/Raptor/Meteor Lake to confirm end to end |
| AMD hybrid (Zen 4c / Zen 5c) is not detected at all | Deliberate. No unprivileged interface exists to detect it — see below |
| Windows throughput under contention unmeasured | Behaviour-tested in CI; performance reasoned from Microsoft's docs and from what Chromium ships |
| macOS I/O coverage structurally unassertable | `setiopolicy_np` would opt the thread out of QoS entirely — deliberately never called. Guarded by a regression test |
| Windows E/P *labelling* branch unexercised | CI runners are homogeneous VMs. Placement is EcoQoS's job, so this affects telemetry only |
| CI flake watch | `tests/comparison.rs` asserts background `max_mhz` ≤ default. Green so far; if it flakes, widen to a tolerance band rather than deleting it |
| macOS `Background` can be very slow | E-core jail; `Utility` is the documented middle ground (LLVM/clangd hit exactly this) |

### Efficiency-core detection on x86 — what changed, and what is left

This row read as purely hardware-blocked until 2026-07-26, which was wrong in a
way worth recording: it implied the code was fine and only a test machine was
missing.

**`cpu_capacity` appears not to exist on x86.** It arrived in 2016 as an
arm/arm64 attribute (`arch_topology.c`, `CONFIG_GENERIC_ARCH_TOPOLOGY`); Intel
explicitly proposed a *different* interface in 2020 rather than adopting it; and
although `intel_pstate` has fed asymmetric capacity to the scheduler since 2024,
it does so via an x86-specific per-CPU variable, not the generic topology code
that creates the sysfs file. So `pin_efficiency_cores(true)` was almost certainly
a **silent no-op on every Intel hybrid CPU** — the exact failure mode this
project calls out elsewhere, since it is indistinguishable from the correct
no-op on a homogeneous machine.

**Fixed by adding a second source:** the hybrid perf PMUs. The kernel registers
`cpu_core` and `cpu_atom` PMUs on hybrid x86, each with a `cpus` file; on an
i9-12900K `cpu_atom/cpus` reads `16-23`. It is unprivileged, authoritative
(no minimum-wins inference needed), and absent on non-hybrid machines, so the
homogeneous case still yields empty. `topology::efficiency_cores()` now tries
`cpu_capacity` first, then the PMU.

**Still unverified**, and why this stays a gap: nobody has run it on real hybrid
silicon. The cpulist parser is unit-tested against the published 12900K values
and the malformed cases, so the *decision* is covered off-hardware — but the
sysfs read and `sched_setaffinity` against a non-empty set have still never
executed. Treat as untested code until someone runs
`cargo run --release -p bgrt-bench -- --duration 3 --pin` on Alder Lake or later.

**AMD is intentionally out of scope.** Zen 4c / Zen 5c ("dense") cores are the
same ISA and microarchitecture as their classic siblings, differing in clock
ceiling and cache — so they share a PMU, and there is no `cpu_atom` equivalent.
The kernel does know the core type (CPUID `0x80000026`,
`X86_FEATURE_AMD_HTR_CORES`), but exposes it to userspace only through
**debugfs** (`/sys/kernel/debug/x86/topo/`), which is root-only and something
this library will not require. Detection therefore returns empty, which disables
pinning rather than pinning to a wrong set.

Deliberately **not** inferred from `cpufreq/cpuinfo_max_freq`: per-core boost
binning and ITMT favoured cores make genuinely homogeneous CPUs report differing
maximums, so a minimum-wins rule over that field would mistake a binned
Threadripper for a hybrid and confine background work to one arbitrary core.
Revisit if AMD's core type gains a non-debugfs interface — the Zen 6 low-power
core work suggests that pressure is building.
