#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root(prefix: &str) -> PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), id));
    fs::create_dir_all(&root).expect("create temp root");
    root
}

fn readme_example_source() -> &'static str {
    ".text\n\
     .build_version macos, 11, 0 sdk_version 15, 5\n\
     .globl _main\n\
     .p2align 2\n\
     _main:\n\
       mov w0, #0\n\
       ret\n\
     .subsections_via_symbols\n"
}

#[test]
fn readme_file_examples_assemble_link_and_run() {
    let root = temp_root("afs_readme_file");
    let input = root.join("hello.s");
    let explicit_obj = root.join("hello-explicit.o");
    let derived_obj = root.join("hello.o");
    let explicit_bin = root.join("hello-explicit");
    let derived_bin = root.join("hello-derived");

    fs::write(&input, readme_example_source()).expect("write input");

    let output = Command::new(env!("CARGO_BIN_EXE_afs-as"))
        .arg(&input)
        .arg("-o")
        .arg(&explicit_obj)
        .output()
        .expect("run afs-as explicit");
    assert!(
        output.status.success(),
        "explicit assemble failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(env!("CARGO_BIN_EXE_afs-as"))
        .arg(&input)
        .output()
        .expect("run afs-as derived");
    assert!(
        output.status.success(),
        "derived assemble failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(derived_obj.exists(), "derived output was not created");

    common::link_with_system(&explicit_obj, &explicit_bin, "_main");
    let (code, _, stderr) = common::run_binary(&explicit_bin);
    assert_eq!(code, 0, "stderr:\n{}", stderr);

    common::link_with_system(&derived_obj, &derived_bin, "_main");
    let (code, _, stderr) = common::run_binary(&derived_bin);
    assert_eq!(code, 0, "stderr:\n{}", stderr);
}

#[test]
fn readme_stdin_stdout_example_assembles_links_and_runs() {
    let root = temp_root("afs_readme_stdio");
    let obj = root.join("hello.o");
    let bin = root.join("hello");

    let mut child = Command::new(env!("CARGO_BIN_EXE_afs-as"))
        .arg("-")
        .arg("-o")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn afs-as stdin/stdout");

    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin handle")
        .write_all(readme_example_source().as_bytes())
        .expect("write source to stdin");

    let output = child.wait_with_output().expect("wait for afs-as");
    assert!(
        output.status.success(),
        "stdin/stdout assemble failed\nstdout bytes: {}\nstderr:\n{}",
        output.stdout.len(),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::write(&obj, &output.stdout).expect("write stdout object");
    common::link_with_system(&obj, &bin, "_main");
    let (code, _, stderr) = common::run_binary(&bin);
    assert_eq!(code, 0, "stderr:\n{}", stderr);
}
