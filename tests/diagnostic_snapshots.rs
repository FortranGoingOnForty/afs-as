use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root(prefix: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), id));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

fn afs_as() -> Command {
    Command::new(env!("CARGO_BIN_EXE_afs-as"))
}

fn run_failure_snapshot(name: &str, src: &str, expected: &str) {
    let root = temp_root("afs_diag_snapshot");
    let input = root.join(name);
    fs::write(&input, src).expect("write input");

    let output = afs_as().arg(&input).output().expect("run afs-as");
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let stderr = normalize_stderr(&String::from_utf8_lossy(&output.stderr), &input);
    assert_eq!(stderr, expected);
}

fn normalize_stderr(stderr: &str, input: &Path) -> String {
    stderr.replace(input.to_str().expect("input path"), "<input>")
}

#[test]
fn snapshot_unsupported_directive() {
    run_failure_snapshot(
        "unsupported-directive.s",
        ".text\n.unknown_directive\n",
        "<input>:2:1: error: unsupported directive '.unknown_directive'\n.unknown_directive\n^\n",
    );
}

#[test]
fn snapshot_trailing_arm64_token() {
    run_failure_snapshot(
        "trailing-token.s",
        ".text\nnop ret\n",
        "<input>:2:5: error: unexpected trailing token: ret\nnop ret\n    ^\n",
    );
}

#[test]
fn snapshot_trailing_tokens_after_arm64_directives() {
    run_failure_snapshot(
        "section-trailing-token.s",
        ".section __TEXT,__text,regular,pure_instructions nop\n",
        "<input>:1:50: error: unexpected trailing token: nop\n.section __TEXT,__text,regular,pure_instructions nop\n                                                 ^\n",
    );
    run_failure_snapshot(
        "build-version-trailing-token.s",
        ".build_version macos, 11, 0 nop\n",
        "<input>:1:29: error: expected sdk_version after .build_version, got nop\n.build_version macos, 11, 0 nop\n                            ^\n",
    );
}

#[test]
fn snapshot_tabbed_trailing_arm64_token() {
    run_failure_snapshot(
        "tabbed-trailing-token.s",
        ".text\nnop\tret\n",
        "<input>:2:5: error: unexpected trailing token: ret\nnop\tret\n   \t^\n",
    );
}

#[test]
fn snapshot_arm64_directive_conversion_ranges() {
    run_failure_snapshot(
        "negative-common-size.s",
        ".comm _x,-1\n",
        "<input>:1:10: error: common size expression value -1 does not fit in u64\n.comm _x,-1\n         ^\n",
    );
    run_failure_snapshot(
        "oversized-byte.s",
        ".data\n.byte 17\n.byte 1,256\n.byte 34\n",
        "<input>:3:9: error: .byte expression value 256 is out of range for 8-bit data\n.byte 1,256\n        ^\n",
    );
}

#[test]
fn snapshot_unsupported_cfi_directive() {
    run_failure_snapshot(
        "unsupported-cfi.s",
        ".cfi_escape 0x1\n",
        "<input>:1:1: error: unsupported CFI directive '.cfi_escape' (supported: .cfi_startproc, .cfi_endproc, .cfi_def_cfa, .cfi_def_cfa_offset, .cfi_def_cfa_register, .cfi_offset, .cfi_restore, .cfi_adjust_cfa_offset)\n.cfi_escape 0x1\n^\n",
    );
}

#[test]
fn snapshot_unsupported_relocation_modifier_for_adrp() {
    run_failure_snapshot(
        "unsupported-reloc-modifier.s",
        ".text\nadrp x0, _foo@TLSGD\n",
        "<input>:2:20: error: unsupported relocation modifier '@TLSGD' for adrp symbol operand\nadrp x0, _foo@TLSGD\n                   ^\n",
    );
}

#[test]
fn snapshot_unsupported_register_offset_modifier() {
    run_failure_snapshot(
        "unsupported-register-offset.s",
        ".text\nldr x0, [x1, x2, ror #1]\n",
        "<input>:2:22: error: unsupported register offset modifier 'ror'\nldr x0, [x1, x2, ror #1]\n                     ^\n",
    );
}

