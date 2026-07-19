//! Audit A2-A4: forms afs-as used to silently encode as a *different*
//! instruction must now be rejected — matching gas, which rejects them
//! too. A2: `movhlps` with a memory operand became movlps. A3: a
//! base+disp displacement past the 32-bit signed range truncated. A4:
//! `%ch/%dh/%bh` as a variable shift count became `%cl`. Qword TEST
//! immediates outside the signed imm32 encoding range also truncated.

#[path = "common/elf.rs"]
mod celf;

use std::process::Command;

use afs_as::x86::encode::encode;
use afs_as::x86::parse::{parse, Stmt};

/// Valid parses that must not encode: each would otherwise assemble as a
/// different instruction than written.
const REJECTED: &[&str] = &[
    // A2: 0F 12 with a memory operand is movlps, a different instruction.
    "movhlps (%rax), %xmm0",
    "movhlps -8(%rbp), %xmm3",
    // A3: base+disp displacement past the 32-bit signed range.
    "movq 4294967296(%rax), %rbx",
    "movl -2147483649(%rdx), %ecx",
    // A4: only %cl is a valid variable shift count.
    "shlq %ch, %rax",
    "shrl %dh, %ebx",
    "sarq %bh, %rcx",
    // TEST r/m64, imm32 sign-extends its immediate.
    "testq $2147483648, %rax",
    "testq $-2147483649, %r11",
    "testq $4294967296, %rax",
];

fn encode_line(line: &str) -> afs_as::x86::encode::EncodeResult {
    let stmts = parse(&format!("{}\n", line)).unwrap_or_else(|e| panic!("{}: parse: {}", line, e));
    let (m, ops) = match &stmts[0].stmt {
        Stmt::Insn { mnemonic, operands } => (mnemonic.clone(), operands.clone()),
        other => panic!("{}: not an insn: {:?}", line, other),
    };
    encode(&m, &ops)
}

#[test]
fn encoder_rejects_silently_wrong_forms() {
    for line in REJECTED {
        assert!(
            encode_line(line).is_err(),
            "{}: encoder accepted a form it must reject (would emit the wrong instruction)",
            line
        );
    }
}

/// Cross-check that our rejection is correct, not an over-restriction:
/// gas rejects every one of these too.
#[test]
fn gas_also_rejects_those_forms() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_encode_rejects",
            "gas_also_rejects_those_forms",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_reject");
    for (i, line) in REJECTED.iter().enumerate() {
        let src = tmp.path(&format!("_{}.s", i));
        let obj = tmp.path(&format!("_{}.o", i));
        std::fs::write(&src, format!(".text\n{}\n", line)).unwrap();
        let out = Command::new(&gas)
            .arg("--64")
            .arg("-o")
            .arg(&obj)
            .arg(&src)
            .output()
            .expect("run gas");
        assert!(
            !out.status.success(),
            "gas accepted {:?} — expected rejection:\n{}",
            line,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
