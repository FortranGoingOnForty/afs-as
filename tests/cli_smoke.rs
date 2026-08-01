use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

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

#[cfg(target_os = "linux")]
fn full_device() -> Stdio {
    Stdio::from(
        fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")
            .expect("open /dev/full"),
    )
}

fn run_with_stdin(args: &[&str], input: &str) -> std::process::Output {
    let mut child = afs_as()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn afs-as");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for afs-as")
}

#[test]
fn help_flag_prints_usage_to_stdout() {
    let output = afs_as().arg("--help").output().expect("run afs-as --help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("usage: afs-as <input.s> [-o <output.o>]"),
        "stdout:\n{}",
        stdout
    );
    assert!(stdout.contains("exit status:"), "stdout:\n{}", stdout);
    assert!(
        stdout.contains("0            success"),
        "stdout:\n{}",
        stdout
    );
    assert!(stderr.is_empty(), "stderr:\n{}", stderr);
}

#[test]
fn version_flag_prints_version_to_stdout() {
    let output = afs_as()
        .arg("--version")
        .output()
        .expect("run afs-as --version");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stdout.trim(),
        format!("afs-as {}", env!("CARGO_PKG_VERSION"))
    );
    assert!(stderr.is_empty(), "stderr:\n{}", stderr);
}

#[cfg(target_os = "linux")]
#[test]
fn help_and_version_write_failures_exit_with_controlled_status() {
    for flag in ["--help", "--version"] {
        let output = afs_as()
            .arg(flag)
            .stdout(full_device())
            .output()
            .expect("run afs-as with failing stdout");

        assert_eq!(output.status.code(), Some(1), "flag {flag}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("afs-as: failed to write stdout:"),
            "flag {flag} stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains("panicked"),
            "flag {flag} stderr:\n{stderr}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn broken_stderr_preserves_usage_and_assembly_statuses() {
    let usage_status = afs_as()
        .arg("--wat")
        .stdout(Stdio::null())
        .stderr(full_device())
        .status()
        .expect("run usage error with failing stderr");
    assert_eq!(usage_status.code(), Some(2));

    let root = temp_root("afs_cli_broken_stderr");
    let missing_input = root.join("missing.s");
    let assembly_status = afs_as()
        .arg(&missing_input)
        .stdout(Stdio::null())
        .stderr(full_device())
        .status()
        .expect("run assembly error with failing stderr");
    assert_eq!(assembly_status.code(), Some(1));
    fs::remove_dir_all(&root).ok();
}

#[test]
fn missing_input_exits_with_usage_error() {
    let output = afs_as().output().expect("run afs-as without args");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("afs-as: missing input file"),
        "stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("usage: afs-as <input.s> [-o <output.o>]"),
        "stderr:\n{}",
        stderr
    );
}

#[test]
fn unknown_option_exits_with_usage_error() {
    let output = afs_as().arg("--wat").output().expect("run afs-as --wat");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("afs-as: unrecognized option '--wat'"),
        "stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Only a single input file is supported."),
        "stderr:\n{}",
        stderr
    );
}

#[test]
fn default_output_path_is_created_next_to_input() {
    let root = temp_root("afs_cli_default_output");
    let input = root.join("hello.s");
    let output = root.join("hello.o");
    fs::write(&input, ".text\n.globl _entry\n_entry:\nret\n").expect("write input");

    let result = afs_as().arg(&input).output().expect("run afs-as");
    assert!(
        result.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.exists(), "expected {} to exist", output.display());
    assert!(!fs::read(&output).expect("read output").is_empty());
}

#[cfg(unix)]
#[test]
fn non_utf8_input_and_output_paths_are_preserved() {
    let root = temp_root("afs_cli_non_utf8_paths");
    let input = root.join(OsString::from_vec(b"input-\xff.s".to_vec()));
    let output = root.join(OsString::from_vec(b"output-\xfe.o".to_vec()));
    fs::write(&input, ".text\n.globl f\nf:\nret\n").expect("write raw-byte input");

    let result = afs_as()
        .args(["--64", "-o"])
        .arg(&output)
        .arg(&input)
        .output()
        .expect("run afs-as with raw-byte paths");

    assert!(result.status.success(), "stderr bytes: {:?}", result.stderr);
    assert!(output.exists(), "raw-byte output path was not created");
    assert!(!fs::read(&output).expect("read raw-byte output").is_empty());
    fs::remove_dir_all(&root).ok();
}