#[test]
fn snapshot_unsupported_section() {
    run_failure_snapshot(
        "unsupported-section.s",
        ".section __TEXT,__foo\n.space 16\n",
        "<input>:1:1: error: unsupported section __TEXT,__foo (supported sections: __TEXT,__text, __TEXT,__cstring, __TEXT,__literal16, __TEXT,__const, __DATA,__data, __DATA,__const, __DATA,__thread_data, __DATA,__thread_vars, __DATA,__thread_bss, __DATA,__bss)\n.section __TEXT,__foo\n^\n",
    );
}

#[test]
fn snapshot_unsupported_section_attr() {
    run_failure_snapshot(
        "unsupported-section-attr.s",
        ".section __TEXT,__text,regular,garbage\nret\n",
        "<input>:1:39: error: unsupported section attributes for __TEXT,__text: garbage (supported attrs: regular, pure_instructions)\n.section __TEXT,__text,regular,garbage\n                                      ^\n",
    );
}

#[test]
fn snapshot_unsupported_loh_kind() {
    run_failure_snapshot(
        "unsupported-loh-kind.s",
        ".loh UnknownKind Lloh0\n",
        "<input>:1:18: error: unsupported .loh kind 'UnknownKind' (supported: AdrpAdd, AdrpLdr, AdrpLdrGot, AdrpLdrGotLdr)\n.loh UnknownKind Lloh0\n                 ^\n",
    );
}

#[test]
fn snapshot_unsupported_build_version_platform() {
    run_failure_snapshot(
        "unsupported-build-version-platform.s",
        ".build_version ios, 11, 0\n",
        "<input>:1:1: error: unsupported .build_version platform 'ios' (supported: macos)\n.build_version ios, 11, 0\n^\n",
    );
}

#[test]
fn snapshot_zerofill_requires_zero_fill_section() {
    run_failure_snapshot(
        "bad-zerofill-section.s",
        ".zerofill __DATA,__data,_bad,8,2\n",
        "<input>:1:1: error: .zerofill requires a zero-fill section, got __DATA,__data\n.zerofill __DATA,__data,_bad,8,2\n^\n",
    );
}

#[test]
fn snapshot_zerofill_alignment_too_large() {
    run_failure_snapshot(
        "zerofill-alignment-too-large.s",
        ".zerofill __DATA,__bss,_bad,8,31\n",
        "<input>:1:1: error: zerofill alignment power 31 too large (max 30)\n.zerofill __DATA,__bss,_bad,8,31\n^\n",
    );
}

#[test]
fn snapshot_tbss_alignment_too_large() {
    run_failure_snapshot(
        "tbss-alignment-too-large.s",
        ".tbss _tls_counter$tlv$init, 8, 31\n",
        "<input>:1:1: error: zerofill alignment power 31 too large (max 30)\n.tbss _tls_counter$tlv$init, 8, 31\n^\n",
    );
}

#[test]
fn snapshot_text_section_requires_regular_with_pure_instructions() {
    run_failure_snapshot(
        "text-section-missing-regular.s",
        ".section __TEXT,__text,pure_instructions\nret\n",
        "<input>:1:41: error: section __TEXT,__text requires 'regular' when using 'pure_instructions'\n.section __TEXT,__text,pure_instructions\n                                        ^\n",
    );
}

#[test]
fn snapshot_external_literal_target_requires_local_label() {
    run_failure_snapshot(
        "literal-local-label.s",
        ".text\nldr x0, _ext\n",
        "<input>:2:1: error: ldr literal target '_ext' requires an assembler-local label\nldr x0, _ext\n^\n",
    );
}

#[test]
fn snapshot_branch_target_must_be_aligned() {
    run_failure_snapshot(
        "misaligned-branch.s",
        ".text\nb done\n.byte 0\ndone:\nret\n",
        "<input>:2:1: error: branch offset 5 is not 4-byte aligned\nb done\n^\n",
    );
}

#[test]
fn snapshot_branch_target_must_be_in_range() {
    run_failure_snapshot(
        "branch-out-of-range.s",
        ".text\ncbz x0, done\n.space 1048576\ndone:\nret\n",
        "<input>:2:1: error: branch offset 1048580 is out of range for 19-bit immediate\ncbz x0, done\n^\n",
    );
}

