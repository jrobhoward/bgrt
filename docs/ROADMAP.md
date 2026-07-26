# bgrt — Roadmap

Status and plan: what stands between here and 1.0. Everything else lives where it
will not go stale.

- What each class does per OS — [the README](../README.md#qos-classes)
- Why it does that, and what turned up along the way — [`DESIGN.md`](DESIGN.md)
- Measured results, and the machines still missing — [`BENCHMARKS.md`](BENCHMARKS.md)
- Module map, commands, conventions — [`../CLAUDE.md`](../CLAUDE.md)
- What changed when — [`../CHANGELOG.md`](../CHANGELOG.md)

The eight-phase build plan is finished and has been retired from this file. What
each phase delivered is in `CHANGELOG.md` and the git history.

## 0.9 — public preview

The 1.0 candidate: the API expected to freeze, under a version number that still
allows a break if the evaluation turns one up.

Nothing is outstanding. The last blocker — a benchmark for the disk half of a
`QosClass` — closed on 2026-07-26.

Release steps, in order. The crates.io and docs.rs badges read "not found" until
the crate is published, so publishing before the repository goes public avoids a
window where the README looks broken.

| # | Step | Notes |
|---|---|---|
| 1 | Bump the workspace `version` to `0.9.0` | Including the internal `bgrt = { path, version }` dependency |
| 2 | Retitle `## [Unreleased]` to `## [0.9.0]` in `CHANGELOG.md`, dated | Also drop "nothing has been released yet" from the header |
| 3 | `cargo publish -p bgrt` | Check that docs.rs builds and renders the `doc(cfg)` feature badges |
| 4 | Make the GitHub repository public | The CI badge only resolves once it is public; the rest resolve after step 3 |
| 5 | Tag `v0.9.0` and push | Starts the 60-day clock below |

## 0.9 to 1.0 — the evaluation window

1.0 is tagged 60 days after the 0.9 release, provided nothing in that window
calls for a breaking API change. If something does, the clock restarts from the
release that addresses it.

The window exists because internal review is no substitute for outside use. It is
not an open-ended hold: 60 days with a stated end is what makes it close.

Two things are known not to block it.

- **The `io_class` override** — split CPU and I/O control, if it is ever wanted —
  is additive by construction, so it can land in a 1.x minor. See
  [`DESIGN.md`](DESIGN.md#why-one-knob-and-not-two).
- **The unmeasured platforms.** Hybrid-Linux `--pin` and the Windows performance
  story are documented as unverified rather than implied to work. Measuring them
  changes no API, so they affect confidence rather than the release.

The remaining semver risk is not in this crate's own surface. Because `bgrt`
wraps tokio and rayon rather than hiding them, a major release of either forces a
major release here.

### Stability commitments made at 0.9

| Commitment | Where it lives |
|---|---|
| Raising the MSRV is a minor bump, never a patch | crate docs, README |
| `telemetry` is exempt from semver | `telemetry` module docs |
| `QosClass` is `#[non_exhaustive]` | `qos.rs` |
| `Error` is `#[non_exhaustive]` | `error.rs` |
| A tokio or rayon major is a `bgrt` major | crate docs, README |

## Open gaps carried into 0.9

None of these block the release, and all are written down rather than glossed
over. The ones that need hardware are listed with the command to run in
[`BENCHMARKS.md`](BENCHMARKS.md#data-points-still-wanted).

| Gap | Standing |
|---|---|
| Hybrid-Linux E-core pinning has never run on P+E silicon | Untested code rather than a measured feature. The selection logic is unit-tested; the sysfs read and `sched_setaffinity` against a non-empty set have never executed |
| Windows performance is unmeasured | Behaviour-tested in CI. Throughput and disk behaviour are reasoned from Microsoft's documentation and from what Chromium ships |
| Disk measured on macOS only | `--workload io` runs everywhere, but the published numbers are from one M1. Linux needs a run per I/O scheduler; Windows needs a run at all |
| AMD hybrid (Zen 4c, Zen 5c) is not detected | Deliberate: no unprivileged interface exists. Revisit if the core type gains one outside debugfs ([`DESIGN.md`](DESIGN.md#efficiency-core-detection-vs-pinning)) |
| The CPU workload has no contended mode | `nice(19)` does nothing without contention, so the CPU half of the class is unproven on Linux. Two concurrent harness runs are the workaround for now |
| The macOS I/O policy cannot be read back | `setiopolicy_np` would opt the thread out of QoS entirely, so it is never called. The effect is measured instead, through `--workload io` |
| CI flake watch | `tests/comparison.rs` asserts that background `max_mhz` is at most default's. If it starts flaking, widen it to a tolerance band rather than deleting it |
