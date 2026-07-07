//! Audit A5/A6: the x86 assembler must not panic on degenerate section use,
//! and must match gas's accept/reject for content in a NOBITS (`.bss`)
//! section.
//!
//! A5 (panic-instead-of-diagnostic):
//!   - `.ascii`/`.asciz`/`.zero` before any section directive indexed section
//!     `usize::MAX` and panicked. gas defaults leading content to `.text`.
//!   - an instruction with a same-section PC-relative reference inside `.bss`
//!     patched into the section's (empty) NOBITS data and panicked on the
//!     out-of-range slice.
//!
//! A6 (silent-wrong): non-zero `.byte`/`.ascii` in `.bss` was silently
//! dropped; gas errors ("attempt to store non-zero value/string").

use afs_as::x86::assemble::assemble_x86;

fn asm(src: &str) -> Result<(), String> {
    assemble_x86(src, 0).map(|_| ()).map_err(|e| e.msg)
}

#[test]
fn leading_data_directives_default_to_text_not_panic() {
    // gas puts each of these in .text with no error. The old code panicked
    // on `usize::MAX` for .ascii/.asciz/.zero and hard-errored on .byte.
    for src in [".ascii \"hi\"\n", ".asciz \"hi\"\n", ".zero 4\n", ".byte 5\n", ".long 7\n"] {
        assert!(asm(src).is_ok(), "leading {src:?} should assemble into .text");
    }
}

#[test]
fn nonzero_data_in_bss_is_rejected() {
    // gas: "attempt to store non-zero value/string in section `.bss'".
    for src in [
        ".bss\n.byte 5\n",
        ".bss\n.short 1\n",
        ".bss\n.long 9\n",
        ".bss\n.quad 1\n",
        ".bss\n.ascii \"hi\"\n",
        ".bss\n.asciz \"x\"\n",
    ] {
        let e = asm(src).expect_err(&format!("{src:?} must be rejected in .bss"));
        assert!(
            e.contains(".bss"),
            "{src:?}: diagnostic should name .bss, got: {e}"
        );
    }
}

#[test]
fn symbolic_data_in_bss_is_rejected() {
    // A relocation is non-zero storage too.
    let e = asm(".bss\n.quad foo\n").expect_err("symbolic .quad in .bss must be rejected");
    assert!(e.contains(".bss"), "diagnostic should name .bss, got: {e}");
}

#[test]
fn zero_fill_in_bss_is_accepted() {
    // gas accepts these: they reserve space without storing initialized data.
    for src in [".bss\n.zero 8\n", ".bss\n.byte 0\n", ".bss\n.asciz \"\"\n"] {
        assert!(asm(src).is_ok(), "{src:?} should be accepted in .bss");
    }
}

#[test]
fn pcrel_reference_inside_bss_diagnoses_not_panics() {
    // Same-section local PC-rel patch into NOBITS: no data to patch. Must be
    // a diagnostic, never an out-of-bounds panic.
    let src = ".bss\nleaq foo(%rip), %rax\nfoo:\n.zero 4\n";
    let e = asm(src).expect_err("PC-rel into NOBITS must be a diagnostic");
    assert!(
        e.contains("NOBITS") || e.contains(".bss"),
        "diagnostic should mention NOBITS/.bss, got: {e}"
    );
}
