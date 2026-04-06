use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root(prefix: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), id));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

#[test]
fn section_attrs_match_system_as() {
    let root = temp_root("afs_section_attr_surface");
    let input = root.join("section-attrs.s");
    let ours = root.join("ours.o");
    let system = root.join("system.o");

    let src = r#"
.section __TEXT,__text,regular,pure_instructions
.build_version macos, 11, 0 sdk_version 15, 5
.globl _use_sections
.p2align 2
_use_sections:
    ret

.section __TEXT,__cstring,regular
msg:
    .asciz "hello"

.section __TEXT,__const,regular
.p2align 3
const_word:
    .quad 42

.section __TEXT,__literal16,regular
.p2align 4
lit16:
    .quad 1
    .quad 2

.section __DATA,__thread_data,thread_local_variables
.p2align 2
_tls_value$tlv$init:
    .long 5

.section __DATA,__thread_vars,thread_local_regular
.globl _tls_value
_tls_value:
    .quad __tlv_bootstrap
    .quad 0
    .quad _tls_value$tlv$init

.subsections_via_symbols
"#;
    fs::write(&input, src).expect("write input");

    let output = Command::new(env!("CARGO_BIN_EXE_afs-as"))
        .arg(&input)
        .arg("-o")
        .arg(&ours)
        .output()
        .expect("run afs-as");
    assert!(
        output.status.success(),
        "afs-as failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new("as")
        .arg("-o")
        .arg(&system)
        .arg(&input)
        .output()
        .expect("run system as");
    assert!(
        output.status.success(),
        "system as failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let status = Command::new("cmp")
        .arg("-s")
        .arg(&ours)
        .arg(&system)
        .status()
        .expect("run cmp");
    assert!(status.success(), "objects differ");
}