#[test]
fn snapshot_invalid_arm64_immediates() {
    for (name, instruction, expected) in [
        (
            "add-immediate-out-of-range.s",
            "add x0, x1, #4097",
            "<input>:2:13: error: add/sub immediate 4097 is not encodable; expected magnitude 0..=4095 or a multiple of 4096 through 16773120\nadd x0, x1, #4097\n            ^\n",
        ),
        (
            "movz-immediate-out-of-range.s",
            "movz x0, #65536",
            "<input>:2:10: error: mov wide immediate must be in the range 0..=65535, got 65536\nmovz x0, #65536\n         ^\n",
        ),
        (
            "ldur-offset-out-of-range.s",
            "ldur x0, [x1, #256]",
            "<input>:2:15: error: memory offset 256 is out of range for a signed 9-bit immediate with scale 1\nldur x0, [x1, #256]\n              ^\n",
        ),
        (
            "branch-offset-out-of-range.s",
            "b #4294967296",
            "<input>:2:3: error: branch offset 4294967296 is out of range for a signed 26-bit immediate with scale 4\nb #4294967296\n  ^\n",
        ),
        (
            "svc-immediate-out-of-range.s",
            "svc #65536",
            "<input>:2:5: error: svc immediate must be in the range 0..=65535, got 65536\nsvc #65536\n    ^\n",
        ),
        (
            "logical-immediate-out-of-range.s",
            "and w0, w1, #8589934590",
            "<input>:2:13: error: 32-bit logical immediate must be in the range -4294967295..=4294967295, got 8589934590\nand w0, w1, #8589934590\n            ^\n",
        ),
        (
            "bitfield-lsb-out-of-range.s",
            "ubfiz x0, x1, #-1, #1",
            "<input>:2:15: error: ubfiz lsb -1 is out of range for 64-bit register\nubfiz x0, x1, #-1, #1\n              ^\n",
        ),
        (
            "bitfield-width-out-of-range.s",
            "bfi w0, w1, #0, #0",
            "<input>:2:17: error: bfi width must be at least 1\nbfi w0, w1, #0, #0\n                ^\n",
        ),
    ] {
        run_failure_snapshot(name, &format!(".text\n{}\n", instruction), expected);
    }
}

