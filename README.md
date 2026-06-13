# bgrt

**bgrt** ("background runtime") runs your async tasks and threads at the lowest
energy footprint the OS allows — on **efficiency cores**, at **low clock
frequency**, **without spinning up the fans** — while still making forward
progress when the machine is busy. Write ordinary async/sync Rust and schedule
it onto a quiet executor.

- **Per-thread energy QoS** — classify work as `Background`, `Utility`, or
  `Default`, mapped to each OS's native facility: macOS QoS classes, Windows
  EcoQoS, Linux `nice` + efficiency-core affinity.
- **No admin required** — the library only ever *lowers* its own threads' demands.
- **Wraps tokio** (no fork) — an energy-classified runtime, plus a quiet
  OS-thread spawner for non-async work.
- **Never starves** — quiet work uses weighted-fair low priority, so it always
  crawls forward under load (not run-only-when-idle).
- **Cross-platform** — macOS, Windows, Linux, including big.LITTLE / P+E CPUs.

> **Status:** early development. The plan lives in
> [`docs/ROADMAP.md`](docs/ROADMAP.md) and project state in
> [`CHANGELOG.md`](CHANGELOG.md). Phase 0 (scaffold) is in place; the QoS
> backends, runtime wrapper, and measurement harness land phase by phase.

## Intended usage

```rust
use bgrt::QosClass;

// A quiet async executor (wraps a tokio runtime).
let rt = bgrt::Builder::new()
    .qos(QosClass::Background)
    .worker_threads(1)
    .build()?;
rt.spawn(async { /* quiet async work */ });

// A quiet OS thread (non-async path).
let jh = bgrt::spawn_thread(QosClass::Background, || { /* CPU-bound loop */ });

// Or classify the current thread directly.
bgrt::apply(QosClass::Utility)?;
```

Run some work normally and other work quietly by keeping a normal tokio runtime
*and* a `bgrt` runtime in the same process, then spawning onto the right one.

## QoS classes

| Class | macOS | Windows | Linux |
|---|---|---|---|
| `Background` | `QOS_CLASS_BACKGROUND` (efficiency cores) | EcoQoS + below-normal | `nice(19)` + opt-in E-core affinity |
| `Utility` | `QOS_CLASS_UTILITY` | EcoQoS + normal | `nice(10)` |
| `Default` | none | none | `nice(0)` |

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --tests -- -Dwarnings
cargo run -p bgrt-bench          # comparison harness (work in progress)
```

Requires Rust ≥ 1.85 (edition 2024). See [`CLAUDE.md`](CLAUDE.md) for
architecture and conventions.
