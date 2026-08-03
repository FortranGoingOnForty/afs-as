//! Forms afs-as used to silently encode as a *different* instruction
//! must now be rejected — matching gas, which rejects them too. These
//! cover instruction aliases, truncated fields, invalid fixed-register
//! roles, surplus operands, and malformed memory-address components.

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
    // RET's optional stack adjustment is a 16-bit immediate.
    "ret $65536",
    "ret $-32769",
    "ret %rax",
    "ret $8, $16",
    // Packed shuffle/shift immediates must not truncate before encoding.
    "pshuflw $-129, %xmm0, %xmm1",
    "pshuflw $256, %xmm0, %xmm1",
    "psrldq $-1, %xmm0",
    "psrldq $256, %xmm0",
];

const ZERO_OPERAND_MNEMONICS: &[&str] = &["cqto", "cqo", "cltd", "cdq", "syscall"];

const SCALE_WITHOUT_INDEX: &[u8] = &[1, 2, 4, 8];

const SETCC_MNEMONICS: &[&str] = &[
    "seto", "setno", "setb", "setc", "setnae", "setae", "setnb", "setnc", "sete", "setz", "setne",
    "setnz", "setbe", "setna", "seta", "setnbe", "sets", "setns", "setp", "setpe", "setnp",
    "setpo", "setl", "setnge", "setge", "setnl", "setle", "setng", "setg", "setnle",
];

fn fixed_arity_surplus_forms() -> Vec<String> {
    ZERO_OPERAND_MNEMONICS
        .iter()
        .map(|mnemonic| format!("{mnemonic} %rax"))
        .chain(
            ["pushq", "popq"]
                .into_iter()
                .map(|mnemonic| format!("{mnemonic} %rax, %rbx")),
        )
        .chain(
            SETCC_MNEMONICS
                .iter()
                .map(|mnemonic| format!("{mnemonic} %al, %bl")),
        )
        .collect()
}

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

#[test]
fn parser_rejects_every_scale_without_an_index() {
    for scale in SCALE_WITHOUT_INDEX {
        let source = format!(".text\n    movq (%rax,,{scale}), %rbx\n");
        let err = parse(&source).expect_err("scale without index parsed successfully");
        assert_eq!((err.line, err.col), (2, 5), "scale {scale}");
        assert_eq!(
            err.msg,
            format!("memory operand '(%rax,,{scale})' has a scale but no index"),
            "scale {scale}"
        );
    }
}

#[test]
fn encoder_rejects_a_semantic_scale_without_an_index() {
    let stmts = parse("movq (%rax), %rbx\n").expect("parse base-only control");
    let mut operands = match &stmts[0].stmt {
        Stmt::Insn { operands, .. } => operands.clone(),
        other => panic!("expected instruction, got {other:?}"),
    };
    let afs_as::x86::Operand::Mem(memory) = &mut operands[0] else {
        panic!("expected memory source operand");
    };
    memory.scale = 8;

    let err = encode("movq", &operands).expect_err("encoder discarded scale without index");
    assert_eq!(err, "memory scale requires an index register");
}

#[test]
fn valid_memory_scale_boundaries_keep_exact_encodings() {
    for (line, expected) in [
        ("movq (%rax), %rbx", &[0x48, 0x8b, 0x18][..]),
        ("movq (%rax,%rcx,8), %rbx", &[0x48, 0x8b, 0x1c, 0xc8][..]),
        (
            "movq (,%rcx,8), %rbx",
            &[0x48, 0x8b, 0x1c, 0xcd, 0x00, 0x00, 0x00, 0x00][..],
        ),
    ] {
        assert_eq!(encode_line(line).expect(line).bytes, expected, "{line}");
    }
}

#[test]
fn fixed_arity_encoders_require_exact_operand_counts() {
    for mnemonic in ZERO_OPERAND_MNEMONICS {
        assert!(
            encode_line(mnemonic).is_ok(),
            "{mnemonic}: rejected its operand-free form"
        );
        let extra = format!("{mnemonic} %rax");
        assert!(
            encode_line(&extra).is_err(),
            "{extra}: encoder silently discarded the operand"
        );
    }

    for mnemonic in ["pushq", "popq"] {
        assert!(
            encode_line(mnemonic).is_err(),
            "{mnemonic}: accepted a missing operand"
        );
        let exact = format!("{mnemonic} %rax");
        assert!(
            encode_line(&exact).is_ok(),
            "{exact}: rejected its supported unary form"
        );
        let extra = format!("{mnemonic} %rax, %rbx");
        assert!(
            encode_line(&extra).is_err(),
            "{extra}: encoder silently discarded the trailing operand"
        );
    }

    for mnemonic in SETCC_MNEMONICS {
        assert!(
            encode_line(mnemonic).is_err(),
            "{mnemonic}: accepted a missing operand"
        );
        let exact = format!("{mnemonic} %al");
        assert!(
            encode_line(&exact).is_ok(),
            "{exact}: rejected its supported unary form"
        );
        let extra = format!("{mnemonic} %al, %bl");
        assert!(
            encode_line(&extra).is_err(),
            "{extra}: encoder silently discarded the trailing operand"
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

#[test]
fn gas_rejects_every_scale_without_an_index() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_encode_rejects",
            "gas_rejects_every_scale_without_an_index",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_scale_without_index");
    for scale in SCALE_WITHOUT_INDEX {
        let source = tmp.path(&format!("_{scale}.s"));
        let object = tmp.path(&format!("_{scale}.o"));
        std::fs::write(&source, format!(".text\nmovq (%rax,,{scale}), %rbx\n"))
            .expect("write malformed memory operand");
        let output = Command::new(&gas)
            .arg("--64")
            .arg("-o")
            .arg(&object)
            .arg(&source)
            .output()
            .expect("run gas");
        assert!(
            !output.status.success(),
            "gas accepted scale {scale} without index"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("expecting scale factor"),
            "unexpected gas diagnostic for scale {scale}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn gas_rejects_fixed_arity_surplus_operands() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_encode_rejects",
            "gas_rejects_fixed_arity_surplus_operands",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_fixed_arity_reject");
    for (i, line) in fixed_arity_surplus_forms().iter().enumerate() {
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
            "gas accepted {:?} — expected operand-count rejection:\n{}",
            line,
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
