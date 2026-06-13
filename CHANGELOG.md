# Changelog

All notable changes to **bgrt** are recorded here. This file tracks the
project's running state; the phased plan lives in [`docs/ROADMAP.md`](docs/ROADMAP.md).

Format loosely follows [Keep a Changelog](https://keepachangelog.com/);
the project is pre-1.0 and not yet released.

## [Unreleased]

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