#[cfg(unix)]
#[test]
fn non_utf8_dash_prefixed_path_requires_double_dash() {
    let root = temp_root("afs_cli_non_utf8_dash_path");
    let input_name = OsString::from_vec(b"-input-\xff.s".to_vec());
    let input = root.join(&input_name);
    let output = root.join(OsString::from_vec(b"-input-\xff.o".to_vec()));
    fs::write(&input, ".text\n.globl _entry\n_entry:\nret\n")
        .expect("write dash-prefixed raw-byte input");

    let rejected = afs_as()
        .current_dir(&root)
        .arg(&input_name)
        .output()
        .expect("run afs-as without --");
    assert_eq!(rejected.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("afs-as: unrecognized option"),
        "stderr bytes: {:?}",
        rejected.stderr
    );

    let accepted = afs_as()
        .current_dir(&root)
        .arg("--")
        .arg(&input_name)
        .output()
        .expect("run afs-as with --");
    assert!(
        accepted.status.success(),
        "stderr bytes: {:?}",
        accepted.stderr
    );
    assert!(
        output.exists(),
        "default raw-byte output path was not created"
    );
    fs::remove_dir_all(&root).ok();
}

#[test]
fn stdin_requires_explicit_output_path() {
    let output = run_with_stdin(&["-"], ".text\nret\n");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("afs-as: input '-' requires explicit -o <output.o> or -o -"),
        "stderr:\n{}",
        stderr
    );
}

#[test]
fn stdin_can_write_object_to_stdout() {
    let output = run_with_stdin(&["-", "-o", "-"], ".text\n.globl _entry\n_entry:\nret\n");
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]),
        "stdout bytes: {:?}",
        output.stdout.get(..8).unwrap_or(&output.stdout)
    );
}

#[test]
fn double_dash_allows_dash_prefixed_input_file() {
    let root = temp_root("afs_cli_double_dash");
    let input = root.join("--version.s");
    let output = root.join("--version.o");
    fs::write(&input, ".text\n.globl _entry\n_entry:\nret\n").expect("write input");

    let result = afs_as()
        .current_dir(&root)
        .args(["--", "--version.s"])
        .output()
        .expect("run afs-as with --");
    assert!(
        result.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.exists(), "expected {} to exist", output.display());
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
fn excessive_expression_depth_exits_with_a_located_error() {
    let unary = format!(".data\n.quad {}1\n", "-".repeat(100_000));
    let parenthesized = format!(
        ".data\n.quad {}1{}\n",
        "(".repeat(100_000),
        ")".repeat(100_000)
    );
    let assignment = format!(".set X,{}1\n", "1+".repeat(100_000));

    for (shape, source, expected_location) in [
        ("unary", unary, "<stdin>:2:263:"),
        ("parenthesized", parenthesized, "<stdin>:2:263:"),
        ("assignment", assignment, "<stdin>:1:521:"),
    ] {
        let output = run_with_stdin(&["-", "-o", "-"], &source);

        assert_eq!(
            output.status.code(),
            Some(1),
            "wrong exit status for {shape} expression"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(expected_location),
            "wrong source location for {shape} expression:\n{}",
            stderr
        );
        assert!(
            stderr.contains("expression exceeds maximum depth of 256"),
            "wrong diagnostic for {shape} expression:\n{}",
            stderr
        );
    }
}

#[test]
fn unsupported_directive_errors_include_file_line_source_and_caret() {
    let root = temp_root("afs_cli_unsupported_directive");
    let input = root.join("broken.s");
    fs::write(&input, ".text\n.unknown_directive\n").expect("write input");

    let output = afs_as().arg(&input).output().expect("run afs-as");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("{}:2:1: error:", input.display())),
        "stderr:\n{}",
        stderr
    );
    assert!(
        stderr.contains("unsupported directive '.unknown_directive'"),
        "stderr:\n{}",
        stderr
    );
    assert!(stderr.contains(".unknown_directive"), "stderr:\n{}", stderr);
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
    assert!(
        stderr.contains("assembler-local label"),
        "stderr:\n{}",
        stderr
    );
    assert!(stderr.contains("ldr x0, _ext"), "stderr:\n{}", stderr);
    assert!(stderr.contains("^"), "stderr:\n{}", stderr);
}

