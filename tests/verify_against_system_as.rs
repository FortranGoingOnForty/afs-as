//! Integration tests that verify our encodings against the system assembler.
//!
//! For each test, we:
//! 1. Write an assembly instruction to a temp .s file
//! 2. Assemble it with Apple `as`
//! 3. Extract the 4-byte encoding from the .o with `otool -t`
//! 4. Compare against our encode() output
//!
//! This gives us ground-truth validation: if Apple `as` and our encoder
//! agree on every instruction, our encoder is correct.

use std::io::Write;
use std::process::Command;

use afs_as::encode::Inst;
use afs_as::reg::*;

/// Assemble a single ARM64 instruction with Apple `as` and return its 4-byte encoding.
fn system_encode(asm: &str) -> u32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tid = std::thread::current().id();
    let dir = std::env::temp_dir();
    let s_path = dir.join(format!("afs_as_test_{:?}_{}.s", tid, id));
    let o_path = dir.join(format!("afs_as_test_{:?}_{}.o", tid, id));

    // Write assembly file
    let mut f = std::fs::File::create(&s_path).expect("create .s");
    writeln!(f, ".text\n{}", asm).expect("write .s");
    drop(f);

    // Assemble
    let status = Command::new("as")
        .args(["-o", o_path.to_str().unwrap(), s_path.to_str().unwrap()])
        .status()
        .expect("run as");
    assert!(status.success(), "as failed for: {}", asm);

    // Extract encoding with otool
    let output = Command::new("otool")
        .args(["-t", o_path.to_str().unwrap()])
        .output()
        .expect("run otool");
    let text = String::from_utf8_lossy(&output.stdout);

    // Parse: otool output has "addr hex hex hex ..." lines after header
    // We want the first hex word after the address on the data line
    let hex = text
        .lines()
        .filter(|line| line.starts_with("0"))
        .flat_map(|line| line.split_whitespace().skip(1))
        .next()
        .unwrap_or_else(|| panic!("no encoding found in otool output for: {}", asm));

    u32::from_str_radix(hex, 16).unwrap_or_else(|_| panic!("bad hex '{}' for: {}", hex, asm))
}

/// Helper: verify that our encoding matches the system assembler.
fn verify(asm: &str, inst: Inst) {
    let expected = system_encode(asm);
    let got = inst.encode();
    assert_eq!(
        got, expected,
        "\nMismatch for: {}\n  ours:   0x{:08X}\n  system: 0x{:08X}",
        asm, got, expected
    );
}

// ---- Data processing ----

#[test] fn sys_add_reg()  { verify("add x5, x6, x7",  Inst::AddReg  { rd: X5,  rn: X6,  rm: X7,  sf: true }); }
#[test] fn sys_sub_reg()  { verify("sub x10, x11, x12", Inst::SubReg { rd: X10, rn: X11, rm: X12, sf: true }); }
#[test] fn sys_add_w()    { verify("add w3, w4, w5",  Inst::AddReg  { rd: W3, rn: W4, rm: W5, sf: false }); }
#[test] fn sys_mul()      { verify("mul x0, x1, x2",  Inst::Mul     { rd: X0, rn: X1, rm: X2, sf: true }); }
#[test] fn sys_sdiv()     { verify("sdiv x3, x4, x5", Inst::Sdiv    { rd: X3, rn: X4, rm: X5, sf: true }); }
#[test] fn sys_udiv()     { verify("udiv x3, x4, x5", Inst::Udiv    { rd: X3, rn: X4, rm: X5, sf: true }); }
#[test] fn sys_and()      { verify("and x3, x4, x5",  Inst::AndReg  { rd: X3, rn: X4, rm: X5, sf: true }); }
#[test] fn sys_orr()      { verify("orr x3, x4, x5",  Inst::OrrReg  { rd: X3, rn: X4, rm: X5, sf: true }); }
#[test] fn sys_eor()      { verify("eor x3, x4, x5",  Inst::EorReg  { rd: X3, rn: X4, rm: X5, sf: true }); }

// ---- Data processing (immediate) ----

#[test] fn sys_add_imm()    { verify("add x3, x4, #100", Inst::AddImm { rd: X3, rn: X4, imm12: 100, shift: false, sf: true }); }
#[test] fn sys_sub_imm()    { verify("sub x3, x4, #100", Inst::SubImm { rd: X3, rn: X4, imm12: 100, shift: false, sf: true }); }
#[test] fn sys_cmp_imm()    { verify("cmp x5, #255", Inst::SubsImm { rd: XZR, rn: X5, imm12: 255, shift: false, sf: true }); }

// ---- Move ----

#[test] fn sys_movz()     { verify("movz x8, #0xBEEF",       Inst::Movz { rd: X8, imm16: 0xBEEF, shift: 0,  sf: true }); }
#[test] fn sys_movz_hi()  { verify("movz x8, #0xCAFE, lsl #48", Inst::Movz { rd: X8, imm16: 0xCAFE, shift: 48, sf: true }); }
#[test] fn sys_movk()     { verify("movk x9, #0x1234, lsl #16", Inst::Movk { rd: X9, imm16: 0x1234, shift: 16, sf: true }); }

// ---- Shifts ----

#[test] fn sys_lsl()  { verify("lsl x0, x1, #7",  Inst::LslImm { rd: X0, rn: X1, amount: 7,  sf: true }); }
#[test] fn sys_lsr()  { verify("lsr x0, x1, #15", Inst::LsrImm { rd: X0, rn: X1, amount: 15, sf: true }); }
#[test] fn sys_asr()  { verify("asr x0, x1, #31", Inst::AsrImm { rd: X0, rn: X1, amount: 31, sf: true }); }

