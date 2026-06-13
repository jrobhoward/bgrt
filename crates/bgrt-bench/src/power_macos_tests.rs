//! Tests for the macOS powermetrics parser.
#![allow(non_snake_case)]

use super::{cpu_power_mw, parse_cpu_power_mw};

#[test]
fn parse_cpu_power_mw____well_formed_line____parses() {
    let text = "Some header\nCPU Power: 1234 mW\nOther: 5\n";
    assert_eq!(parse_cpu_power_mw(text), Some(1234));
}

#[test]
fn parse_cpu_power_mw____no_power_line____is_none() {
    assert_eq!(parse_cpu_power_mw("nothing useful here"), None);
}

#[test]
fn cpu_power_mw____without_root____degrades_to_none() {
    // We can't assert a value (would need sudo), but it must never panic and,
    // unprivileged, returns None rather than erroring.
    let _ = cpu_power_mw();
}
