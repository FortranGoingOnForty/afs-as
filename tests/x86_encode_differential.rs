//! x14: encoder byte-differential against gas, one instruction at a
//! time. Each case is assembled by gas into an object, the .text
//! bytes are lifted with the x13 ELF reader, and the encoder's bytes
//! must match exactly. Reloc-carrying cases also compare the
//! relocation tuple.

#[path = "common/elf.rs"]
mod celf;

use afs_as::elf::parse_elf;
use afs_as::x86::encode::encode;
use afs_as::x86::parse::{parse, Stmt};

/// Every implemented instruction form. Grow this list with every
/// encoder family — a form without a differential case here is not
/// considered implemented.
const CASES: &[&str] = &[
    // moves
    "movq %rax, %rbx",
    "movq %r15, %rsp",
    "movl %eax, %r9d",
    "movb %al, %dil",
    "movw %ax, %cx",
    "movq %rax, -8(%rbp)",
    "movq 16(%rsp), %rdi",
    "movl %esi, (%rax,%rcx,1)",
    "movl (%rbx,%rdx,4), %eax",
    "movq %rcx, (%r12)",
    "movq (%r13), %rax",
    "movb $1, %al",
    "movl $305419896, %edx",
    "movq $-1, %rax",
    "movq $2147483647, %r11",
    "movl $0, -4(%rbp)",
    "movq $0, (%rax)",
    "movabsq $-9223372036854775808, %rax",
    "movabsq $81985529216486895, %rdx",
    // lea
    "leaq -24(%rbp), %rdi",
    "leaq (%rax,%rbx,8), %rcx",
    "leaq str(%rip), %rdx",
    "leaq tbl+16(%rip), %r8",
    // arith group
    "addq %rax, %rbx",
    "addq $8, %rsp",
    "addl $1000, %eax",
    "addq $127, %rcx",
    "addq $128, %rcx",
    "subq $32, %rsp",
    "subl %ecx, %edx",
    "cmpq %rax, %rbx",
    "cmpl $0, %eax",
    "cmpq $-1, %r10",
    "cmpl $5, -12(%rbp)",
    "andl $255, %r11d",
    "andq %rdx, %rax",
    "orl %eax, %ecx",
    "orq %r9, %r10",
    "xorl %eax, %eax",
    "xorq %rax, %rax",
    "andb $1, %cl",
    // test
    "testl %eax, %eax",
    "testq %rdi, %rdi",
    "testb %al, %al",
    "testl $4, %edx",
    // push/pop/ret
    "pushq %rbp",
    "pushq %r15",
    "popq %rbp",
    "popq %r12",
    "ret",
    // widening
    "movzbl %al, %eax",
    "movzbl %dil, %r10d",
    "movzwl %cx, %edx",
    "movsbl %bl, %esi",
    "movswl %ax, %eax",
    "movslq %eax, %rax",
    "movslq %r9d, %r13",
    "movzbl -1(%rax), %ecx",
    "movslq (%rbx), %rdx",
    // setcc
    "sete %al",
    "setne %cl",
    "setg %dil",
    "setae %r8b",
    // mul/div family
    "imulq %rbx, %rax",
    "imulq -8(%rbp), %rcx",
    "imull %edx, %eax",
    "idivq %rcx",
    "idivl %esi",
    "divq %r8",
    "negq %rax",
    "negl %r9d",
    "notq %rdx",
    "cqto",
    "cltd",
    // shifts
    "shlq $3, %rax",
    "shrq $1, %rdx",
    "sarl $31, %eax",
    "shlq %cl, %r10",
    "shrl %cl, %ebx",
    // calls/jumps
    "call ext_func",
    "jmp *%r11",
    "callq *%rax",
    "jmp *16(%rcx)",
    "syscall",
];

fn our_bytes(line: &str) -> (Vec<u8>, Option<afs_as::x86::encode::InsnReloc>) {
    let stmts = parse(&format!("{}\n", line)).unwrap_or_else(|e| panic!("{}: {}", line, e));
    let (m, ops) = match &stmts[0].stmt {
        Stmt::Insn { mnemonic, operands } => (mnemonic.clone(), operands.clone()),
        other => panic!("{}: not an insn: {:?}", line, other),
    };
    let enc = encode(&m, &ops).unwrap_or_else(|e| panic!("{}: encode: {}", line, e));
    assert!(enc.label_fix.is_none(), "{}: unexpected label fix", line);
    (enc.bytes, enc.reloc)
}

#[test]
fn encoder_matches_gas_bytes() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_encode_differential",
            "encoder_matches_gas_bytes",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_encdiff");
    let mut failures = Vec::new();
    for (i, line) in CASES.iter().enumerate() {
        let src = format!(".text\n{}\n", line);
        let src_path = tmp.path(&format!("_{}.s", i));
        let obj_path = tmp.path(&format!("_{}.o", i));
        std::fs::write(&src_path, &src).unwrap();
        celf::assemble_with_gas(&gas, &src_path, &obj_path);
        let obj = parse_elf(&std::fs::read(&obj_path).unwrap()).expect("lift gas object");
        let gas_text = &obj.section_by_name(".text").expect(".text").data;

        let (ours, our_reloc) = our_bytes(line);
        if &ours != gas_text {
            failures.push(format!(
                "{}\n  gas:  {:02x?}\n  ours: {:02x?}",
                line, gas_text, ours
            ));
            continue;
        }
        // Reloc tuples must agree when present on either side.
        let text = obj.section_by_name(".text").unwrap();
        match (&our_reloc, text.relas.as_slice()) {
            (None, []) => {}
            (Some(r), [g]) => {
                let gsym = &obj.symbols[g.symbol].name;
                if (r.offset as u64, &r.sym, r.r_type, r.addend)
                    != (g.offset, gsym, g.r_type, g.addend)
                {
                    failures.push(format!(
                        "{}\n  reloc gas:  off={} sym={} type={} addend={}\n  reloc ours: off={} sym={} type={} addend={}",
                        line, g.offset, gsym, g.r_type, g.addend, r.offset, r.sym, r.r_type, r.addend
                    ));
                }
            }
            (ours_r, gas_r) => failures.push(format!(
                "{}: reloc count mismatch ours={:?} gas={}",
                line,
                ours_r.is_some() as u8,
                gas_r.len()
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases diverge from gas:\n\n{}",
        failures.len(),
        CASES.len(),
        failures.join("\n\n")
    );
}
