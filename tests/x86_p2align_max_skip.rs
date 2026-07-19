//! `.p2align` layout and fill regression coverage.

use afs_as::x86::assemble::assemble_x86;

fn text_len(src: &str) -> usize {
    text_bytes(src).len()
}

fn text_bytes(src: &str) -> Vec<u8> {
    let obj = assemble_x86(src, 0).unwrap_or_else(|e| panic!("{src:?}: {}", e.msg));
    obj.sections
        .iter()
        .find(|s| s.name == ".text")
        .map(|s| s.data.clone())
        .unwrap_or_default()
}

#[test]
fn max_skip_suppresses_alignment_when_padding_exceeds_it() {
    // One byte in, aligning to 16 needs 15 bytes of padding. With max-skip 7
    // that exceeds the limit, so gas emits no padding: .text is just the two
    // bytes.
    assert_eq!(text_len(".text\n.byte 1\n.p2align 4,,7\n.byte 2\n"), 2);
    // 7 > 3: also skipped.
    assert_eq!(text_len(".text\n.byte 1\n.p2align 3,,3\n.byte 2\n"), 2);
}

#[test]
fn alignment_applies_when_padding_fits_the_max_skip() {
    // 15 <= 15: padding fits, so it aligns to 16 (byte 2 at offset 16).
    assert_eq!(text_len(".text\n.byte 1\n.p2align 4,,15\n.byte 2\n"), 17);
}

#[test]
fn no_max_skip_always_aligns() {
    // Without the third argument the alignment is unconditional.
    assert_eq!(text_len(".text\n.byte 1\n.p2align 4\n.byte 2\n"), 17);
    assert_eq!(text_len(".text\n.byte 1\n.p2align 3\n.byte 2\n"), 9);
}

#[test]
fn explicit_fill_byte_is_preserved() {
    assert_eq!(
        text_bytes(".text\n.byte 0\n.p2align 2,0xcc\n.byte 1\n"),
        [0, 0xcc, 0xcc, 0xcc, 1]
    );
    assert_eq!(
        text_bytes(".text\n.byte 0\n.p2align 2,511\n.byte 1\n"),
        [0, 0xff, 0xff, 0xff, 1]
    );
    assert_eq!(
        text_bytes(".text\n.byte 0\n.p2align 2,\n.byte 1\n"),
        [0, 0, 0, 0, 1]
    );
}
