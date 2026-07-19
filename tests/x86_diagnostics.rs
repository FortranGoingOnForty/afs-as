use afs_as::x86::assemble::assemble_x86;
use std::process::Command;

const OVERSIZED_SECTION_CHILD: &str = "AFS_AS_OVERSIZED_SECTION_CHILD";

#[test]
fn semantic_errors_preserve_post_label_location() {
    let err = assemble_x86("label:   frobnicate %rax\n", 0)
        .expect_err("unknown instruction unexpectedly assembled");

    assert_eq!(err.line, Some(1));
    assert_eq!(err.col, Some(10));
    assert!(err.msg.contains("frobnicate"), "unexpected error: {err}");
    assert_eq!(err.to_string(), format!("1:10: error: {}", err.msg));
}

#[test]
fn deferred_size_errors_point_to_the_directive() {
    let src = ".text\nfoo:\n    ret\n    .size foo, .-missing\n";
    let err = assemble_x86(src, 0).expect_err("invalid .size unexpectedly assembled");

    assert_eq!(err.line, Some(4));
    assert_eq!(err.col, Some(5));
    assert!(
        err.msg.contains("base 'missing' not defined"),
        "unexpected error: {err}"
    );
}

#[test]
fn same_section_nobits_fixups_point_to_the_instruction() {
    let src = ".bss\n    leaq foo(%rip), %rax\nfoo:\n.zero 4\n";
    let err = assemble_x86(src, 0).expect_err("NOBITS fixup unexpectedly assembled");

    assert_eq!(err.line, Some(2));
    assert_eq!(err.col, Some(5));
    assert!(err.msg.contains("NOBITS"), "unexpected error: {err}");
}

#[test]
fn external_nobits_relocations_point_to_the_instruction() {
    let err = assemble_x86(".bss\n    call ext\n", 0)
        .expect_err("external NOBITS relocation unexpectedly assembled");

    assert_eq!(err.line, Some(2));
    assert_eq!(err.col, Some(5));
    assert!(
        err.msg.contains("cannot emit relocation") && err.msg.contains("NOBITS"),
        "unexpected error: {err}"
    );
}

#[test]
fn oversized_relocation_offsets_point_to_the_instruction() {
    let src = ".bss\n.space 4294967296\n    call ext\n";
    let err = assemble_x86(src, 0).expect_err("oversized relocation unexpectedly assembled");

    assert_eq!(err.line, Some(3));
    assert_eq!(err.col, Some(5));
    assert!(
        err.msg.contains("relocation offset") && err.msg.contains("exceeds u32"),
        "unexpected error: {err}"
    );
}

#[test]
fn oversized_initialized_sections_point_to_the_directive() {
    if let Ok(directive) = std::env::var(OVERSIZED_SECTION_CHILD) {
        let src = format!(".data\n    {directive} {}\n", i64::MAX);
        let err = assemble_x86(&src, 0).expect_err("oversized section unexpectedly assembled");

        assert_eq!(err.line, Some(2), "directive {directive}");
        assert_eq!(err.col, Some(5), "directive {directive}");
        assert!(
            err.msg
                .contains("initialized section is too large to materialize"),
            "directive {directive}: {err}"
        );
        return;
    }

    for directive in [".zero", ".space", ".skip"] {
        let output = Command::new(std::env::current_exe().expect("current test executable"))
            .arg("oversized_initialized_sections_point_to_the_directive")
            .args(["--exact", "--nocapture"])
            .env(OVERSIZED_SECTION_CHILD, directive)
            .output()
            .expect("run isolated assembler API test");
        assert!(
            output.status.success(),
            "directive {directive} exited {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
