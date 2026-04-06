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
    msg_fragment: &'static str,
    snippet_fragment: &'static str,
}

fn replace_once(base: &str, from: &str, to: &str) -> String {
    let out = base.replacen(from, to, 1);
    assert_ne!(out, base, "mutation source not found: {}", from);
    out
}

fn base_metadata_source() -> &'static str {
    ".build_version macos, 11, 0 sdk_version 15, 5\n\
     .subsections_via_symbols\n\
     .globl _neg_meta\n\
     .private_extern _hidden_neg\n\
     .weak_definition _local_neg\n\
     .weak_reference _weak_ext\n\
     .comm _common_neg, 8, 3\n\
     .section __TEXT,__text,regular,pure_instructions\n\
     .p2align 2\n\
     _neg_meta:\n\
     LlohP0:\n\
       adrp x0, msg_neg@PAGE\n\
     LlohP1:\n\
       add x0, x0, msg_neg@PAGEOFF\n\
       adrp x8, _ext@GOTPAGE\n\
       ldr x8, [x8, _ext@GOTPAGEOFF]\n\
       adrp x9, _tls_neg@TLVPPAGE\n\
       ldr x9, [x9, _tls_neg@TLVPPAGEOFF]\n\
       ret\n\
     .loh AdrpAdd LlohP0, LlohP1\n\
     .section __TEXT,__cstring,cstring_literals\n\
     msg_neg:\n\
       .asciz \"neg-meta\"\n\
     .section __DATA,__thread_data,thread_local_regular\n\
     _tls_neg$tlv$init:\n\
       .quad 5\n\
     .section __DATA,__thread_vars,thread_local_variables\n\
     .globl _tls_neg\n\
     _tls_neg:\n\
       .quad __tlv_bootstrap\n\
       .quad 0\n\
       .quad _tls_neg$tlv$init\n"
}

fn base_section_switch_source() -> &'static str {
    ".build_version macos, 11, 0 sdk_version 15, 5\n\
     .subsections_via_symbols\n\
     .section __TEXT,__text,regular,pure_instructions\n\
     .globl _neg_switch\n\
     .p2align 2\n\
     _neg_switch:\n\
       adrp x0, cstr0_neg@PAGE\n\
       add x0, x0, cstr0_neg@PAGEOFF\n\
       ret\n\
     .section __TEXT,__cstring,cstring_literals\n\
     cstr0_neg:\n\
       .asciz \"switch-neg\"\n\
     .section __TEXT,__const\n\
     .p2align 3\n\
     const0_neg:\n\
       .quad cstr0_neg\n\
     .section __TEXT,__literal16,16byte_literals\n\
     .p2align 4\n\
     lit0_neg:\n\
       .quad const0_neg\n\
       .quad cstr0_neg\n\
     .data\n\
     .p2align 3\n\
     data0_neg:\n\
       .quad const0_neg\n\
     .zerofill __DATA,__bss,_scratch_neg,16,4\n"
}

fn mutation_cases() -> Vec<MutationCase> {
    vec![
        MutationCase {
            name: "grouped_loh_arity",
            src: replace_once(
                base_metadata_source(),
                ".loh AdrpAdd LlohP0, LlohP1",
                ".loh AdrpAdd LlohP0",
            ),
            msg_fragment: ".loh AdrpAdd expects 2 labels, got 1",
            snippet_fragment: ".loh AdrpAdd LlohP0",
        },
        MutationCase {
            name: "grouped_bad_tlvp_modifier",
            src: replace_once(
                base_metadata_source(),
                "_tls_neg@TLVPPAGEOFF",
                "_tls_neg@TLVPOFF",
            ),
            msg_fragment: "unsupported relocation modifier '@TLVPOFF'",
            snippet_fragment: "ldr x9, [x9, _tls_neg@TLVPOFF]",
        },
        MutationCase {
            name: "grouped_bad_zerofill_target",
            src: replace_once(
                base_section_switch_source(),
                ".zerofill __DATA,__bss,_scratch_neg,16,4",
                ".zerofill __DATA,__data,_scratch_neg,16,4",
            ),
            msg_fragment: ".zerofill requires a zero-fill section",
            snippet_fragment: ".zerofill __DATA,__data,_scratch_neg,16,4",
        },
        MutationCase {
            name: "grouped_text_pure_without_regular",
            src: replace_once(
                base_section_switch_source(),
                ".section __TEXT,__text,regular,pure_instructions",
                ".section __TEXT,__text,pure_instructions",
            ),
            msg_fragment: "requires 'regular' when using 'pure_instructions'",
            snippet_fragment: ".section __TEXT,__text,pure_instructions",
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
fn grouped_mutations_fail_with_source_context_and_no_panics() {
    for case in mutation_cases() {
        let paths = common::TempPaths::new(&format!("afs_grouped_neg_{}", case.name));
        fs::write(&paths.asm, &case.src).expect("write grouped mutation source");
        system_as_rejects(&paths.asm);

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            assemble::assemble_file(&paths.asm, &paths.obj)
        }));
        assert!(
            result.is_ok(),
            "panic for grouped mutation {}\n---source---\n{}",
            case.name,
            case.src
        );

        let err = match result.unwrap() {
            Ok(_) => panic!("grouped mutation {} unexpectedly assembled", case.name),
            Err(err) => err,
        };

        assert_eq!(err.path.as_deref(), Some(paths.asm.as_path()));
        assert!(err.line.is_some(), "missing line for {}", case.name);
        assert!(err.col.is_some(), "missing column for {}", case.name);
        assert!(
            err.msg.contains(case.msg_fragment),
            "wrong error for {}\nexpected fragment: {:?}\nactual: {}",
            case.name,
            case.msg_fragment,
            err
        );
        assert!(
            err.snippet
                .as_deref()
                .is_some_and(|snippet| snippet.contains(case.snippet_fragment)),
            "missing snippet fragment for {}\nrendered:\n{}",
            case.name,
            err
        );
    }
}

#[test]
fn duplicate_build_version_uses_last_directive_like_system_as() {
    let src = "\
.build_version macos, 11, 0 sdk_version 15, 5
.build_version macos, 12, 0 sdk_version 15, 5
.section __TEXT,__text,regular,pure_instructions
.globl _dup_build
.p2align 2
_dup_build:
  ret
.subsections_via_symbols
";
    let paths = common::TempPaths::new("afs_build_version_override");
    fs::write(&paths.asm, src).expect("write build-version override source");

    common::assemble_with_ours(src, &paths.obj);
    common::assemble_with_system(&paths.asm, &paths.ref_obj);

    assert_eq!(
        fs::read(&paths.obj).expect("read afs-as object"),
        fs::read(&paths.ref_obj).expect("read system object")
    );
    assert_eq!(
        normalize_tool_output(&common::object_load_commands(&paths.obj)),
        normalize_tool_output(&common::object_load_commands(&paths.ref_obj))
    );
}

fn normalize_tool_output(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_end().ends_with(".o:") && !line.trim_end().ends_with(":"))
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}
