#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::Path;
use std::process::Command;

use afs_as::assemble;

struct MutationCase {
    name: &'static str,
    src: String,
    snippet_fragment: &'static str,
}

fn replace_once(base: &str, from: &str, to: &str) -> String {
    let out = base.replacen(from, to, 1);
    assert_ne!(out, base, "mutation source not found: {}", from);
    out
}

fn base_instruction_source() -> &'static str {
    ".build_version macos, 11, 0 sdk_version 15, 5\n\
     .text\n\
     .globl _mut\n\
     _mut:\n\
       add x0, x1, x2\n\
       cmp x0, #1\n\
       csel x3, x4, x5, eq\n\
       ret\n"
}

fn base_reloc_source() -> &'static str {
    ".build_version macos, 11, 0 sdk_version 15, 5\n\
     .text\n\
     .globl _mut\n\
     _mut:\n\
     1:\n\
       cbz w0, 2f\n\
     Lloh0:\n\
       adrp x1, msg@PAGE\n\
     Lloh1:\n\
       add x1, x1, msg@PAGEOFF\n\
       ldr x2, [x1, #8]\n\
       ret\n\
     2:\n\
       ret\n\
     .loh AdrpAdd Lloh0, Lloh1\n\
     .section __TEXT,__cstring,cstring_literals\n\
     msg:\n\
       .asciz \"hello\"\n"
}

fn base_directive_source() -> &'static str {
    ".build_version macos, 11, 0 sdk_version 15, 5\n\
     .subsections_via_symbols\n\
     .section __TEXT,__const\n\
     value:\n\
       .quad 1\n\
     .section __TEXT,__cstring,cstring_literals\n\
     msg:\n\
       .asciz \"hello\"\n"
}

fn base_tls_source() -> &'static str {
    ".build_version macos, 11, 0 sdk_version 15, 5\n\
     .text\n\
     .globl _mut\n\
     _mut:\n\
       adrp x0, _tls_value@TLVPPAGE\n\
       ldr x0, [x0, _tls_value@TLVPPAGEOFF]\n\
       ret\n\
     .section __DATA,__thread_data,thread_local_regular\n\
     .p2align 2\n\
     _tls_value$tlv$init:\n\
       .long 5\n\
     .section __DATA,__thread_vars,thread_local_variables\n\
     .globl _tls_value\n\
     _tls_value:\n\
       .quad __tlv_bootstrap\n\
       .quad 0\n\
       .quad _tls_value$tlv$init\n"
}