// ---- Branches ----

#[test] fn sys_b()        { verify("b #20",         Inst::B    { offset: 20 }); }
#[test] fn sys_bl()       { verify("bl #40",        Inst::Bl   { offset: 40 }); }
#[test] fn sys_b_eq()     { verify("b.eq #24",      Inst::BCond { cond: Cond::EQ, offset: 24 }); }
#[test] fn sys_b_lt()     { verify("b.lt #32",      Inst::BCond { cond: Cond::LT, offset: 32 }); }
#[test] fn sys_cbz()      { verify("cbz x5, #16",   Inst::Cbz  { rt: X5, offset: 16, sf: true }); }
#[test] fn sys_cbnz()     { verify("cbnz x10, #24", Inst::Cbnz { rt: X10, offset: 24, sf: true }); }
#[test] fn sys_ret()      { verify("ret",           Inst::Ret  { rn: X30 }); }
#[test] fn sys_br()       { verify("br x8",         Inst::Br   { rn: X8 }); }
#[test] fn sys_blr()      { verify("blr x9",        Inst::Blr  { rn: X9 }); }

// ---- Load/Store (unsigned offset) ----

#[test] fn sys_ldr64()    { verify("ldr x3, [x4, #24]",  Inst::LdrImm64 { rt: X3, rn: X4, offset: 24 }); }
#[test] fn sys_str64()    { verify("str x3, [x4, #32]",  Inst::StrImm64 { rt: X3, rn: X4, offset: 32 }); }
#[test] fn sys_ldr32()    { verify("ldr w3, [x4, #12]",  Inst::LdrImm32 { rt: W3, rn: X4, offset: 12 }); }
#[test] fn sys_str32()    { verify("str w3, [x4, #16]",  Inst::StrImm32 { rt: W3, rn: X4, offset: 16 }); }
#[test] fn sys_ldrb()     { verify("ldrb w0, [x1, #3]",  Inst::Ldrb { rt: W0, rn: X1, offset: 3 }); }
#[test] fn sys_ldrh()     { verify("ldrh w0, [x1, #6]",  Inst::Ldrh { rt: W0, rn: X1, offset: 6 }); }
#[test] fn sys_ldrsw()    { verify("ldrsw x0, [x1, #8]", Inst::Ldrsw { rt: X0, rn: X1, offset: 8 }); }

// ---- Load/Store pair ----

#[test] fn sys_stp_pre()  { verify("stp x29, x30, [sp, #-32]!", Inst::StpPre64 { rt1: X29, rt2: X30, rn: SP, offset: -32 }); }
#[test] fn sys_ldp_post() { verify("ldp x29, x30, [sp], #32",  Inst::LdpPost64 { rt1: X29, rt2: X30, rn: SP, offset: 32 }); }
#[test] fn sys_stp_off()  { verify("stp x19, x20, [sp, #32]",  Inst::StpOff64 { rt1: X19, rt2: X20, rn: SP, offset: 32 }); }
#[test] fn sys_ldp_off()  { verify("ldp x21, x22, [sp, #48]",  Inst::LdpOff64 { rt1: X21, rt2: X22, rn: SP, offset: 48 }); }

// ---- FP arithmetic ----

#[test] fn sys_fadd_d()   { verify("fadd d5, d6, d7",   Inst::FaddD { rd: D5, rn: D6, rm: D7 }); }
#[test] fn sys_fsub_d()   { verify("fsub d5, d6, d7",   Inst::FsubD { rd: D5, rn: D6, rm: D7 }); }
#[test] fn sys_fmul_d()   { verify("fmul d5, d6, d7",   Inst::FmulD { rd: D5, rn: D6, rm: D7 }); }
#[test] fn sys_fdiv_d()   { verify("fdiv d5, d6, d7",   Inst::FdivD { rd: D5, rn: D6, rm: D7 }); }
#[test] fn sys_fadd_s()   { verify("fadd s5, s6, s7",   Inst::FaddS { rd: S5, rn: S6, rm: S7 }); }
#[test] fn sys_fneg_d()   { verify("fneg d3, d4",       Inst::FnegD { rd: D3, rn: D4 }); }
#[test] fn sys_fabs_d()   { verify("fabs d3, d4",       Inst::FabsD { rd: D3, rn: D4 }); }
#[test] fn sys_fsqrt_d()  { verify("fsqrt d3, d4",      Inst::FsqrtD { rd: D3, rn: D4 }); }
#[test] fn sys_fcmp_d()   { verify("fcmp d3, d4",       Inst::FcmpD { rn: D3, rm: D4 }); }
#[test] fn sys_fmadd_d()  { verify("fmadd d0, d1, d2, d3", Inst::FmaddD { rd: D0, rn: D1, rm: D2, ra: D3 }); }

// ---- FP / integer conversion ----

#[test] fn sys_fcvtzs()   { verify("fcvtzs x5, d6",     Inst::FcvtzsD { rd: X5, rn: D6 }); }
#[test] fn sys_scvtf()    { verify("scvtf d5, x6",      Inst::ScvtfD { rd: D5, rn: X6 }); }
#[test] fn sys_fmov_to()  { verify("fmov d5, x6",       Inst::FmovToD { rd: D5, rn: X6 }); }
#[test] fn sys_fmov_from(){ verify("fmov x5, d6",       Inst::FmovFromD { rd: X5, rn: D6 }); }

// ---- System ----

#[test] fn sys_svc()      { verify("svc #0x80",         Inst::Svc { imm16: 0x80 }); }
#[test] fn sys_nop()      { verify("nop",               Inst::Nop); }
#[test] fn sys_brk()      { verify("brk #42",           Inst::Brk { imm16: 42 }); }
