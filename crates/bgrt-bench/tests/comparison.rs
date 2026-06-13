//! End-to-end check: run the harness and assert the background executor doesn't
//! clock higher than the default one. Tolerant — skips where the platform can't
//! report frequency (e.g. macOS without `powermetrics`).
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
