//! End-to-end checks: run the harness and assert the background executor doesn't
//! clock higher than the default one, and that the disk workload produces a
//! well-formed report. Tolerant — skips where the platform can't report
//! frequency (e.g. macOS without `powermetrics`) or can't bypass the page cache.
#![allow(non_snake_case)]

use std::process::Command;

use serde_json::Value;

#[test]
fn comparison____background_peak_freq____at_most_default_or_skipped() {
    let exe = env!("CARGO_BIN_EXE_bgrt-bench");
    let output = Command::new(exe)
        .args([
            "--duration",
            "0.3",
            "--interval",
            "20",
            "--executors",
            "default,background",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run bgrt-bench");
    assert!(output.status.success(), "bgrt-bench exited non-zero");

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("invalid JSON output");
    let rows = parsed.as_array().expect("expected a JSON array");
    let max_mhz = |label: &str| {
        rows.iter()
            .find(|r| r["executor"] == label)
            .and_then(|r| r["max_mhz"].as_u64())
    };

    match (max_mhz("default"), max_mhz("background")) {
        (Some(default), Some(background)) => {
            assert!(
                background <= default,
                "background peaked at {background} MHz, above default's {default} MHz"
            );
        }
        _ => eprintln!("skipping frequency assertion: telemetry unavailable on this platform"),
    }
}

#[test]
fn comparison____io_workload____reports_both_phases_against_a_baseline() {
    let exe = env!("CARGO_BIN_EXE_bgrt-bench");
    let dir = tempfile::tempdir().expect("failed to create scratch dir");
    let output = Command::new(exe)
        .args([
            "--workload",
            "io",
            "--duration",
            "0.2",
            "--io-file-size-mib",
            "16",
            "--io-dir",
            &dir.path().to_string_lossy(),
            "--executors",
            "default,background",
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run bgrt-bench");
    assert!(output.status.success(), "bgrt-bench exited non-zero");

    let parsed: Value = serde_json::from_slice(&output.stdout).expect("invalid JSON output");
    assert!(
        parsed["foreground_baseline_mib_s"].as_f64().unwrap_or(0.0) > 0.0,
        "foreground baseline measured nothing"
    );

    let rows = parsed["rows"].as_array().expect("expected a rows array");
    assert_eq!(rows.len(), 2, "expected one row per executor");
    for label in ["default", "background"] {
        let row = rows
            .iter()
            .find(|r| r["executor"] == label)
            .unwrap_or_else(|| panic!("missing row for {label}"));
        assert_eq!(row["errors"].as_u64(), Some(0), "{label} had read errors");
        assert!(
            row["solo_mib_s"].as_f64().unwrap_or(0.0) > 0.0,
            "{label} read nothing while solo"
        );
        assert!(
            row["foreground_protection_pct"].as_f64().unwrap_or(0.0) > 0.0,
            "{label} starved the foreground completely"
        );
    }

    // Deliberately *not* asserted: that background protects the foreground more
    // than default does. That is the result that matters, but a 0.2 s run on a shared,
    // virtualized CI disk can't measure it reliably — see docs/BENCHMARKS.md for
    // the real-hardware numbers. This test guards the plumbing.
    eprintln!("cache bypass: {}", parsed["cache_bypass"]);
}
