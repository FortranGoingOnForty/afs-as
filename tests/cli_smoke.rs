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

fn afs_as() -> Command {
    Command::new(env!("CARGO_BIN_EXE_afs-as"))
}

#[test]
fn help_flag_prints_usage_to_stdout() {
    let output = afs_as().arg("--help").output().expect("run afs-as --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("usage: afs-as <input.s> [-o <output.o>]"), "stdout:\n{}", stdout);
    assert!(stderr.is_empty(), "stderr:\n{}", stderr);
}

#[test]
fn version_flag_prints_version_to_stdout() {
    let output = afs_as().arg("--version").output().expect("run afs-as --version");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stdout.trim(), format!("afs-as {}", env!("CARGO_PKG_VERSION")));
    assert!(stderr.is_empty(), "stderr:\n{}", stderr);
}

#[test]
fn missing_input_exits_with_usage_error() {
    let output = afs_as().output().expect("run afs-as without args");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("afs-as: missing input file"), "stderr:\n{}", stderr);
    assert!(stderr.contains("usage: afs-as <input.s> [-o <output.o>]"), "stderr:\n{}", stderr);
}

#[test]
fn unknown_option_exits_with_usage_error() {
    let output = afs_as().arg("--wat").output().expect("run afs-as --wat");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("afs-as: unrecognized option '--wat'"), "stderr:\n{}", stderr);
    assert!(stderr.contains("Only a single input file is supported."), "stderr:\n{}", stderr);
}

#[test]
fn default_output_path_is_created_next_to_input() {
    let root = temp_root("afs_cli_default_output");
    let input = root.join("hello.s");
    let output = root.join("hello.o");
    fs::write(
        &input,
        ".text\n.globl _entry\n_entry:\nret\n",
    )
    .expect("write input");

    let result = afs_as().arg(&input).output().expect("run afs-as");
    assert!(result.status.success(), "stderr:\n{}", String::from_utf8_lossy(&result.stderr));
    assert!(output.exists(), "expected {} to exist", output.display());
    assert!(!fs::read(&output).expect("read output").is_empty());
}

#[test]
fn parse_errors_include_file_line_source_and_caret() {
    let root = temp_root("afs_cli_parse_error");
    let input = root.join("broken.s");
    fs::write(&input, ".text\nadd x0 x1, x2\n").expect("write input");

    let output = afs_as().arg(&input).output().expect("run afs-as");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("{}:2:", input.display())),
        "stderr:\n{}",
        stderr
    );
    assert!(stderr.contains("error:"), "stderr:\n{}", stderr);
    assert!(stderr.contains("add x0 x1, x2"), "stderr:\n{}", stderr);
    assert!(stderr.contains("^"), "stderr:\n{}", stderr);
}

#[test]
fn assembly_errors_include_file_line_source_and_caret() {
    let root = temp_root("afs_cli_asm_error");
    let input = root.join("broken.s");
    fs::write(&input, ".text\nldr x0, _ext\n").expect("write input");

    let output = afs_as().arg(&input).output().expect("run afs-as");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("{}:2:1: error:", input.display())),
        "stderr:\n{}",
        stderr
    );
    assert!(stderr.contains("assembler-local label"), "stderr:\n{}", stderr);
    assert!(stderr.contains("ldr x0, _ext"), "stderr:\n{}", stderr);
    assert!(stderr.contains("^"), "stderr:\n{}", stderr);
}
