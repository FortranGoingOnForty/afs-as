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
        "<input>:1:1: error: unsupported section __TEXT,__foo (supported sections: __TEXT,__text, __TEXT,__cstring, __TEXT,__literal16, __TEXT,__const, __DATA,__data, __DATA,__thread_data, __DATA,__thread_vars, __DATA,__thread_bss, __DATA,__bss)\n.section __TEXT,__foo\n^\n",
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
fn snapshot_conflicting_build_version() {
    run_failure_snapshot(
        "conflicting-build-version.s",
        ".build_version macos, 11, 0\n.build_version macos, 14, 1\n",
        "<input>:2:1: error: conflicting .build_version directives: already saw BuildVersionDirective { platform: \"macos\", minos: VersionTriple { major: 11, minor: 0, patch: 0 }, sdk: None }, then BuildVersionDirective { platform: \"macos\", minos: VersionTriple { major: 14, minor: 1, patch: 0 }, sdk: None }\n.build_version macos, 14, 1\n^\n",
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
fn snapshot_attr_on_section_without_attr_surface() {
    run_failure_snapshot(
        "unsupported-section-attr-none.s",
        ".section __TEXT,__const,regular\n.quad 1\n",
        "<input>:1:32: error: section __TEXT,__const does not support explicit attributes\n.section __TEXT,__const,regular\n                               ^\n",
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
