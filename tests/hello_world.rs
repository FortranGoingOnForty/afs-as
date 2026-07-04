//! The ultimate Sprint 3 test: assemble, link, and run Hello World.
//!
//! This test proves the complete afs-as pipeline works end-to-end:
//! .s source → parse → encode → Mach-O .o → ld → running binary.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_path(ext: &str) -> std::path::PathBuf {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("afs_hw_{}_{}.{}", std::process::id(), id, ext))
}

fn assemble_link_run(asm: &str) -> (i32, String) {
    let s_path = temp_path("s");
    let o_path = temp_path("o");
    let bin_path = temp_path("out");

    std::fs::write(&s_path, asm).unwrap();

    // Assemble with our assembler.
    let obj = afs_as::assemble::assemble_source(asm).unwrap();
    let mut file = std::fs::File::create(&o_path).unwrap();
    afs_as::macho::write_macho(&obj, &mut file).unwrap();
    drop(file);

    // Link.
    let sdk = Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
        .unwrap();
    let sdk_path = String::from_utf8_lossy(&sdk.stdout).trim().to_string();

    let ld_status = Command::new("ld")
        .args([
            o_path.to_str().unwrap(),
            "-o",
            bin_path.to_str().unwrap(),
            "-lSystem",
            "-syslibroot",
            &sdk_path,
            "-e",
            "_main",
        ])
        .output()
        .expect("ld failed");

    if !ld_status.status.success() {
        let stderr = String::from_utf8_lossy(&ld_status.stderr);
        panic!("ld failed: {}", stderr);
    }

    // Run.
    let output = Command::new(bin_path.to_str().unwrap())
        .output()
        .expect("binary failed");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let code = output.status.code().unwrap_or(-1);

    (code, stdout)
}


/// Same policy as tests/common/corpus.rs::native_macho_host: this
/// suite drives the macOS arm64 system toolchain; skip loudly on any
/// other host.
fn native_macho_host(suite: &str, test: &str) -> bool {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return true;
    }
    eprintln!(
        "\nHARNESS_SKIP suite={} test={} count=1 reason=\"needs a macOS arm64 host toolchain\"",
        suite, test
    );
    false
}

#[test]
fn hello_world() {
    if !native_macho_host("hello_world", "hello_world") {
        return;
    }
    let (code, stdout) = assemble_link_run(
        "\
.global _main
.p2align 2

_main:
    mov x0, #1
    adrp x1, msg@PAGE
    add x1, x1, msg@PAGEOFF
    mov x2, #14
    mov x16, #4
    svc #0x80
    mov x0, #0
    mov x16, #1
    svc #0x80

.data
msg: .asciz \"Hello, World!\\n\"
",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "Hello, World!\n");
}

#[test]
fn exit_code_42() {
    if !native_macho_host("hello_world", "exit_code_42") {
        return;
    }
    let (code, _stdout) = assemble_link_run(
        "\
.global _main
.p2align 2

_main:
    mov x0, #42
    mov x16, #1
    svc #0x80
",
    );
    assert_eq!(code, 42);
}

#[test]
fn exit_code_0() {
    if !native_macho_host("hello_world", "exit_code_0") {
        return;
    }
    let (code, _stdout) = assemble_link_run(
        "\
.global _main
.p2align 2

_main:
    mov x0, #0
    mov x16, #1
    svc #0x80
",
    );
    assert_eq!(code, 0);
}

#[test]
fn arithmetic_and_exit() {
    if !native_macho_host("hello_world", "arithmetic_and_exit") {
        return;
    }
    // Compute 6 * 7 = 42, exit with that code.
    let (code, _stdout) = assemble_link_run(
        "\
.global _main
.p2align 2

_main:
    mov x0, #6
    mov x1, #7
    mul x0, x0, x1
    mov x16, #1
    svc #0x80
",
    );
    assert_eq!(code, 42);
}

#[test]
fn write_data_string() {
    if !native_macho_host("hello_world", "write_data_string") {
        return;
    }
    let (code, stdout) = assemble_link_run(
        "\
.global _main
.p2align 2

_main:
    // write(1, msg, 5)
    mov x0, #1
    adrp x1, msg@PAGE
    add x1, x1, msg@PAGEOFF
    mov x2, #5
    mov x16, #4
    svc #0x80
    // exit(0)
    mov x0, #0
    mov x16, #1
    svc #0x80

.data
msg: .asciz \"ARMF!\"
",
    );
    assert_eq!(code, 0);
    assert_eq!(stdout, "ARMF!");
}
