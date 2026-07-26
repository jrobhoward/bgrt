//! Tests for scratch-file setup, alignment, and the sysfs scheduler parser.
#![allow(non_snake_case)]

use rstest::rstest;

use super::{
    ALIGN, AlignedBuf, CacheBypass, Reader, ScratchFile, block_offset, blocks_in, parse_scheduler,
    validate_block,
};

#[rstest]
#[case("mq-deadline kyber [bfq]", Some("bfq"))]
#[case("[mq-deadline] kyber bfq", Some("mq-deadline"))]
#[case("none\n", Some("none"))]
#[case("mq-deadline kyber bfq", None)] // no active marker: don't guess
#[case("", None)]
#[case("[]", None)]
fn parse_scheduler____queue_scheduler_line____picks_the_active_one(
    #[case] raw: &str,
    #[case] expected: Option<&str>,
) {
    assert_eq!(parse_scheduler(raw).as_deref(), expected);
}

#[rstest]
#[case(4096, true)]
#[case(64 * 1024, true)]
#[case(0, false)]
#[case(1000, false)]
#[case(4096 + 1, false)]
fn validate_block____alignment____only_multiples_of_align_accepted(
    #[case] bytes: usize,
    #[case] ok: bool,
) {
    assert_eq!(validate_block(bytes).is_ok(), ok);
}

#[test]
fn blocks_in____file_smaller_than_a_block____never_zero() {
    assert_eq!(blocks_in(1024, 4096), 1);
    assert_eq!(blocks_in(0, 4096), 1);
    assert_eq!(blocks_in(64 * 1024, 4096), 16);
}

#[test]
fn block_offset____any_random_value____stays_aligned_and_in_range() {
    let (blocks, block) = (16u64, 4096usize);
    for rand in [0u64, 1, 15, 16, 12_345, u64::MAX] {
        let off = block_offset(rand, blocks, block);
        assert_eq!(off % block as u64, 0, "offset {off} not block-aligned");
        assert!(off < blocks * block as u64, "offset {off} past end of file");
    }
}

#[test]
fn aligned_buf____requested_length____exposes_an_aligned_window() {
    let mut buf = AlignedBuf::new(64 * 1024);
    let slice = buf.as_mut_slice();
    assert_eq!(slice.len(), 64 * 1024);
    assert_eq!(slice.as_ptr() as usize % ALIGN, 0, "buffer not aligned");
}

#[test]
fn cache_bypass____merge____any_buffered_taints_the_phase() {
    assert_eq!(
        CacheBypass::Direct.merge(CacheBypass::Direct),
        CacheBypass::Direct
    );
    assert_eq!(
        CacheBypass::Direct.merge(CacheBypass::Buffered),
        CacheBypass::Buffered
    );
    assert_eq!(CacheBypass::default(), CacheBypass::Buffered);
}

#[test]
fn scratch_file____created____is_readable_at_aligned_offsets_and_removed_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let size = 2 * 1024 * 1024;
    let path = {
        let scratch = ScratchFile::create(dir.path(), size, false).unwrap();
        assert_eq!(scratch.size(), size);
        assert_eq!(std::fs::metadata(scratch.path()).unwrap().len(), size);

        let reader = Reader::open(scratch.path()).unwrap();
        let mut buf = AlignedBuf::new(ALIGN);
        let n = reader.read_at(buf.as_mut_slice(), ALIGN as u64).unwrap();
        assert_eq!(n, ALIGN);
        // Pseudo-random fill, not a hole: a compressing filesystem can't fake it.
        assert!(buf.as_mut_slice().iter().any(|&b| b != 0));

        scratch.path().to_path_buf()
    };
    assert!(!path.exists(), "scratch file outlived its owner");
}

#[test]
fn scratch_file____kept_then_recreated____is_reused() {
    let dir = tempfile::tempdir().unwrap();
    let size = 1024 * 1024;

    let first = ScratchFile::create(dir.path(), size, true).unwrap();
    let path = first.path().to_path_buf();
    let created = std::fs::metadata(&path).unwrap().modified().unwrap();
    drop(first);
    assert!(path.exists(), "keep=true should leave the file behind");

    let second = ScratchFile::create(dir.path(), size, false).unwrap();
    assert_eq!(
        std::fs::metadata(second.path())
            .unwrap()
            .modified()
            .unwrap(),
        created,
        "a same-sized scratch file should be reused, not rewritten"
    );
}