fn mutation_cases() -> Vec<MutationCase> {
    vec![
        MutationCase {
            name: "bad_gp_register",
            src: replace_once(
                base_instruction_source(),
                "add x0, x1, x2",
                "add x0, x32, x2",
            ),
            snippet_fragment: "add x0, x32, x2",
        },
        MutationCase {
            name: "missing_comma",
            src: replace_once(base_instruction_source(), "add x0, x1, x2", "add x0, x1 x2"),
            snippet_fragment: "add x0, x1 x2",
        },
        MutationCase {
            name: "bad_immediate_hex",
            src: replace_once(base_instruction_source(), "cmp x0, #1", "cmp x0, #0xZZ"),
            snippet_fragment: "cmp x0, #0xZZ",
        },
        MutationCase {
            name: "bad_condition_code",
            src: replace_once(
                base_instruction_source(),
                "csel x3, x4, x5, eq",
                "csel x3, x4, x5, qq",
            ),
            snippet_fragment: "csel x3, x4, x5, qq",
        },
        MutationCase {
            name: "bad_numeric_label_ref",
            src: replace_once(base_reloc_source(), "cbz w0, 2f", "cbz w0, 2g"),
            snippet_fragment: "cbz w0, 2g",
        },
        MutationCase {
            name: "bad_pageoff_modifier",
            src: replace_once(
                base_reloc_source(),
                "add x1, x1, msg@PAGEOFF",
                "add x1, x1, msg@PAGEOF",
            ),
            snippet_fragment: "add x1, x1, msg@PAGEOF",
        },
        MutationCase {
            name: "missing_memory_bracket",
            src: replace_once(base_reloc_source(), "ldr x2, [x1, #8]", "ldr x2, [x1, #8"),
            snippet_fragment: "ldr x2, [x1, #8",
        },
        MutationCase {
            name: "bad_loh_arity",
            src: replace_once(
                base_reloc_source(),
                ".loh AdrpAdd Lloh0, Lloh1",
                ".loh AdrpAdd Lloh0",
            ),
            snippet_fragment: ".loh AdrpAdd Lloh0",
        },
        MutationCase {
            name: "bad_loh_separator",
            src: replace_once(
                base_reloc_source(),
                ".loh AdrpAdd Lloh0, Lloh1",
                ".loh AdrpAdd Lloh0 Lloh1",
            ),
            snippet_fragment: ".loh AdrpAdd Lloh0 Lloh1",
        },
        MutationCase {
            name: "malformed_section_directive",
            src: replace_once(
                base_directive_source(),
                ".section __TEXT,__const",
                ".section __TEXT",
            ),
            snippet_fragment: ".section __TEXT",
        },
        MutationCase {
            name: "bad_build_platform",
            src: replace_once(
                base_directive_source(),
                ".build_version macos, 11, 0 sdk_version 15, 5",
                ".build_version marsos, 11, 0 sdk_version 15, 5",
            ),
            snippet_fragment: ".build_version marsos, 11, 0 sdk_version 15, 5",
        },
        MutationCase {
            name: "bad_build_component",
            src: replace_once(
                base_directive_source(),
                ".build_version macos, 11, 0 sdk_version 15, 5",
                ".build_version macos, eleven, 0 sdk_version 15, 5",
            ),
            snippet_fragment: ".build_version macos, eleven, 0 sdk_version 15, 5",
        },
        MutationCase {
            name: "unterminated_string",
            src: replace_once(
                base_directive_source(),
                ".asciz \"hello\"",
                ".asciz \"hello",
            ),
            snippet_fragment: ".asciz \"hello",
        },
        MutationCase {
            name: "bad_local_label_name",
            src: replace_once(base_reloc_source(), "1:", "1x:"),
            snippet_fragment: "1x:",
        },
        MutationCase {
            name: "bad_tlvp_modifier",
            src: replace_once(
                base_tls_source(),
                "_tls_value@TLVPPAGEOFF",
                "_tls_value@TLVPOFF",
            ),
            snippet_fragment: "ldr x0, [x0, _tls_value@TLVPOFF]",
        },
        MutationCase {
            name: "bad_tls_initializer_expression",
            src: replace_once(
                base_tls_source(),
                ".quad _tls_value$tlv$init",
                ".quad _tls_value$tlv$init +",
            ),
            snippet_fragment: ".quad _tls_value$tlv$init +",
        },
    ]
}

fn system_as_rejects(path: &Path) {
    let output = Command::new("as")
        .arg("-o")
        .arg(path.with_extension("o"))
        .arg(path)
        .output()
        .expect("run system as");
    assert!(
        !output.status.success(),
        "system as unexpectedly accepted malformed input {}:\nstdout:\n{}\nstderr:\n{}",
        path.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn structured_mutations_fail_with_source_context() {
    if !common::native_macho_host("malformed_mutation", "structured_mutations_fail_with_source_context") {
        return;
    }
    for case in mutation_cases() {
        let paths = common::TempPaths::new(&format!("afs_mutation_{}", case.name));
        fs::write(&paths.asm, &case.src).expect("write mutation source");
        system_as_rejects(&paths.asm);

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            assemble::assemble_file(&paths.asm, &paths.obj)
        }));
        assert!(
            result.is_ok(),
            "panic for mutation {}\n---source---\n{}",
            case.name,
            case.src
        );
        let err = match result.unwrap() {
            Ok(_) => panic!("mutation {} unexpectedly assembled", case.name),
            Err(err) => err,
        };
        assert_eq!(err.path.as_deref(), Some(paths.asm.as_path()));
        assert!(err.line.is_some(), "missing line for {}", case.name);
        assert!(err.col.is_some(), "missing column for {}", case.name);
        assert!(
            err.snippet
                .as_deref()
                .is_some_and(|snippet| snippet.contains(case.snippet_fragment)),
            "missing snippet fragment for {}\nrendered:\n{}",
            case.name,
            err
        );
        assert!(
            !paths.obj.exists(),
            "mutation {} should not have produced {}\n---source---\n{}",
            case.name,
            paths.obj.display(),
            case.src
        );

        let rendered = err.to_string();
        assert!(rendered.contains(&paths.asm.display().to_string()));
        assert!(rendered.contains("error:"));
        assert!(rendered.contains(case.snippet_fragment));
        assert!(rendered.contains('^'));
    }
}
