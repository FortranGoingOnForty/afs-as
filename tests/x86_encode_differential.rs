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
    // gas widens movq to the movabs form past i32
    "movq $789750225429331968, %rsi",
    "movq $9223372036854775807, %rdx",
    "movq $-9223372036854775808, %r9",
    "movq $2147483648, %rax",
    "movq $-2147483649, %rcx",
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
    // width-reinterpreted immediates pick the sign-extended 83 form
    "orw $65535, %ax",
    "xorw $65535, %bx",
    "cmpw $65408, %cx",
    "addl $4294967295, %edx",
    "sbbw $65535, %r9w",
    // carry chain (i128 lowering)
    "sbbq $0, %rdx",
    "sbbq -80(%rbp), %rdx",
    "sbbq %rax, %rax",
    "adcq $0, %rcx",
    "adcq %r8, %r9",
    "adcl %eax, %edx",
    // test
    "testl %eax, %eax",
    "testq %rdi, %rdi",
    "testb %al, %al",
    "testl $4, %edx",
    // accumulator short forms A8/A9 (audit A6): gas uses these for
    // al/ax/eax/rax, not the F6/F7 group the non-accumulator forms take.
    "testb $5, %al",
    "testw $5, %ax",
    "testl $5, %eax",
    "testq $5, %rax",
    "testq $2147483647, %rax",
    "testq $-2147483648, %r11",
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
    "bsrl %eax, %edx",
    "bsrq %r9, %rax",
    "bsrl -4(%rbp), %ecx",
    "bsfl %ebx, %eax",
    "bsfq %rcx, %r11",
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
    // SSE scalar moves + arithmetic
    "movss %xmm0, %xmm1",
    "movss (%rax), %xmm2",
    "movss %xmm3, -4(%rbp)",
    "movsd %xmm8, %xmm14",
    "movsd 8(%rsp), %xmm0",
    "movsd %xmm5, (%r12)",
    "movss lit(%rip), %xmm7",
    "addss %xmm1, %xmm0",
    "addsd %xmm9, %xmm10",
    "subss %xmm2, %xmm3",
    "subsd (%rbx), %xmm4",
    "mulss %xmm0, %xmm15",
    "mulsd -16(%rbp), %xmm6",
    "divss %xmm1, %xmm2",
    "divsd %xmm3, %xmm4",
    "minsd %xmm1, %xmm0",
    "maxss %xmm2, %xmm5",
    "sqrtss %xmm0, %xmm0",
    "sqrtsd %xmm7, %xmm8",
    "ucomiss %xmm1, %xmm0",
    "ucomisd %xmm11, %xmm12",
    "cvtss2sd %xmm0, %xmm1",
    "cvtsd2ss %xmm2, %xmm3",
    // conversions
    "cvtsi2ssl %eax, %xmm0",
    "cvtsi2sdl %r9d, %xmm5",
    "cvtsi2sdq %rax, %xmm1",
    "cvttss2sil %xmm0, %eax",
    "cvttsd2sil %xmm3, %r10d",
    "cvttsd2siq %xmm2, %rdx",
    // GP <-> xmm
    "movd %eax, %xmm0",
    "movd %xmm1, %ecx",
    "movq %rax, %xmm2",
    "movq %xmm3, %rdx",
    // packed float
    "movaps %xmm0, %xmm1",
    "movaps (%rax), %xmm2",
    "movaps %xmm3, 16(%rsp)",
    "movups (%rdi,%rcx,1), %xmm4",
    "movups %xmm5, (%rsi)",
    "addps %xmm1, %xmm0",
    "addpd %xmm2, %xmm3",
    "mulps (%rax), %xmm6",
    "mulpd %xmm8, %xmm9",
    "minps %xmm0, %xmm1",
    "minpd %xmm2, %xmm3",
    "maxps %xmm4, %xmm5",
    "maxpd %xmm6, %xmm7",
    "andps %xmm0, %xmm1",
    "andpd %xmm10, %xmm11",
    "xorps %xmm0, %xmm0",
    "xorpd %xmm1, %xmm1",
    "sqrtps %xmm2, %xmm3",
    "sqrtpd %xmm4, %xmm5",
    "unpcklps %xmm1, %xmm0",
    "unpcklpd %xmm9, %xmm0",
    "unpcklpd %xmm3, %xmm7",
    // packed integer
    "movdqa %xmm0, %xmm1",
    "movdqa (%rax), %xmm2",
    "movdqa %xmm3, 32(%rbp)",
    "paddd %xmm1, %xmm0",
    "pand %xmm2, %xmm3",
    "pandn %xmm4, %xmm5",
    "por %xmm6, %xmm7",
    "pcmpgtd %xmm8, %xmm9",
    "pmuludq %xmm4, %xmm7",
    "punpcklqdq %xmm0, %xmm1",
    "punpcklqdq %xmm8, %xmm3",
    "paddq %xmm1, %xmm0",
    "paddq (%rax), %xmm6",
    "psubq %xmm2, %xmm3",
    "pxor %xmm4, %xmm4",
    "pcmpeqd %xmm5, %xmm6",
    "divps %xmm1, %xmm2",
    "divpd %xmm3, %xmm4",
    "movhlps %xmm1, %xmm3",
    // shuffles with imm8
    "pshufd $245, %xmm3, %xmm5",
    "pshufd $0, %xmm0, %xmm1",
    "pshufd $216, %xmm9, %xmm10",
    // RIP-relative source + trailing imm8: the PC32 addend must count the
    // imm8 byte (gas emits sym-5 / sym+off-5, not sym-4).
    "pshufd $3, tbl(%rip), %xmm0",
    "pshufd $216, lut+16(%rip), %xmm9",
    "shufps $136, %xmm2, %xmm4",
    "cmpps $1, %xmm5, %xmm7",
    "cmpps $2, %xmm8, %xmm10",
    "cmppd $1, %xmm2, %xmm9",
    "cmppd $6, %xmm0, %xmm1",
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
