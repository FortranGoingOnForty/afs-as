//! Audit A6: `.p2align N,,M` carries a max-skip — gcc emits it — and gas
//! omits the alignment padding entirely when it would exceed M. The x86
//! assembler dropped the third argument, so it always padded and the layout
//! (offsets, section size) diverged from gas.
//!
//! Keyed on the layout decision (`.text` length) rather than the exact NOP
//! fill bytes, which are a separate cosmetic detail.

use afs_as::x86::assemble::assemble_x86;

fn text_len(src: &str) -> usize {
    let obj = assemble_x86(src, 0).unwrap_or_else(|e| panic!("{src:?}: {}", e.msg));
    obj.sections
        .iter()
        .find(|s| s.name == ".text")
        .map(|s| s.data.len())
        .unwrap_or(0)
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
