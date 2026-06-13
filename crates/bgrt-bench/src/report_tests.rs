//! Tests for report formatting and the comparison check.
#![allow(non_snake_case)]

use super::{Summary, background_not_hotter, json, table};

fn summ(executor: &str, max_mhz: Option<u32>) -> Summary {
    Summary {
        executor: executor.to_owned(),
        wall_ms: 1000,
        distinct_cpus: 1,
        efficiency_pct: None,
        mean_mhz: None,
        max_mhz,
        energy_j: None,
        samples: 10,
    }
}

#[test]
fn background_not_hotter____background_lower____is_some_true() {
    let s = vec![summ("default", Some(3200)), summ("background", Some(1000))];
    assert_eq!(background_not_hotter(&s), Some(true));
}

#[test]
fn background_not_hotter____background_higher____is_some_false() {
    let s = vec![summ("default", Some(1000)), summ("background", Some(3200))];
    assert_eq!(background_not_hotter(&s), Some(false));
}

#[test]
fn background_not_hotter____missing_frequency____is_none() {
    let s = vec![summ("default", None), summ("background", Some(1000))];
    assert_eq!(background_not_hotter(&s), None);
}

#[test]
fn background_not_hotter____missing_executor____is_none() {
    let s = vec![summ("background", Some(1000))];
    assert_eq!(background_not_hotter(&s), None);
}

#[test]
fn json____serializes_expected_fields() {
    let s = vec![summ("background", Some(1200))];
    let out = json(&s).unwrap();
    assert!(out.contains("\"executor\""));
    assert!(out.contains("background"));
    assert!(out.contains("\"max_mhz\""));
}

#[test]
fn table____contains_header_and_rows() {
    let s = vec![summ("default", Some(3000)), summ("background", None)];
    let out = table(&s);
    assert!(out.contains("executor"));
    assert!(out.contains("max_mhz"));
    assert!(out.contains("default"));
    assert!(out.contains("background"));
    // Missing frequency renders as "n/a", not a panic or empty cell.
    assert!(out.contains("n/a"));
}
