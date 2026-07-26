//! Tests for the solo/contended phase runner.
#![allow(non_snake_case)]

use std::time::Duration;

use super::{IoPlan, foreground_baseline, run, seed};
use crate::io_file::ScratchFile;
use crate::io_workload::IoConfig;
use crate::runner::Executor;

fn plan(scratch: &ScratchFile) -> IoPlan {
    let block = 64 * 1024;
    IoPlan {
        cfg: IoConfig {
            duration: Duration::from_millis(60),
            block,
            blocks: crate::io_file::blocks_in(scratch.size(), block),
        },
        path: scratch.path().to_path_buf(),
        workers: 1,
        foreground: 1,
        pin: false,
        clamp: false,
    }
}

#[test]
fn seed____per_worker____is_distinct_and_stable() {
    assert_ne!(seed(1, 0), seed(1, 1));
    assert_eq!(seed(1, 0), seed(1, 0));
}

#[test]
fn foreground_baseline____plain_threads____reads_the_scratch_file() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = ScratchFile::create(dir.path(), 4 * 1024 * 1024, false).unwrap();
    let phase = foreground_baseline(&plan(&scratch));
    assert_eq!(phase.errors, 0);
    assert!(phase.reads >= 1);
    assert!(phase.mib_per_s() > 0.0);
}

#[test]
fn run____every_executor____produces_both_phases() {
    let dir = tempfile::tempdir().unwrap();
    let scratch = ScratchFile::create(dir.path(), 4 * 1024 * 1024, false).unwrap();
    let plan = plan(&scratch);

    for executor in [Executor::Default, Executor::BackgroundThreads] {
        let result = run(executor, &plan).unwrap();
        assert_eq!(result.executor, executor);
        assert_eq!(
            result.solo.errors,
            0,
            "{} solo phase errored",
            executor.label()
        );
        assert!(
            result.solo.reads >= 1,
            "{} did no solo reads",
            executor.label()
        );
        assert!(
            result.contended.reads >= 1,
            "{} did no contended reads",
            executor.label()
        );
        assert!(
            result.foreground.reads >= 1,
            "foreground did no reads against {}",
            executor.label()
        );
        // The gate means both sides of the contended phase cover the same window.
        let skew = result.contended.elapsed.abs_diff(result.foreground.elapsed);
        assert!(
            skew < Duration::from_millis(50),
            "contended phases drifted apart by {skew:?}"
        );
    }
}