#[test]
fn dash_dash_64_writes_elf64_object() {
    let root = temp_root("afs_as_cli_elf");
    let src_path = root.join("in.s");
    let obj_path = root.join("out.o");
    fs::write(
        &src_path,
        ".text\n.globl f\n.type f, @function\nf:\n    movl $42, %eax\n    ret\n.size f, .-f\n",
    )
    .expect("write source");

    let output = afs_as()
        .args(["--64", "-o"])
        .arg(&obj_path)
        .arg(&src_path)
        .output()
        .expect("run afs-as --64");
    assert!(
        output.status.success(),
        "afs-as --64 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = fs::read(&obj_path).expect("read object");
    let obj = afs_as::elf::parse_elf(&bytes).expect("parse ELF output");
    assert_eq!(obj.machine, afs_as::elf::EM_X86_64);
    let text = obj.section_by_name(".text").expect(".text");
    assert_eq!(text.data, [0xb8, 0x2a, 0x00, 0x00, 0x00, 0xc3]);
    let f = obj.symbols.iter().find(|s| s.name == "f").expect("f sym");
    assert_eq!(f.size, 6);
    fs::remove_dir_all(&root).ok();
}

#[test]
fn dash_dash_64_errors_meet_diagnostic_contract() {
    let root = temp_root("afs_as_cli_elf_err");
    let half = 9_223_372_036_854_775_807u64;
    let cases = [
        (
            "parse",
            ".text\n    .unknown_directive\n".to_string(),
            2,
            "    .unknown_directive",
        ),
        (
            "semantic",
            ".text\n    frobnicate %rax\n".to_string(),
            2,
            "    frobnicate %rax",
        ),
        (
            "layout",
            format!(".bss\n.space {half}\n.space {half}\n    .zero 3\n"),
            4,
            "    .zero 3",
        ),
    ];

    for (name, source, line, snippet) in cases {
        let src_path = root.join(format!("{name}.s"));
        fs::write(&src_path, source).expect("write source");

        let output = afs_as()
            .args(["--64", "-o"])
            .arg(root.join(format!("{name}.o")))
            .arg(&src_path)
            .output()
            .expect("run afs-as --64");
        assert_eq!(output.status.code(), Some(1), "case {name}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("{}:{line}:5: error:", src_path.display())),
            "case {name} stderr:\n{stderr}"
        );
        assert!(
            stderr.lines().any(|rendered| rendered == snippet),
            "case {name} stderr:\n{stderr}"
        );
        assert!(
            stderr.lines().any(|rendered| rendered == "    ^"),
            "case {name} stderr:\n{stderr}"
        );
    }
    fs::remove_dir_all(&root).ok();
}

#[test]
fn dash_dash_64_stdin_errors_use_stdin_source_name() {
    let output = run_with_stdin(&["--64", "-", "-o", "-"], ".text\n\tfrobnicate %rax\n");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("<stdin>:2:2: error:"), "stderr:\n{stderr}");
    assert!(
        stderr.contains("\tfrobnicate %rax\n\t^"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn dash_dash_64_oversized_initialized_sections_report_errors() {
    let root = temp_root("afs_as_cli_elf_materialization");
    let size = i64::MAX;

    for (name, directive) in [("zero", ".zero"), ("space", ".space"), ("skip", ".skip")] {
        let src_path = root.join(format!("{name}.s"));
        let obj_path = root.join(format!("{name}.o"));
        let source = format!(".data\n.byte 1\n    {directive} {size}\n");
        fs::write(&src_path, &source).expect("write source");

        let output = afs_as()
            .args(["--64", "-o"])
            .arg(&obj_path)
            .arg(&src_path)
            .output()
            .expect("run afs-as --64");
        assert_eq!(output.status.code(), Some(1), "case {name}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("{}:3:5: error:", src_path.display())),
            "case {name} stderr:\n{stderr}"
        );
        assert!(
            stderr.contains(&format!(
                "initialized section is too large to materialize (1 + {size} bytes)"
            )),
            "case {name} stderr:\n{stderr}"
        );
        assert!(
            stderr.contains(&format!("    {directive} {size}\n    ^")),
            "case {name} stderr:\n{stderr}"
        );
        assert!(!obj_path.exists(), "case {name} left an output object");
    }

    fs::remove_dir_all(&root).ok();
}
