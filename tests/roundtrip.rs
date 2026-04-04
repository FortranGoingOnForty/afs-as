//! Round-trip tests: parse assembly text → encode → compare with system `as`.
//!
//! These tests prove that our parser and encoder agree with Apple's assembler
//! end-to-end. For each test, we:
//! 1. Parse the assembly text with our parser
//! 2. Encode each instruction with our encoder
//! 3. Assemble the same text with Apple `as`
//! 4. Compare byte-for-byte

use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use afs_as::parse::{self, Stmt};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Assemble with Apple `as` and return the code bytes.
fn system_assemble(asm: &str) -> Vec<u8> {
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir();
    let s_path = dir.join(format!("afs_rt_{}.s", id));
    let o_path = dir.join(format!("afs_rt_{}.o", id));

    let mut f = std::fs::File::create(&s_path).unwrap();
    write!(f, "{}", asm).unwrap();
    drop(f);

    let status = Command::new("as")
        .args(["-o", o_path.to_str().unwrap(), s_path.to_str().unwrap()])
        .status()
        .expect("run as");
    assert!(status.success(), "as failed for input");

    let output = Command::new("otool")
        .args(["-t", o_path.to_str().unwrap()])
        .output()
        .expect("run otool");
    let text = String::from_utf8_lossy(&output.stdout);

    // Parse hex words from otool output.
    let mut bytes = Vec::new();
    for line in text.lines().filter(|l| l.starts_with('0')) {
        for hex in line.split_whitespace().skip(1) {
            let word = u32::from_str_radix(hex, 16).unwrap();
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes
}

/// Parse + encode with our tools and return code bytes.
fn our_assemble(asm: &str) -> Vec<u8> {
    let stmts = parse::parse(asm).unwrap();
    let mut bytes = Vec::new();
    for stmt in &stmts {
        if let Stmt::Instruction(inst) = stmt {
            let word = inst.encode();
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes
}

/// Compare our assembly output against the system assembler.
fn roundtrip(asm: &str) {
    let ours = our_assemble(asm);
    let theirs = system_assemble(asm);
    assert_eq!(
        ours.len(), theirs.len(),
        "byte count mismatch: ours={} theirs={}\n---input---\n{}",
        ours.len(), theirs.len(), asm
    );
    for (i, (a, b)) in ours.iter().zip(theirs.iter()).enumerate() {
        assert_eq!(
            a, b,
            "byte mismatch at offset {}: ours=0x{:02X} theirs=0x{:02X}\n---input---\n{}",
            i, a, b, asm
        );
    }
}

// ---- Single instruction round-trips ----

#[test] fn rt_add_reg()    { roundtrip(".text\nadd x0, x1, x2\n"); }
#[test] fn rt_sub_reg()    { roundtrip(".text\nsub x10, x11, x12\n"); }
#[test] fn rt_add_w()      { roundtrip(".text\nadd w3, w4, w5\n"); }
#[test] fn rt_add_imm()    { roundtrip(".text\nadd x0, x1, #100\n"); }
#[test] fn rt_sub_imm()    { roundtrip(".text\nsub x3, x4, #200\n"); }
#[test] fn rt_mul()        { roundtrip(".text\nmul x0, x1, x2\n"); }
#[test] fn rt_sdiv()       { roundtrip(".text\nsdiv x3, x4, x5\n"); }
#[test] fn rt_udiv()       { roundtrip(".text\nudiv x6, x7, x8\n"); }
#[test] fn rt_and()        { roundtrip(".text\nand x0, x1, x2\n"); }
#[test] fn rt_orr()        { roundtrip(".text\norr x0, x1, x2\n"); }
#[test] fn rt_eor()        { roundtrip(".text\neor x0, x1, x2\n"); }
#[test] fn rt_cmp_reg()    { roundtrip(".text\ncmp x5, x6\n"); }
#[test] fn rt_cmp_imm()    { roundtrip(".text\ncmp x5, #42\n"); }
#[test] fn rt_tst()        { roundtrip(".text\ntst x0, x1\n"); }
#[test] fn rt_movz()       { roundtrip(".text\nmovz x0, #0x1234\n"); }
#[test] fn rt_movz_lsl16() { roundtrip(".text\nmovz x0, #0x5678, lsl #16\n"); }
#[test] fn rt_movk()       { roundtrip(".text\nmovk x0, #0xABCD, lsl #32\n"); }
#[test] fn rt_lsl()        { roundtrip(".text\nlsl x0, x1, #7\n"); }
#[test] fn rt_lsr()        { roundtrip(".text\nlsr x0, x1, #15\n"); }
#[test] fn rt_asr()        { roundtrip(".text\nasr x0, x1, #31\n"); }
#[test] fn rt_b()          { roundtrip(".text\nb #20\n"); }
#[test] fn rt_bl()         { roundtrip(".text\nbl #40\n"); }
#[test] fn rt_b_eq()       { roundtrip(".text\nb.eq #24\n"); }
#[test] fn rt_b_ne()       { roundtrip(".text\nb.ne #32\n"); }
#[test] fn rt_b_lt()       { roundtrip(".text\nb.lt #16\n"); }
#[test] fn rt_b_ge()       { roundtrip(".text\nb.ge #8\n"); }
#[test] fn rt_b_gt()       { roundtrip(".text\nb.gt #12\n"); }
#[test] fn rt_b_le()       { roundtrip(".text\nb.le #20\n"); }
#[test] fn rt_cbz()        { roundtrip(".text\ncbz x5, #16\n"); }
#[test] fn rt_cbnz()       { roundtrip(".text\ncbnz x10, #24\n"); }
#[test] fn rt_ret()        { roundtrip(".text\nret\n"); }
#[test] fn rt_br()         { roundtrip(".text\nbr x8\n"); }
#[test] fn rt_blr()        { roundtrip(".text\nblr x9\n"); }
#[test] fn rt_ldr64()      { roundtrip(".text\nldr x0, [x1, #24]\n"); }
#[test] fn rt_str64()      { roundtrip(".text\nstr x2, [x3, #32]\n"); }
#[test] fn rt_ldr32()      { roundtrip(".text\nldr w4, [x5, #12]\n"); }
#[test] fn rt_str32()      { roundtrip(".text\nstr w6, [x7, #16]\n"); }
#[test] fn rt_ldrb()       { roundtrip(".text\nldrb w0, [x1, #3]\n"); }
#[test] fn rt_ldrh()       { roundtrip(".text\nldrh w2, [x3, #6]\n"); }
#[test] fn rt_ldrsw()      { roundtrip(".text\nldrsw x0, [x1, #8]\n"); }
#[test] fn rt_stp_pre()    { roundtrip(".text\nstp x29, x30, [sp, #-16]!\n"); }
#[test] fn rt_ldp_post()   { roundtrip(".text\nldp x29, x30, [sp], #16\n"); }
#[test] fn rt_stp_off()    { roundtrip(".text\nstp x19, x20, [sp, #16]\n"); }
#[test] fn rt_ldp_off()    { roundtrip(".text\nldp x21, x22, [sp, #48]\n"); }
#[test] fn rt_fadd_d()     { roundtrip(".text\nfadd d0, d1, d2\n"); }
#[test] fn rt_fsub_d()     { roundtrip(".text\nfsub d3, d4, d5\n"); }
#[test] fn rt_fmul_d()     { roundtrip(".text\nfmul d6, d7, d8\n"); }
#[test] fn rt_fdiv_d()     { roundtrip(".text\nfdiv d9, d10, d11\n"); }
#[test] fn rt_fadd_s()     { roundtrip(".text\nfadd s0, s1, s2\n"); }
#[test] fn rt_fneg_d()     { roundtrip(".text\nfneg d3, d4\n"); }
#[test] fn rt_fabs_d()     { roundtrip(".text\nfabs d3, d4\n"); }
#[test] fn rt_fsqrt_d()    { roundtrip(".text\nfsqrt d3, d4\n"); }
#[test] fn rt_fcmp_d()     { roundtrip(".text\nfcmp d3, d4\n"); }
#[test] fn rt_fmadd_d()    { roundtrip(".text\nfmadd d0, d1, d2, d3\n"); }
#[test] fn rt_fcvtzs()     { roundtrip(".text\nfcvtzs x0, d1\n"); }
#[test] fn rt_scvtf()      { roundtrip(".text\nscvtf d0, x1\n"); }
#[test] fn rt_fmov_to()    { roundtrip(".text\nfmov d5, x6\n"); }
#[test] fn rt_fmov_from()  { roundtrip(".text\nfmov x5, d6\n"); }
#[test] fn rt_svc()        { roundtrip(".text\nsvc #0x80\n"); }
#[test] fn rt_nop()        { roundtrip(".text\nnop\n"); }
#[test] fn rt_brk()        { roundtrip(".text\nbrk #42\n"); }

// ---- Multi-instruction round-trips ----

#[test]
fn rt_function_prologue() {
    roundtrip(".text\n\
stp x29, x30, [sp, #-16]!\n\
mov x29, sp\n\
");
}

#[test]
fn rt_function_epilogue() {
    roundtrip(".text\n\
ldp x29, x30, [sp], #16\n\
ret\n\
");
}

#[test]
fn rt_arithmetic_sequence() {
    roundtrip(".text\n\
add x0, x1, x2\n\
sub x3, x4, x5\n\
mul x6, x7, x8\n\
sdiv x9, x10, x11\n\
");
}

#[test]
fn rt_branch_sequence() {
    roundtrip(".text\n\
cmp x0, #0\n\
b.eq #12\n\
add x0, x0, #1\n\
b #8\n\
sub x0, x0, #1\n\
ret\n\
");
}

#[test]
fn rt_fp_sequence() {
    roundtrip(".text\n\
fadd d0, d1, d2\n\
fmul d3, d0, d4\n\
fsub d5, d3, d6\n\
fcmp d5, d7\n\
");
}

// ---- Parse real clang output ----

#[test]
fn rt_clang_output() {
    // Generate a real .s file from clang and parse it.
    let c_src = "int square(int x) { return x * x; }\n";
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir();
    let c_path = dir.join(format!("afs_rt_clang_{}.c", id));
    let s_path = dir.join(format!("afs_rt_clang_{}.s", id));

    std::fs::write(&c_path, c_src).unwrap();

    let status = Command::new("clang")
        .args(["-S", "-O2", "-o", s_path.to_str().unwrap(), c_path.to_str().unwrap()])
        .status();

    if status.is_err() || !status.unwrap().success() {
        // clang not available — skip gracefully.
        return;
    }

    let asm = std::fs::read_to_string(&s_path).unwrap();
    // Just verify we can parse it without errors.
    let result = afs_as::parse::parse(&asm);
    assert!(result.is_ok(), "failed to parse clang output: {:?}", result.err());
    let stmts = result.unwrap();
    let inst_count = stmts.iter().filter(|s| matches!(s, Stmt::Instruction(_))).count();
    assert!(inst_count > 0, "expected at least one instruction from clang output");
}