#[test]
fn snapshot_arm64_register_errors_point_to_the_operand() {
    for (name, instruction, expected) in [
        (
            "gp-register-width.s",
            "and x0, w1, x2",
            "<input>:2:9: error: and requires registers of the same width\nand x0, w1, x2\n        ^\n",
        ),
        (
            "fixed-register-width.s",
            "br w0",
            "<input>:2:4: error: br requires an x-register\nbr w0\n   ^\n",
        ),
        (
            "fp-register-width.s",
            "fadd d0, s1, s2",
            "<input>:2:10: error: fadd requires registers of the same width\nfadd d0, s1, s2\n         ^\n",
        ),
        (
            "add-immediate-base-width.s",
            "add x0, w1, #1",
            "<input>:2:9: error: add/sub immediate operands must have matching widths\nadd x0, w1, #1\n        ^\n",
        ),
        (
            "add-register-base-width.s",
            "add x0, w1, x2",
            "<input>:2:9: error: add/sub requires registers of the same width\nadd x0, w1, x2\n        ^\n",
        ),
        (
            "add-register-source-width.s",
            "add x0, x1, w2",
            "<input>:2:13: error: add/sub requires registers of the same width\nadd x0, x1, w2\n            ^\n",
        ),
        (
            "neg-register-width.s",
            "neg x0, w1",
            "<input>:2:9: error: neg requires registers of the same width\nneg x0, w1\n        ^\n",
        ),
        (
            "umull-destination-width.s",
            "umull w0, w1, w2",
            "<input>:2:7: error: umull destination must be an X register\numull w0, w1, w2\n      ^\n",
        ),
        (
            "umull-first-source-width.s",
            "umull x0, x1, w2",
            "<input>:2:11: error: umull sources must be W registers\numull x0, x1, w2\n          ^\n",
        ),
        (
            "umull-second-source-width.s",
            "umull x0, w1, x2",
            "<input>:2:15: error: umull sources must be W registers\numull x0, w1, x2\n              ^\n",
        ),
        (
            "extend-source-width.s",
            "sxtw x0, x1",
            "<input>:2:10: error: sxtw requires a w-register source\nsxtw x0, x1\n         ^\n",
        ),
        (
            "extend-destination-width.s",
            "uxtw w0, w1",
            "<input>:2:6: error: uxtw requires an x-register destination\nuxtw w0, w1\n     ^\n",
        ),
        (
            "extend-stack-pointer-destination.s",
            "sxtb sp, w1",
            "<input>:2:6: error: sxtb does not allow SP\nsxtb sp, w1\n     ^\n",
        ),
        (
            "immediate-zero-register-destination.s",
            "add xzr, x1, #1",
            "<input>:2:5: error: add/sub immediate forms require a register or sp destination, not xzr/wzr\nadd xzr, x1, #1\n    ^\n",
        ),
        (
            "immediate-zero-register-base.s",
            "add x0, xzr, #1",
            "<input>:2:9: error: add/sub immediate forms require a register or sp base, not xzr/wzr\nadd x0, xzr, #1\n        ^\n",
        ),
        (
            "extended-zero-register-destination.s",
            "add xzr, x1, w2, uxtw",
            "<input>:2:5: error: extended add/sub forms require a register or sp destination, not xzr/wzr\nadd xzr, x1, w2, uxtw\n    ^\n",
        ),
        (
            "extended-zero-register-base.s",
            "add x0, xzr, w2, uxtw",
            "<input>:2:9: error: extended add/sub forms require an x-register or sp base operand, not xzr/wzr\nadd x0, xzr, w2, uxtw\n        ^\n",
        ),
        (
            "ldrsw-destination-width.s",
            "ldrsw w0, [x1]",
            "<input>:2:7: error: ldrsw destination must be an x-register\nldrsw w0, [x1]\n      ^\n",
        ),
        (
            "pair-register-width.s",
            "ldp x0, w1, [x2]",
            "<input>:2:9: error: ldp/stp register pair must use matching register widths\nldp x0, w1, [x2]\n        ^\n",
        ),
        (
            "register-offset-stack-pointer.s",
            "ldr x0, [x1, sp]",
            "<input>:2:14: error: register offset does not allow sp\nldr x0, [x1, sp]\n             ^\n",
        ),
        (
            "wsp-data-operand.s",
            "ldr wsp, [x0]",
            "<input>:2:5: error: ldr/str data operand does not allow sp as a data register\nldr wsp, [x0]\n    ^\n",
        ),
        (
            "wsp-memory-base.s",
            "ldr x0, [wsp]",
            "<input>:2:10: error: ldr/str memory base requires an x-register or sp base\nldr x0, [wsp]\n         ^\n",
        ),
        (
            "wsp-register-offset.s",
            "ldr x0, [x1, wsp]",
            "<input>:2:14: error: register offset does not allow sp\nldr x0, [x1, wsp]\n             ^\n",
        ),
        (
            "wsp-atomic-data.s",
            "stlr wsp, [x0]",
            "<input>:2:6: error: stlr does not allow SP as a data register\nstlr wsp, [x0]\n     ^\n",
        ),
        (
            "wsp-atomic-base.s",
            "stlr w0, [wsp]",
            "<input>:2:11: error: stlr expects an [Xn] or [sp] base register\nstlr w0, [wsp]\n          ^\n",
        ),
        (
            "wsp-shift-operand.s",
            "lsl w0, wsp, #1",
            "<input>:2:9: error: lsl does not allow SP\nlsl w0, wsp, #1\n        ^\n",
        ),
        (
            "wsp-shift-register.s",
            "lsl w0, w1, wsp",
            "<input>:2:13: error: lsl register form does not allow SP\nlsl w0, w1, wsp\n            ^\n",
        ),
        (
            "wsp-fmov-source.s",
            "fmov s0, wsp",
            "<input>:2:10: error: fmov does not allow SP\nfmov s0, wsp\n         ^\n",
        ),
        (
            "wsp-fmov-destination.s",
            "fmov wsp, s0",
            "<input>:2:6: error: fmov does not allow SP\nfmov wsp, s0\n     ^\n",
        ),
        (
            "atomic-narrow-data-width.s",
            "ldaprb x0, [x1]",
            "<input>:2:8: error: ldaprb requires a w-register\nldaprb x0, [x1]\n       ^\n",
        ),
        (
            "atomic-rmw-width.s",
            "ldaddal x0, w1, [x2]",
            "<input>:2:13: error: ldaddal requires source and destination registers of the same width\nldaddal x0, w1, [x2]\n            ^\n",
        ),
        (
            "shift-source-width.s",
            "lsl x0, w1, #1",
            "<input>:2:9: error: lsl requires source and destination registers of the same width\nlsl x0, w1, #1\n        ^\n",
        ),
        (
            "shift-register-width.s",
            "lsl x0, x1, w2",
            "<input>:2:13: error: lsl register form requires registers of the same width\nlsl x0, x1, w2\n            ^\n",
        ),
        (
            "fmov-gp-source-width.s",
            "fmov d0, w1",
            "<input>:2:10: error: fmov dN, ... requires an x-register source\nfmov d0, w1\n         ^\n",
        ),
        (
            "fmov-fp-source-width.s",
            "fmov x0, s1",
            "<input>:2:10: error: fmov xN, ... requires a d-register source\nfmov x0, s1\n         ^\n",
        ),
        (
            "malformed-add-register.s",
            "add x0, x32, x2",
            "<input>:2:9: error: bad register 'x32'\nadd x0, x32, x2\n        ^\n",
        ),
        (
            "malformed-branch-register.s",
            "br x32",
            "<input>:2:4: error: bad register 'x32'\nbr x32\n   ^\n",
        ),
        (
            "malformed-memory-register.s",
            "ldr x0, [x32]",
            "<input>:2:10: error: bad register 'x32'\nldr x0, [x32]\n         ^\n",
        ),
        (
            "malformed-atomic-register.s",
            "stlr x32, [x0]",
            "<input>:2:6: error: bad register 'x32'\nstlr x32, [x0]\n     ^\n",
        ),
        (
            "malformed-conversion-register.s",
            "fcvtzs x32, d0",
            "<input>:2:8: error: bad register 'x32'\nfcvtzs x32, d0\n       ^\n",
        ),
        (
            "malformed-move-register.s",
            "mov x0, x32",
            "<input>:2:9: error: bad register 'x32'\nmov x0, x32\n        ^\n",
        ),
        (
            "malformed-shift-register.s",
            "lsl x0, x32, #1",
            "<input>:2:9: error: bad register 'x32'\nlsl x0, x32, #1\n        ^\n",
        ),
        (
            "malformed-fp-arithmetic-register.s",
            "fadd d0, d32, d2",
            "<input>:2:10: error: bad FP register 'd32'\nfadd d0, d32, d2\n         ^\n",
        ),
        (
            "malformed-fp-conversion-register.s",
            "fcvtzs x0, d32",
            "<input>:2:12: error: bad FP register 'd32'\nfcvtzs x0, d32\n           ^\n",
        ),
        (
            "malformed-fp-memory-register.s",
            "ldr d32, [x0]",
            "<input>:2:5: error: bad FP/SIMD register 'd32'\nldr d32, [x0]\n    ^\n",
        ),
        (
            "malformed-fmov-register.s",
            "fmov d0, d32",
            "<input>:2:10: error: bad FP register 'd32'\nfmov d0, d32\n         ^\n",
        ),
        (
            "malformed-fmov-destination.s",
            "fmov d32, d0",
            "<input>:2:6: error: bad FP register 'd32'\nfmov d32, d0\n     ^\n",
        ),
        (
            "fp-pair-register-width.s",
            "ldp d0, s1, [x2]",
            "<input>:2:9: error: ldp/stp FP register pair must use matching register widths\nldp d0, s1, [x2]\n        ^\n",
        ),
        (
            "malformed-simd-register.s",
            "ld1.s { v32 }[0], [x0]",
            "<input>:2:9: error: expected vector register, got 'v32'\nld1.s { v32 }[0], [x0]\n        ^\n",
        ),
        (
            "architectural-register-branch-target.s",
            "b d0",
            "<input>:2:3: error: expected label, got architectural register 'd0'\nb d0\n  ^\n",
        ),
    ] {
        run_failure_snapshot(name, &format!(".text\n{}\n", instruction), expected);
    }
}
