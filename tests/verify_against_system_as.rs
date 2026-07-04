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

use afs_as::encode::{AddrExtend, BarrierOpt, Inst, RegExtend, RegShift};
use afs_as::reg::*;

/// Assemble a single ARM64 instruction with Apple `as` and return its 4-byte encoding.
fn system_encode(asm: &str) -> u32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let tid = std::thread::current().id();
    let dir = std::env::temp_dir();
    let s_path = dir.join(format!("afs_as_test_{}_{:?}_{}.s", pid, tid, id));
    let o_path = dir.join(format!("afs_as_test_{}_{:?}_{}.o", pid, tid, id));

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
fn sys_add_reg() {
    if !native_macho_host("verify_against_system_as", "sys_add_reg") {
        return;
    }
    verify(
        "add x5, x6, x7",
        Inst::AddReg {
            rd: X5,
            rn: X6,
            rm: X7,
            sf: true,
        },
    );
}
#[test]
fn sys_sub_reg() {
    if !native_macho_host("verify_against_system_as", "sys_sub_reg") {
        return;
    }
    verify(
        "sub x10, x11, x12",
        Inst::SubReg {
            rd: X10,
            rn: X11,
            rm: X12,
            sf: true,
        },
    );
}
#[test]
fn sys_add_w() {
    if !native_macho_host("verify_against_system_as", "sys_add_w") {
        return;
    }
    verify(
        "add w3, w4, w5",
        Inst::AddReg {
            rd: W3,
            rn: W4,
            rm: W5,
            sf: false,
        },
    );
}
#[test]
fn sys_add_shift_reg() {
    if !native_macho_host("verify_against_system_as", "sys_add_shift_reg") {
        return;
    }
    verify(
        "add x0, x1, x2, lsl #3",
        Inst::AddShiftReg {
            rd: X0,
            rn: X1,
            rm: X2,
            shift: RegShift::Lsl,
            amount: 3,
            sf: true,
        },
    );
}
#[test]
fn sys_sub_shift_reg() {
    if !native_macho_host("verify_against_system_as", "sys_sub_shift_reg") {
        return;
    }
    verify(
        "sub w3, w4, w5, asr #7",
        Inst::SubShiftReg {
            rd: W3,
            rn: W4,
            rm: W5,
            shift: RegShift::Asr,
            amount: 7,
            sf: false,
        },
    );
}
#[test]
fn sys_cmp_shift_reg() {
    if !native_macho_host("verify_against_system_as", "sys_cmp_shift_reg") {
        return;
    }
    verify(
        "cmp x6, x7, lsr #4",
        Inst::SubsShiftReg {
            rd: XZR,
            rn: X6,
            rm: X7,
            shift: RegShift::Lsr,
            amount: 4,
            sf: true,
        },
    );
}
#[test]
fn sys_add_ext_reg() {
    if !native_macho_host("verify_against_system_as", "sys_add_ext_reg") {
        return;
    }
    verify(
        "add x0, x0, w1, sxtw #3",
        Inst::AddExtReg {
            rd: X0,
            rn: X0,
            rm: W1,
            extend: RegExtend::Sxtw,
            amount: 3,
            sf: true,
        },
    );
}
#[test]
fn sys_add_ext_reg_sp_base() {
    if !native_macho_host("verify_against_system_as", "sys_add_ext_reg_sp_base") {
        return;
    }
    verify(
        "add x11, sp, w12, sxtw #2",
        Inst::AddExtReg {
            rd: X11,
            rn: SP,
            rm: W12,
            extend: RegExtend::Sxtw,
            amount: 2,
            sf: true,
        },
    );
}
#[test]
fn sys_sub_ext_reg() {
    if !native_macho_host("verify_against_system_as", "sys_sub_ext_reg") {
        return;
    }
    verify(
        "sub x2, x3, w4, uxtw #2",
        Inst::SubExtReg {
            rd: X2,
            rn: X3,
            rm: W4,
            extend: RegExtend::Uxtw,
            amount: 2,
            sf: true,
        },
    );
}
#[test]
fn sys_cmp_ext_reg() {
    if !native_macho_host("verify_against_system_as", "sys_cmp_ext_reg") {
        return;
    }
    verify(
        "cmp x0, w1, sxtw",
        Inst::SubsExtReg {
            rd: XZR,
            rn: X0,
            rm: W1,
            extend: RegExtend::Sxtw,
            amount: 0,
            sf: true,
        },
    );
}
#[test]
fn sys_mul() {
    if !native_macho_host("verify_against_system_as", "sys_mul") {
        return;
    }
    verify(
        "mul x0, x1, x2",
        Inst::Mul {
            rd: X0,
            rn: X1,
            rm: X2,
            sf: true,
        },
    );
}
#[test]
fn sys_madd() {
    if !native_macho_host("verify_against_system_as", "sys_madd") {
        return;
    }
    verify(
        "madd w0, w0, w0, w8",
        Inst::Madd {
            rd: W0,
            rn: W0,
            rm: W0,
            ra: W8,
            sf: false,
        },
    );
}
#[test]
fn sys_msub() {
    if !native_macho_host("verify_against_system_as", "sys_msub") {
        return;
    }
    verify(
        "msub w9, w8, w1, w0",
        Inst::Msub {
            rd: W9,
            rn: W8,
            rm: W1,
            ra: W0,
            sf: false,
        },
    );
}
#[test]
fn sys_umull() {
    if !native_macho_host("verify_against_system_as", "sys_umull") {
        return;
    }
    verify(
        "umull x9, w8, w9",
        Inst::Umull {
            rd: X9,
            rn: W8,
            rm: W9,
        },
    );
}
#[test]
fn sys_sdiv() {
    if !native_macho_host("verify_against_system_as", "sys_sdiv") {
        return;
    }
    verify(
        "sdiv x3, x4, x5",
        Inst::Sdiv {
            rd: X3,
            rn: X4,
            rm: X5,
            sf: true,
        },
    );
}
#[test]
fn sys_udiv() {
    if !native_macho_host("verify_against_system_as", "sys_udiv") {
        return;
    }
    verify(
        "udiv x3, x4, x5",
        Inst::Udiv {
            rd: X3,
            rn: X4,
            rm: X5,
            sf: true,
        },
    );
}
#[test]
fn sys_and() {
    if !native_macho_host("verify_against_system_as", "sys_and") {
        return;
    }
    verify(
        "and x3, x4, x5",
        Inst::AndReg {
            rd: X3,
            rn: X4,
            rm: X5,
            sf: true,
        },
    );
}
#[test]
fn sys_and_imm() {
    if !native_macho_host("verify_against_system_as", "sys_and_imm") {
        return;
    }
    verify(
        "and w8, w8, #0x7",
        Inst::AndImm {
            rd: W8,
            rn: W8,
            imm: 0x7,
            sf: false,
        },
    );
}
#[test]
fn sys_orr() {
    if !native_macho_host("verify_against_system_as", "sys_orr") {
        return;
    }
    verify(
        "orr x3, x4, x5",
        Inst::OrrReg {
            rd: X3,
            rn: X4,
            rm: X5,
            sf: true,
        },
    );
}
#[test]
fn sys_orn() {
    if !native_macho_host("verify_against_system_as", "sys_orn") {
        return;
    }
    verify(
        "orn x3, x4, x5",
        Inst::OrnReg {
            rd: X3,
            rn: X4,
            rm: X5,
            sf: true,
        },
    );
}
#[test]
fn sys_eor() {
    if !native_macho_host("verify_against_system_as", "sys_eor") {
        return;
    }
    verify(
        "eor x3, x4, x5",
        Inst::EorReg {
            rd: X3,
            rn: X4,
            rm: X5,
            sf: true,
        },
    );
}

// ---- Data processing (immediate) ----

#[test]
fn sys_add_imm() {
    if !native_macho_host("verify_against_system_as", "sys_add_imm") {
        return;
    }
    verify(
        "add x3, x4, #100",
        Inst::AddImm {
            rd: X3,
            rn: X4,
            imm12: 100,
            shift: false,
            sf: true,
        },
    );
}
#[test]
fn sys_sub_imm() {
    if !native_macho_host("verify_against_system_as", "sys_sub_imm") {
        return;
    }
    verify(
        "sub x3, x4, #100",
        Inst::SubImm {
            rd: X3,
            rn: X4,
            imm12: 100,
            shift: false,
            sf: true,
        },
    );
}
#[test]
fn sys_cmp_imm() {
    if !native_macho_host("verify_against_system_as", "sys_cmp_imm") {
        return;
    }
    verify(
        "cmp x5, #255",
        Inst::SubsImm {
            rd: XZR,
            rn: X5,
            imm12: 255,
            shift: false,
            sf: true,
        },
    );
}

// ---- Move ----

#[test]
fn sys_movz() {
    if !native_macho_host("verify_against_system_as", "sys_movz") {
        return;
    }
    verify(
        "movz x8, #0xBEEF",
        Inst::Movz {
            rd: X8,
            imm16: 0xBEEF,
            shift: 0,
            sf: true,
        },
    );
}
#[test]
fn sys_movz_hi() {
    if !native_macho_host("verify_against_system_as", "sys_movz_hi") {
        return;
    }
    verify(
        "movz x8, #0xCAFE, lsl #48",
        Inst::Movz {
            rd: X8,
            imm16: 0xCAFE,
            shift: 48,
            sf: true,
        },
    );
}
#[test]
fn sys_movk() {
    if !native_macho_host("verify_against_system_as", "sys_movk") {
        return;
    }
    verify(
        "movk x9, #0x1234, lsl #16",
        Inst::Movk {
            rd: X9,
            imm16: 0x1234,
            shift: 16,
            sf: true,
        },
    );
}
#[test]
fn sys_movn_hi() {
    if !native_macho_host("verify_against_system_as", "sys_movn_hi") {
        return;
    }
    verify(
        "movn x0, #1, lsl #16",
        Inst::Movn {
            rd: X0,
            imm16: 1,
            shift: 16,
            sf: true,
        },
    );
}

#[test]
fn sys_mov_wzr() {
    if !native_macho_host("verify_against_system_as", "sys_mov_wzr") {
        return;
    }
    verify(
        "mov w26, wzr",
        Inst::OrrReg {
            rd: W26,
            rn: WZR,
            rm: WZR,
            sf: false,
        },
    );
}

// ---- Shifts ----

#[test]
fn sys_lsl() {
    if !native_macho_host("verify_against_system_as", "sys_lsl") {
        return;
    }
    verify(
        "lsl x0, x1, #7",
        Inst::LslImm {
            rd: X0,
            rn: X1,
            amount: 7,
            sf: true,
        },
    );
}
#[test]
fn sys_lsr() {
    if !native_macho_host("verify_against_system_as", "sys_lsr") {
        return;
    }
    verify(
        "lsr x0, x1, #15",
        Inst::LsrImm {
            rd: X0,
            rn: X1,
            amount: 15,
            sf: true,
        },
    );
}
#[test]
fn sys_asr() {
    if !native_macho_host("verify_against_system_as", "sys_asr") {
        return;
    }
    verify(
        "asr x0, x1, #31",
        Inst::AsrImm {
            rd: X0,
            rn: X1,
            amount: 31,
            sf: true,
        },
    );
}

// ---- Branches ----

#[test]
fn sys_b() {
    if !native_macho_host("verify_against_system_as", "sys_b") {
        return;
    }
    verify("b #20", Inst::B { offset: 20 });
}
#[test]
fn sys_bl() {
    if !native_macho_host("verify_against_system_as", "sys_bl") {
        return;
    }
    verify("bl #40", Inst::Bl { offset: 40 });
}
#[test]
fn sys_b_eq() {
    if !native_macho_host("verify_against_system_as", "sys_b_eq") {
        return;
    }
    verify(
        "b.eq #24",
        Inst::BCond {
            cond: Cond::EQ,
            offset: 24,
        },
    );
}
#[test]
fn sys_b_lt() {
    if !native_macho_host("verify_against_system_as", "sys_b_lt") {
        return;
    }
    verify(
        "b.lt #32",
        Inst::BCond {
            cond: Cond::LT,
            offset: 32,
        },
    );
}
#[test]
fn sys_cbz() {
    if !native_macho_host("verify_against_system_as", "sys_cbz") {
        return;
    }
    verify(
        "cbz x5, #16",
        Inst::Cbz {
            rt: X5,
            offset: 16,
            sf: true,
        },
    );
}
#[test]
fn sys_cbnz() {
    if !native_macho_host("verify_against_system_as", "sys_cbnz") {
        return;
    }
    verify(
        "cbnz x10, #24",
        Inst::Cbnz {
            rt: X10,
            offset: 24,
            sf: true,
        },
    );
}
#[test]
fn sys_tbz() {
    if !native_macho_host("verify_against_system_as", "sys_tbz") {
        return;
    }
    verify(
        "tbz x0, #5, #8",
        Inst::Tbz {
            rt: X0,
            bit: 5,
            offset: 8,
            sf: true,
        },
    );
}
#[test]
fn sys_tbnz() {
    if !native_macho_host("verify_against_system_as", "sys_tbnz") {
        return;
    }
    verify(
        "tbnz x1, #33, #12",
        Inst::Tbnz {
            rt: X1,
            bit: 33,
            offset: 12,
            sf: true,
        },
    );
}
#[test]
fn sys_ret() {
    if !native_macho_host("verify_against_system_as", "sys_ret") {
        return;
    }
    verify("ret", Inst::Ret { rn: X30 });
}
#[test]
fn sys_br() {
    if !native_macho_host("verify_against_system_as", "sys_br") {
        return;
    }
    verify("br x8", Inst::Br { rn: X8 });
}
#[test]
fn sys_blr() {
    if !native_macho_host("verify_against_system_as", "sys_blr") {
        return;
    }
    verify("blr x9", Inst::Blr { rn: X9 });
}
#[test]
fn sys_csel() {
    if !native_macho_host("verify_against_system_as", "sys_csel") {
        return;
    }
    verify(
        "csel w0, w0, w1, gt",
        Inst::Csel {
            rd: W0,
            rn: W0,
            rm: W1,
            cond: Cond::GT,
            sf: false,
        },
    );
}
#[test]
fn sys_ccmp() {
    if !native_macho_host("verify_against_system_as", "sys_ccmp") {
        return;
    }
    verify(
        "ccmp w0, #3, #4, ne",
        Inst::CcmpImm {
            rn: W0,
            imm5: 3,
            nzcv: 4,
            cond: Cond::NE,
            sf: false,
        },
    );
}
#[test]
fn sys_ccmn() {
    if !native_macho_host("verify_against_system_as", "sys_ccmn") {
        return;
    }
    verify(
        "ccmn x3, #9, #1, ge",
        Inst::CcmnImm {
            rn: X3,
            imm5: 9,
            nzcv: 1,
            cond: Cond::GE,
            sf: true,
        },
    );
}
#[test]
fn sys_csinc() {
    if !native_macho_host("verify_against_system_as", "sys_csinc") {
        return;
    }
    verify(
        "csinc x2, x3, x3, ne",
        Inst::Csinc {
            rd: X2,
            rn: X3,
            rm: X3,
            cond: Cond::NE,
            sf: true,
        },
    );
}
#[test]
fn sys_csinv() {
    if !native_macho_host("verify_against_system_as", "sys_csinv") {
        return;
    }
    verify(
        "csinv x2, x3, x4, ne",
        Inst::Csinv {
            rd: X2,
            rn: X3,
            rm: X4,
            cond: Cond::NE,
            sf: true,
        },
    );
}
#[test]
fn sys_csneg() {
    if !native_macho_host("verify_against_system_as", "sys_csneg") {
        return;
    }
    verify(
        "csneg x5, x6, x7, gt",
        Inst::Csneg {
            rd: X5,
            rn: X6,
            rm: X7,
            cond: Cond::GT,
            sf: true,
        },
    );
}
#[test]
fn sys_csetm() {
    if !native_macho_host("verify_against_system_as", "sys_csetm") {
        return;
    }
    verify(
        "csetm w8, eq",
        Inst::Csinv {
            rd: W8,
            rn: XZR,
            rm: XZR,
            cond: Cond::NE,
            sf: false,
        },
    );
}
#[test]
fn sys_cinv() {
    if !native_macho_host("verify_against_system_as", "sys_cinv") {
        return;
    }
    verify(
        "cinv w9, w10, mi",
        Inst::Csinv {
            rd: W9,
            rn: W10,
            rm: W10,
            cond: Cond::PL,
            sf: false,
        },
    );
}
#[test]
fn sys_cneg() {
    if !native_macho_host("verify_against_system_as", "sys_cneg") {
        return;
    }
    verify(
        "cneg x11, x12, lt",
        Inst::Csneg {
            rd: X11,
            rn: X12,
            rm: X12,
            cond: Cond::GE,
            sf: true,
        },
    );
}
#[test]
fn sys_tst_imm() {
    if !native_macho_host("verify_against_system_as", "sys_tst_imm") {
        return;
    }
    verify(
        "tst w8, #0x7",
        Inst::AndsImm {
            rd: XZR,
            rn: W8,
            imm: 0x7,
            sf: false,
        },
    );
}
#[test]
fn sys_ubfiz() {
    if !native_macho_host("verify_against_system_as", "sys_ubfiz") {
        return;
    }
    verify(
        "ubfiz w8, w0, #5, #3",
        Inst::Ubfiz {
            rd: W8,
            rn: W0,
            lsb: 5,
            width: 3,
            sf: false,
        },
    );
}
#[test]
fn sys_bfi() {
    if !native_macho_host("verify_against_system_as", "sys_bfi") {
        return;
    }
    verify(
        "bfi w0, w8, #5, #27",
        Inst::Bfi {
            rd: W0,
            rn: W8,
            lsb: 5,
            width: 27,
            sf: false,
        },
    );
}
#[test]
fn sys_bfxil() {
    if !native_macho_host("verify_against_system_as", "sys_bfxil") {
        return;
    }
    verify(
        "bfxil w8, w0, #3, #5",
        Inst::Bfxil {
            rd: W8,
            rn: W0,
            lsb: 3,
            width: 5,
            sf: false,
        },
    );
}

// ---- Load/Store (unsigned offset) ----

#[test]
fn sys_ldr64() {
    if !native_macho_host("verify_against_system_as", "sys_ldr64") {
        return;
    }
    verify(
        "ldr x3, [x4, #24]",
        Inst::LdrImm64 {
            rt: X3,
            rn: X4,
            offset: 24,
        },
    );
}
#[test]
fn sys_str64() {
    if !native_macho_host("verify_against_system_as", "sys_str64") {
        return;
    }
    verify(
        "str x3, [x4, #32]",
        Inst::StrImm64 {
            rt: X3,
            rn: X4,
            offset: 32,
        },
    );
}
#[test]
fn sys_ldr32() {
    if !native_macho_host("verify_against_system_as", "sys_ldr32") {
        return;
    }
    verify(
        "ldr w3, [x4, #12]",
        Inst::LdrImm32 {
            rt: W3,
            rn: X4,
            offset: 12,
        },
    );
}
#[test]
fn sys_str32() {
    if !native_macho_host("verify_against_system_as", "sys_str32") {
        return;
    }
    verify(
        "str w3, [x4, #16]",
        Inst::StrImm32 {
            rt: W3,
            rn: X4,
            offset: 16,
        },
    );
}
#[test]
fn sys_ldur64() {
    if !native_macho_host("verify_against_system_as", "sys_ldur64") {
        return;
    }
    verify(
        "ldur x9, [x29, #-8]",
        Inst::Ldur64 {
            rt: X9,
            rn: X29,
            offset: -8,
        },
    );
}
#[test]
fn sys_stur32() {
    if !native_macho_host("verify_against_system_as", "sys_stur32") {
        return;
    }
    verify(
        "stur w6, [x7, #-4]",
        Inst::Stur32 {
            rt: W6,
            rn: X7,
            offset: -4,
        },
    );
}
#[test]
fn sys_ldr_negative_alias() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_negative_alias") {
        return;
    }
    verify(
        "ldr x0, [x1, #-8]",
        Inst::Ldur64 {
            rt: X0,
            rn: X1,
            offset: -8,
        },
    );
}
#[test]
fn sys_ldr_post32() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_post32") {
        return;
    }
    verify(
        "ldr w0, [x1], #4",
        Inst::LdrPost32 {
            rt: W0,
            rn: X1,
            offset: 4,
        },
    );
}
#[test]
fn sys_str_pre32() {
    if !native_macho_host("verify_against_system_as", "sys_str_pre32") {
        return;
    }
    verify(
        "str w2, [x3, #-4]!",
        Inst::StrPre32 {
            rt: W2,
            rn: X3,
            offset: -4,
        },
    );
}
#[test]
fn sys_ldr64_tlvp_pageoff() {
    if !native_macho_host("verify_against_system_as", "sys_ldr64_tlvp_pageoff") {
        return;
    }
    verify(
        "ldr x0, [x0, _tls_counter@TLVPPAGEOFF]",
        Inst::LdrImm64 {
            rt: X0,
            rn: X0,
            offset: 0,
        },
    );
}
#[test]
fn sys_ldr_lit64() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_lit64") {
        return;
    }
    verify("ldr x0, #8", Inst::LdrLit64 { rt: X0, offset: 8 });
}
#[test]
fn sys_ldr_lit32() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_lit32") {
        return;
    }
    verify("ldr w1, #12", Inst::LdrLit32 { rt: W1, offset: 12 });
}
#[test]
fn sys_ldr64_reg() {
    if !native_macho_host("verify_against_system_as", "sys_ldr64_reg") {
        return;
    }
    verify(
        "ldr x0, [x1, x2]",
        Inst::LdrReg64 {
            rt: X0,
            rn: X1,
            rm: X2,
            extend: AddrExtend::Lsl,
            shift: false,
        },
    );
}
#[test]
fn sys_ldr64_reg_uxtw() {
    if !native_macho_host("verify_against_system_as", "sys_ldr64_reg_uxtw") {
        return;
    }
    verify(
        "ldr x6, [x7, w8, uxtw #3]",
        Inst::LdrReg64 {
            rt: X6,
            rn: X7,
            rm: W8,
            extend: AddrExtend::Uxtw,
            shift: true,
        },
    );
}
#[test]
fn sys_str64_reg() {
    if !native_macho_host("verify_against_system_as", "sys_str64_reg") {
        return;
    }
    verify(
        "str x12, [x13, x14]",
        Inst::StrReg64 {
            rt: X12,
            rn: X13,
            rm: X14,
            extend: AddrExtend::Lsl,
            shift: false,
        },
    );
}
#[test]
fn sys_ldrb() {
    if !native_macho_host("verify_against_system_as", "sys_ldrb") {
        return;
    }
    verify(
        "ldrb w0, [x1, #3]",
        Inst::Ldrb {
            rt: W0,
            rn: X1,
            offset: 3,
        },
    );
}
#[test]
fn sys_ldrsb() {
    if !native_macho_host("verify_against_system_as", "sys_ldrsb") {
        return;
    }
    verify(
        "ldrsb w0, [x1, #3]",
        Inst::Ldrsb32 {
            rt: W0,
            rn: X1,
            offset: 3,
        },
    );
}
#[test]
fn sys_ldrb_post() {
    if !native_macho_host("verify_against_system_as", "sys_ldrb_post") {
        return;
    }
    verify(
        "ldrb w9, [x1], #1",
        Inst::LdrbPost {
            rt: W9,
            rn: X1,
            offset: 1,
        },
    );
}
#[test]
fn sys_ldrsb_post() {
    if !native_macho_host("verify_against_system_as", "sys_ldrsb_post") {
        return;
    }
    verify(
        "ldrsb x9, [x1], #1",
        Inst::LdrsbPost64 {
            rt: X9,
            rn: X1,
            offset: 1,
        },
    );
}
#[test]
fn sys_ldrh() {
    if !native_macho_host("verify_against_system_as", "sys_ldrh") {
        return;
    }
    verify(
        "ldrh w0, [x1, #6]",
        Inst::Ldrh {
            rt: W0,
            rn: X1,
            offset: 6,
        },
    );
}
#[test]
fn sys_ldrsh() {
    if !native_macho_host("verify_against_system_as", "sys_ldrsh") {
        return;
    }
    verify(
        "ldrsh x0, [x1, #4]",
        Inst::Ldrsh64 {
            rt: X0,
            rn: X1,
            offset: 4,
        },
    );
}
#[test]
fn sys_strb() {
    if !native_macho_host("verify_against_system_as", "sys_strb") {
        return;
    }
    verify(
        "strb w8, [x9]",
        Inst::Strb {
            rt: W8,
            rn: X9,
            offset: 0,
        },
    );
}
#[test]
fn sys_strb_post() {
    if !native_macho_host("verify_against_system_as", "sys_strb_post") {
        return;
    }
    verify(
        "strb w9, [x8], #1",
        Inst::StrbPost {
            rt: W9,
            rn: X8,
            offset: 1,
        },
    );
}
#[test]
fn sys_strh_pre() {
    if !native_macho_host("verify_against_system_as", "sys_strh_pre") {
        return;
    }
    verify(
        "strh w5, [x6, #2]!",
        Inst::StrhPre {
            rt: W5,
            rn: X6,
            offset: 2,
        },
    );
}
#[test]
fn sys_ldrsw() {
    if !native_macho_host("verify_against_system_as", "sys_ldrsw") {
        return;
    }
    verify(
        "ldrsw x0, [x1, #8]",
        Inst::Ldrsw {
            rt: X0,
            rn: X1,
            offset: 8,
        },
    );
}
#[test]
fn sys_ldrh_reg() {
    if !native_macho_host("verify_against_system_as", "sys_ldrh_reg") {
        return;
    }
    verify(
        "ldrh w3, [x4, w5, uxtw #1]",
        Inst::LdrhReg {
            rt: W3,
            rn: X4,
            rm: W5,
            extend: AddrExtend::Uxtw,
            shift: true,
        },
    );
}
#[test]
fn sys_ldrsh_reg() {
    if !native_macho_host("verify_against_system_as", "sys_ldrsh_reg") {
        return;
    }
    verify(
        "ldrsh w3, [x4, w5, uxtw #1]",
        Inst::LdrshReg32 {
            rt: W3,
            rn: X4,
            rm: W5,
            extend: AddrExtend::Uxtw,
            shift: true,
        },
    );
}
#[test]
fn sys_ldrb_reg() {
    if !native_macho_host("verify_against_system_as", "sys_ldrb_reg") {
        return;
    }
    verify(
        "ldrb w0, [x1, x2]",
        Inst::LdrbReg {
            rt: W0,
            rn: X1,
            rm: X2,
            extend: AddrExtend::Lsl,
            shift: false,
        },
    );
}
#[test]
fn sys_ldrsw_reg() {
    if !native_macho_host("verify_against_system_as", "sys_ldrsw_reg") {
        return;
    }
    verify(
        "ldrsw x6, [x7, w8, sxtw #2]",
        Inst::LdrswReg {
            rt: X6,
            rn: X7,
            rm: W8,
            extend: AddrExtend::Sxtw,
            shift: true,
        },
    );
}
#[test]
fn sys_ldaprb() {
    if !native_macho_host("verify_against_system_as", "sys_ldaprb") {
        return;
    }
    verify("ldaprb w0, [x1]", Inst::Ldaprb { rt: W0, rn: X1 });
}
#[test]
fn sys_ldaprh() {
    if !native_macho_host("verify_against_system_as", "sys_ldaprh") {
        return;
    }
    verify("ldaprh w2, [x3]", Inst::Ldaprh { rt: W2, rn: X3 });
}
#[test]
fn sys_ldapr() {
    if !native_macho_host("verify_against_system_as", "sys_ldapr") {
        return;
    }
    verify("ldapr w8, [x9]", Inst::Ldapr32 { rt: W8, rn: X9 });
}
#[test]
fn sys_stlrb() {
    if !native_macho_host("verify_against_system_as", "sys_stlrb") {
        return;
    }
    verify("stlrb w4, [x5]", Inst::Stlrb { rt: W4, rn: X5 });
}
#[test]
fn sys_stlrh() {
    if !native_macho_host("verify_against_system_as", "sys_stlrh") {
        return;
    }
    verify("stlrh w6, [x7]", Inst::Stlrh { rt: W6, rn: X7 });
}
#[test]
fn sys_stlr() {
    if !native_macho_host("verify_against_system_as", "sys_stlr") {
        return;
    }
    verify("stlr x10, [x11]", Inst::Stlr64 { rt: X10, rn: X11 });
}
#[test]
fn sys_ldaddalb() {
    if !native_macho_host("verify_against_system_as", "sys_ldaddalb") {
        return;
    }
    verify(
        "ldaddalb w0, w1, [x2]",
        Inst::Ldaddalb {
            rs: W0,
            rt: W1,
            rn: X2,
        },
    );
}
#[test]
fn sys_ldaddalh() {
    if !native_macho_host("verify_against_system_as", "sys_ldaddalh") {
        return;
    }
    verify(
        "ldaddalh w3, w4, [x5]",
        Inst::Ldaddalh {
            rs: W3,
            rt: W4,
            rn: X5,
        },
    );
}
#[test]
fn sys_ldumaxalb() {
    if !native_macho_host("verify_against_system_as", "sys_ldumaxalb") {
        return;
    }
    verify(
        "ldumaxalb w0, w1, [x2]",
        Inst::Ldumaxalb {
            rs: W0,
            rt: W1,
            rn: X2,
        },
    );
}
#[test]
fn sys_ldumaxalh() {
    if !native_macho_host("verify_against_system_as", "sys_ldumaxalh") {
        return;
    }
    verify(
        "ldumaxalh w3, w4, [x5]",
        Inst::Ldumaxalh {
            rs: W3,
            rt: W4,
            rn: X5,
        },
    );
}
#[test]
fn sys_ldsmaxalb() {
    if !native_macho_host("verify_against_system_as", "sys_ldsmaxalb") {
        return;
    }
    verify(
        "ldsmaxalb w18, w19, [x20]",
        Inst::Ldsmaxalb {
            rs: W18,
            rt: W19,
            rn: X20,
        },
    );
}
#[test]
fn sys_ldsmaxalh() {
    if !native_macho_host("verify_against_system_as", "sys_ldsmaxalh") {
        return;
    }
    verify(
        "ldsmaxalh w24, w25, [x26]",
        Inst::Ldsmaxalh {
            rs: W24,
            rt: W25,
            rn: X26,
        },
    );
}
#[test]
fn sys_ldaddal() {
    if !native_macho_host("verify_against_system_as", "sys_ldaddal") {
        return;
    }
    verify(
        "ldaddal w0, w8, [x8]",
        Inst::Ldaddal32 {
            rs: W0,
            rt: W8,
            rn: X8,
        },
    );
}
#[test]
fn sys_ldumaxal() {
    if !native_macho_host("verify_against_system_as", "sys_ldumaxal") {
        return;
    }
    verify(
        "ldumaxal x9, x10, [x11]",
        Inst::Ldumaxal64 {
            rs: X9,
            rt: X10,
            rn: X11,
        },
    );
}
#[test]
fn sys_ldsmaxal() {
    if !native_macho_host("verify_against_system_as", "sys_ldsmaxal") {
        return;
    }
    verify(
        "ldsmaxal w0, w1, [x2]",
        Inst::Ldsmaxal32 {
            rs: W0,
            rt: W1,
            rn: X2,
        },
    );
}
#[test]
fn sys_ldsminal() {
    if !native_macho_host("verify_against_system_as", "sys_ldsminal") {
        return;
    }
    verify(
        "ldsminal x9, x10, [x11]",
        Inst::Ldsminal64 {
            rs: X9,
            rt: X10,
            rn: X11,
        },
    );
}
#[test]
fn sys_lduminalb() {
    if !native_macho_host("verify_against_system_as", "sys_lduminalb") {
        return;
    }
    verify(
        "lduminalb w12, w13, [x14]",
        Inst::Lduminalb {
            rs: W12,
            rt: W13,
            rn: X14,
        },
    );
}
#[test]
fn sys_lduminalh() {
    if !native_macho_host("verify_against_system_as", "sys_lduminalh") {
        return;
    }
    verify(
        "lduminalh w15, w16, [x17]",
        Inst::Lduminalh {
            rs: W15,
            rt: W16,
            rn: X17,
        },
    );
}
#[test]
fn sys_ldsminalb() {
    if !native_macho_host("verify_against_system_as", "sys_ldsminalb") {
        return;
    }
    verify(
        "ldsminalb w21, w22, [x23]",
        Inst::Ldsminalb {
            rs: W21,
            rt: W22,
            rn: X23,
        },
    );
}
#[test]
fn sys_ldsminalh() {
    if !native_macho_host("verify_against_system_as", "sys_ldsminalh") {
        return;
    }
    verify(
        "ldsminalh w27, w28, [x29]",
        Inst::Ldsminalh {
            rs: W27,
            rt: W28,
            rn: X29,
        },
    );
}
#[test]
fn sys_lduminal() {
    if !native_macho_host("verify_against_system_as", "sys_lduminal") {
        return;
    }
    verify(
        "lduminal w18, w19, [x20]",
        Inst::Lduminal32 {
            rs: W18,
            rt: W19,
            rn: X20,
        },
    );
}
#[test]
fn sys_ldclral() {
    if !native_macho_host("verify_against_system_as", "sys_ldclral") {
        return;
    }
    verify(
        "ldclral w12, w13, [x14]",
        Inst::Ldclral32 {
            rs: W12,
            rt: W13,
            rn: X14,
        },
    );
}
#[test]
fn sys_ldclralb() {
    if !native_macho_host("verify_against_system_as", "sys_ldclralb") {
        return;
    }
    verify(
        "ldclralb w6, w7, [x8]",
        Inst::Ldclralb {
            rs: W6,
            rt: W7,
            rn: X8,
        },
    );
}
#[test]
fn sys_ldclralh() {
    if !native_macho_host("verify_against_system_as", "sys_ldclralh") {
        return;
    }
    verify(
        "ldclralh w15, w16, [x17]",
        Inst::Ldclralh {
            rs: W15,
            rt: W16,
            rn: X17,
        },
    );
}
#[test]
fn sys_ldeoral() {
    if !native_macho_host("verify_against_system_as", "sys_ldeoral") {
        return;
    }
    verify(
        "ldeoral x9, x10, [x11]",
        Inst::Ldeoral64 {
            rs: X9,
            rt: X10,
            rn: X11,
        },
    );
}
#[test]
fn sys_ldeoralb() {
    if !native_macho_host("verify_against_system_as", "sys_ldeoralb") {
        return;
    }
    verify(
        "ldeoralb w3, w4, [x5]",
        Inst::Ldeoralb {
            rs: W3,
            rt: W4,
            rn: X5,
        },
    );
}
#[test]
fn sys_ldeoralh() {
    if !native_macho_host("verify_against_system_as", "sys_ldeoralh") {
        return;
    }
    verify(
        "ldeoralh w12, w13, [x14]",
        Inst::Ldeoralh {
            rs: W12,
            rt: W13,
            rn: X14,
        },
    );
}
#[test]
fn sys_ldsetal() {
    if !native_macho_host("verify_against_system_as", "sys_ldsetal") {
        return;
    }
    verify(
        "ldsetal w0, w1, [x2]",
        Inst::Ldsetal32 {
            rs: W0,
            rt: W1,
            rn: X2,
        },
    );
}
#[test]
fn sys_ldsetalb() {
    if !native_macho_host("verify_against_system_as", "sys_ldsetalb") {
        return;
    }
    verify(
        "ldsetalb w0, w1, [x2]",
        Inst::Ldsetalb {
            rs: W0,
            rt: W1,
            rn: X2,
        },
    );
}
#[test]
fn sys_ldsetalh() {
    if !native_macho_host("verify_against_system_as", "sys_ldsetalh") {
        return;
    }
    verify(
        "ldsetalh w9, w10, [x11]",
        Inst::Ldsetalh {
            rs: W9,
            rt: W10,
            rn: X11,
        },
    );
}
#[test]
fn sys_swpal() {
    if !native_macho_host("verify_against_system_as", "sys_swpal") {
        return;
    }
    verify(
        "swpal w0, w0, [x8]",
        Inst::Swpal32 {
            rs: W0,
            rt: W0,
            rn: X8,
        },
    );
}
#[test]
fn sys_swpalb() {
    if !native_macho_host("verify_against_system_as", "sys_swpalb") {
        return;
    }
    verify(
        "swpalb w8, w9, [x10]",
        Inst::Swpalb {
            rs: W8,
            rt: W9,
            rn: X10,
        },
    );
}
#[test]
fn sys_swpalh() {
    if !native_macho_host("verify_against_system_as", "sys_swpalh") {
        return;
    }
    verify(
        "swpalh w11, w12, [x13]",
        Inst::Swpalh {
            rs: W11,
            rt: W12,
            rn: X13,
        },
    );
}
#[test]
fn sys_casalb() {
    if !native_macho_host("verify_against_system_as", "sys_casalb") {
        return;
    }
    verify(
        "casalb w6, w7, [x8]",
        Inst::Casalb {
            rs: W6,
            rt: W7,
            rn: X8,
        },
    );
}
#[test]
fn sys_casalh() {
    if !native_macho_host("verify_against_system_as", "sys_casalh") {
        return;
    }
    verify(
        "casalh w9, w10, [x11]",
        Inst::Casalh {
            rs: W9,
            rt: W10,
            rn: X11,
        },
    );
}
#[test]
fn sys_subs_uxtb() {
    if !native_macho_host("verify_against_system_as", "sys_subs_uxtb") {
        return;
    }
    verify(
        "subs w10, w8, w9, uxtb",
        Inst::SubsExtReg {
            rd: W10,
            rn: W8,
            rm: W9,
            extend: RegExtend::Uxtb,
            amount: 0,
            sf: false,
        },
    );
}
#[test]
fn sys_subs_uxth() {
    if !native_macho_host("verify_against_system_as", "sys_subs_uxth") {
        return;
    }
    verify(
        "subs w11, w12, w13, uxth",
        Inst::SubsExtReg {
            rd: W11,
            rn: W12,
            rm: W13,
            extend: RegExtend::Uxth,
            amount: 0,
            sf: false,
        },
    );
}
#[test]
fn sys_subs_sxtb() {
    if !native_macho_host("verify_against_system_as", "sys_subs_sxtb") {
        return;
    }
    verify(
        "subs x14, x15, w16, sxtb",
        Inst::SubsExtReg {
            rd: X14,
            rn: X15,
            rm: W16,
            extend: RegExtend::Sxtb,
            amount: 0,
            sf: true,
        },
    );
}
#[test]
fn sys_subs_sxth() {
    if !native_macho_host("verify_against_system_as", "sys_subs_sxth") {
        return;
    }
    verify(
        "subs x17, x18, w19, sxth #1",
        Inst::SubsExtReg {
            rd: X17,
            rn: X18,
            rm: W19,
            extend: RegExtend::Sxth,
            amount: 1,
            sf: true,
        },
    );
}
#[test]
fn sys_swpal_x() {
    if !native_macho_host("verify_against_system_as", "sys_swpal_x") {
        return;
    }
    verify(
        "swpal x1, x2, [x3]",
        Inst::Swpal64 {
            rs: X1,
            rt: X2,
            rn: X3,
        },
    );
}
#[test]
fn sys_casal() {
    if !native_macho_host("verify_against_system_as", "sys_casal") {
        return;
    }
    verify(
        "casal w4, w5, [x6]",
        Inst::Casal32 {
            rs: W4,
            rt: W5,
            rn: X6,
        },
    );
}
#[test]
fn sys_casal_x() {
    if !native_macho_host("verify_against_system_as", "sys_casal_x") {
        return;
    }
    verify(
        "casal x7, x8, [x9]",
        Inst::Casal64 {
            rs: X7,
            rt: X8,
            rn: X9,
        },
    );
}
#[test]
fn sys_ldrsw_lit() {
    if !native_macho_host("verify_against_system_as", "sys_ldrsw_lit") {
        return;
    }
    verify("ldrsw x1, #8", Inst::LdrswLit { rt: X1, offset: 8 });
}
#[test]
fn sys_ldr_d() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_d") {
        return;
    }
    verify(
        "ldr d0, [x1]",
        Inst::LdrFpImm64 {
            rt: D0,
            rn: X1,
            offset: 0,
        },
    );
}
#[test]
fn sys_ldr_q() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_q") {
        return;
    }
    verify(
        "ldr q0, [sp, #16]",
        Inst::LdrFpImm128 {
            rt: FpReg::new(0),
            rn: SP,
            offset: 16,
        },
    );
}
#[test]
fn sys_ldr_h() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_h") {
        return;
    }
    verify(
        "ldr h2, [sp, #14]",
        Inst::LdrFpImm16 {
            rt: FpReg::new(2),
            rn: SP,
            offset: 14,
        },
    );
}
#[test]
fn sys_ldr_b() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_b") {
        return;
    }
    verify(
        "ldr b2, [sp, #15]",
        Inst::LdrFpImm8 {
            rt: FpReg::new(2),
            rn: SP,
            offset: 15,
        },
    );
}
#[test]
fn sys_str_q() {
    if !native_macho_host("verify_against_system_as", "sys_str_q") {
        return;
    }
    verify(
        "str q1, [x0]",
        Inst::StrFpImm128 {
            rt: FpReg::new(1),
            rn: X0,
            offset: 0,
        },
    );
}
#[test]
fn sys_str_h() {
    if !native_macho_host("verify_against_system_as", "sys_str_h") {
        return;
    }
    verify(
        "str h2, [sp, #14]",
        Inst::StrFpImm16 {
            rt: FpReg::new(2),
            rn: SP,
            offset: 14,
        },
    );
}
#[test]
fn sys_str_b() {
    if !native_macho_host("verify_against_system_as", "sys_str_b") {
        return;
    }
    verify(
        "str b2, [sp, #15]",
        Inst::StrFpImm8 {
            rt: FpReg::new(2),
            rn: SP,
            offset: 15,
        },
    );
}
#[test]
fn sys_ldr_q_lit() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_q_lit") {
        return;
    }
    verify(
        "ldr q0, #16",
        Inst::LdrFpLit128 {
            rt: FpReg::new(0),
            offset: 16,
        },
    );
}
#[test]
fn sys_ldr_q_reg() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_q_reg") {
        return;
    }
    verify(
        "ldr q0, [x1, x2]",
        Inst::LdrFpReg128 {
            rt: FpReg::new(0),
            rn: X1,
            rm: X2,
            extend: AddrExtend::Lsl,
            shift: false,
        },
    );
}
#[test]
fn sys_str_q_reg_uxtw() {
    if !native_macho_host("verify_against_system_as", "sys_str_q_reg_uxtw") {
        return;
    }
    verify(
        "str q1, [x3, w4, uxtw #4]",
        Inst::StrFpReg128 {
            rt: FpReg::new(1),
            rn: X3,
            rm: W4,
            extend: AddrExtend::Uxtw,
            shift: true,
        },
    );
}
#[test]
fn sys_ldr_q_post() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_q_post") {
        return;
    }
    verify(
        "ldr q0, [sp], #16",
        Inst::LdrFpPost128 {
            rt: FpReg::new(0),
            rn: SP,
            offset: 16,
        },
    );
}
#[test]
fn sys_str_q_pre() {
    if !native_macho_host("verify_against_system_as", "sys_str_q_pre") {
        return;
    }
    verify(
        "str q1, [sp, #-16]!",
        Inst::StrFpPre128 {
            rt: FpReg::new(1),
            rn: SP,
            offset: -16,
        },
    );
}
#[test]
fn sys_str_d_off() {
    if !native_macho_host("verify_against_system_as", "sys_str_d_off") {
        return;
    }
    verify(
        "str d2, [x3, #16]",
        Inst::StrFpImm64 {
            rt: D2,
            rn: X3,
            offset: 16,
        },
    );
}
#[test]
fn sys_ldr_s_reg() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_s_reg") {
        return;
    }
    verify(
        "ldr s4, [x5, x6]",
        Inst::LdrFpReg32 {
            rt: S4,
            rn: X5,
            rm: X6,
            extend: AddrExtend::Lsl,
            shift: false,
        },
    );
}
#[test]
fn sys_str_s_reg_uxtw() {
    if !native_macho_host("verify_against_system_as", "sys_str_s_reg_uxtw") {
        return;
    }
    verify(
        "str s7, [x8, w9, uxtw #2]",
        Inst::StrFpReg32 {
            rt: S7,
            rn: X8,
            rm: W9,
            extend: AddrExtend::Uxtw,
            shift: true,
        },
    );
}
#[test]
fn sys_ldr_d_lit() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_d_lit") {
        return;
    }
    verify("ldr d10, #8", Inst::LdrFpLit64 { rt: D10, offset: 8 });
}
#[test]
fn sys_ldr_d_post() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_d_post") {
        return;
    }
    verify(
        "ldr d0, [sp], #8",
        Inst::LdrFpPost64 {
            rt: D0,
            rn: SP,
            offset: 8,
        },
    );
}
#[test]
fn sys_str_s_pre() {
    if !native_macho_host("verify_against_system_as", "sys_str_s_pre") {
        return;
    }
    verify(
        "str s3, [sp, #-8]!",
        Inst::StrFpPre32 {
            rt: S3,
            rn: SP,
            offset: -8,
        },
    );
}

#[test]
fn sys_str_s_neg_offset() {
    if !native_macho_host("verify_against_system_as", "sys_str_s_neg_offset") {
        return;
    }
    verify(
        "str s8, [x29, #-4]",
        Inst::SturFp32 {
            rt: S8,
            rn: X29,
            offset: -4,
        },
    );
}

#[test]
fn sys_ldr_s_neg_offset() {
    if !native_macho_host("verify_against_system_as", "sys_ldr_s_neg_offset") {
        return;
    }
    verify(
        "ldr s9, [x29, #-4]",
        Inst::LdurFp32 {
            rt: S9,
            rn: X29,
            offset: -4,
        },
    );
}

// ---- Address generation ----

#[test]
fn sys_adr() {
    if !native_macho_host("verify_against_system_as", "sys_adr") {
        return;
    }
    verify("adr x0, #8", Inst::Adr { rd: X0, imm: 8 });
}
#[test]
fn sys_adrp_tlvp() {
    if !native_macho_host("verify_against_system_as", "sys_adrp_tlvp") {
        return;
    }
    verify(
        "adrp x0, _tls_counter@TLVPPAGE",
        Inst::Adrp { rd: X0, imm: 0 },
    );
}

// ---- Load/Store pair ----

#[test]
fn sys_stp_pre() {
    if !native_macho_host("verify_against_system_as", "sys_stp_pre") {
        return;
    }
    verify(
        "stp x29, x30, [sp, #-32]!",
        Inst::StpPre64 {
            rt1: X29,
            rt2: X30,
            rn: SP,
            offset: -32,
        },
    );
}
#[test]
fn sys_ldp_post() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_post") {
        return;
    }
    verify(
        "ldp x29, x30, [sp], #32",
        Inst::LdpPost64 {
            rt1: X29,
            rt2: X30,
            rn: SP,
            offset: 32,
        },
    );
}
#[test]
fn sys_stp_off() {
    if !native_macho_host("verify_against_system_as", "sys_stp_off") {
        return;
    }
    verify(
        "stp x19, x20, [sp, #32]",
        Inst::StpOff64 {
            rt1: X19,
            rt2: X20,
            rn: SP,
            offset: 32,
        },
    );
}
#[test]
fn sys_ldp_off() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_off") {
        return;
    }
    verify(
        "ldp x21, x22, [sp, #48]",
        Inst::LdpOff64 {
            rt1: X21,
            rt2: X22,
            rn: SP,
            offset: 48,
        },
    );
}
#[test]
fn sys_ldp_off32() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_off32") {
        return;
    }
    verify(
        "ldp w9, w8, [x8]",
        Inst::LdpOff32 {
            rt1: W9,
            rt2: W8,
            rn: X8,
            offset: 0,
        },
    );
}
#[test]
fn sys_stp_off32() {
    if !native_macho_host("verify_against_system_as", "sys_stp_off32") {
        return;
    }
    verify(
        "stp w1, w2, [sp, #16]",
        Inst::StpOff32 {
            rt1: W1,
            rt2: W2,
            rn: SP,
            offset: 16,
        },
    );
}
#[test]
fn sys_ldp_post32() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_post32") {
        return;
    }
    verify(
        "ldp w9, w8, [sp], #8",
        Inst::LdpPost32 {
            rt1: W9,
            rt2: W8,
            rn: SP,
            offset: 8,
        },
    );
}
#[test]
fn sys_ldp_pre32() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_pre32") {
        return;
    }
    verify(
        "ldp w9, w8, [sp, #-8]!",
        Inst::LdpPre32 {
            rt1: W9,
            rt2: W8,
            rn: SP,
            offset: -8,
        },
    );
}
#[test]
fn sys_ldp_d_pre() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_d_pre") {
        return;
    }
    verify(
        "ldp d8, d9, [sp, #-16]!",
        Inst::LdpFpPre64 {
            rt1: D8,
            rt2: D9,
            rn: SP,
            offset: -16,
        },
    );
}
#[test]
fn sys_stp_d_post() {
    if !native_macho_host("verify_against_system_as", "sys_stp_d_post") {
        return;
    }
    verify(
        "stp d10, d11, [sp], #16",
        Inst::StpFpPost64 {
            rt1: D10,
            rt2: D11,
            rn: SP,
            offset: 16,
        },
    );
}
#[test]
fn sys_ldp_d_off() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_d_off") {
        return;
    }
    verify(
        "ldp d12, d13, [sp, #32]",
        Inst::LdpFpOff64 {
            rt1: D12,
            rt2: D13,
            rn: SP,
            offset: 32,
        },
    );
}
#[test]
fn sys_stp_s_post() {
    if !native_macho_host("verify_against_system_as", "sys_stp_s_post") {
        return;
    }
    verify(
        "stp s0, s1, [sp], #8",
        Inst::StpFpPost32 {
            rt1: S0,
            rt2: S1,
            rn: SP,
            offset: 8,
        },
    );
}
#[test]
fn sys_ldp_s_pre() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_s_pre") {
        return;
    }
    verify(
        "ldp s2, s3, [sp, #-8]!",
        Inst::LdpFpPre32 {
            rt1: S2,
            rt2: S3,
            rn: SP,
            offset: -8,
        },
    );
}
#[test]
fn sys_stp_q_pre() {
    if !native_macho_host("verify_against_system_as", "sys_stp_q_pre") {
        return;
    }
    verify(
        "stp q0, q1, [sp, #-32]!",
        Inst::StpFpPre128 {
            rt1: FpReg::new(0),
            rt2: FpReg::new(1),
            rn: SP,
            offset: -32,
        },
    );
}
#[test]
fn sys_ldp_q_post() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_q_post") {
        return;
    }
    verify(
        "ldp q2, q3, [sp], #32",
        Inst::LdpFpPost128 {
            rt1: FpReg::new(2),
            rt2: FpReg::new(3),
            rn: SP,
            offset: 32,
        },
    );
}
#[test]
fn sys_ldp_q_off() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_q_off") {
        return;
    }
    verify(
        "ldp q4, q5, [sp, #64]",
        Inst::LdpFpOff128 {
            rt1: FpReg::new(4),
            rt2: FpReg::new(5),
            rn: SP,
            offset: 64,
        },
    );
}
#[test]
fn sys_ldp_s_off() {
    if !native_macho_host("verify_against_system_as", "sys_ldp_s_off") {
        return;
    }
    verify(
        "ldp s4, s5, [sp, #16]",
        Inst::LdpFpOff32 {
            rt1: S4,
            rt2: S5,
            rn: SP,
            offset: 16,
        },
    );
}

// ---- FP arithmetic ----

#[test]
fn sys_fadd_d() {
    if !native_macho_host("verify_against_system_as", "sys_fadd_d") {
        return;
    }
    verify(
        "fadd d5, d6, d7",
        Inst::FaddD {
            rd: D5,
            rn: D6,
            rm: D7,
        },
    );
}
#[test]
fn sys_fsub_d() {
    if !native_macho_host("verify_against_system_as", "sys_fsub_d") {
        return;
    }
    verify(
        "fsub d5, d6, d7",
        Inst::FsubD {
            rd: D5,
            rn: D6,
            rm: D7,
        },
    );
}
#[test]
fn sys_fmul_d() {
    if !native_macho_host("verify_against_system_as", "sys_fmul_d") {
        return;
    }
    verify(
        "fmul d5, d6, d7",
        Inst::FmulD {
            rd: D5,
            rn: D6,
            rm: D7,
        },
    );
}
#[test]
fn sys_fdiv_d() {
    if !native_macho_host("verify_against_system_as", "sys_fdiv_d") {
        return;
    }
    verify(
        "fdiv d5, d6, d7",
        Inst::FdivD {
            rd: D5,
            rn: D6,
            rm: D7,
        },
    );
}
#[test]
fn sys_fadd_s() {
    if !native_macho_host("verify_against_system_as", "sys_fadd_s") {
        return;
    }
    verify(
        "fadd s5, s6, s7",
        Inst::FaddS {
            rd: S5,
            rn: S6,
            rm: S7,
        },
    );
}
#[test]
fn sys_fadd_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fadd_2d") {
        return;
    }
    verify(
        "fadd.2d v0, v1, v2",
        Inst::FaddV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fadd_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fadd_4s") {
        return;
    }
    verify(
        "fadd.4s v0, v1, v2",
        Inst::FaddV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_add_4s() {
    if !native_macho_host("verify_against_system_as", "sys_add_4s") {
        return;
    }
    verify(
        "add.4s v0, v1, v2",
        Inst::AddV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_addp_2d() {
    if !native_macho_host("verify_against_system_as", "sys_addp_2d") {
        return;
    }
    verify(
        "addp.2d v0, v1, v2",
        Inst::AddpV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_addp_16b() {
    if !native_macho_host("verify_against_system_as", "sys_addp_16b") {
        return;
    }
    verify(
        "addp.16b v6, v7, v8",
        Inst::AddpV16B {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_addp_8h() {
    if !native_macho_host("verify_against_system_as", "sys_addp_8h") {
        return;
    }
    verify(
        "addp.8h v0, v1, v2",
        Inst::AddpV8H {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_addp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_addp_4s") {
        return;
    }
    verify(
        "addp.4s v0, v1, v2",
        Inst::AddpV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fmax_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fmax_2d") {
        return;
    }
    verify(
        "fmax.2d v0, v0, v1",
        Inst::FmaxV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
            rm: FpReg::new(1),
        },
    );
}
#[test]
fn sys_fmax_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmax_4s") {
        return;
    }
    verify(
        "fmax.4s v0, v0, v1",
        Inst::FmaxV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
            rm: FpReg::new(1),
        },
    );
}
#[test]
fn sys_fmaxnm_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxnm_4s") {
        return;
    }
    verify(
        "fmaxnm.4s v0, v1, v2",
        Inst::FmaxnmV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fmaxnm_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxnm_2d") {
        return;
    }
    verify(
        "fmaxnm.2d v0, v0, v1",
        Inst::FmaxnmV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
            rm: FpReg::new(1),
        },
    );
}
#[test]
fn sys_fmin_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fmin_2d") {
        return;
    }
    verify(
        "fmin.2d v2, v3, v4",
        Inst::FminV2D {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
            rm: FpReg::new(4),
        },
    );
}
#[test]
fn sys_fmin_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmin_4s") {
        return;
    }
    verify(
        "fmin.4s v2, v3, v4",
        Inst::FminV4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
            rm: FpReg::new(4),
        },
    );
}
#[test]
fn sys_fminnm_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fminnm_4s") {
        return;
    }
    verify(
        "fminnm.4s v3, v4, v5",
        Inst::FminnmV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fminnm_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fminnm_2d") {
        return;
    }
    verify(
        "fminnm.2d v2, v3, v4",
        Inst::FminnmV2D {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
            rm: FpReg::new(4),
        },
    );
}
#[test]
fn sys_smax_4s() {
    if !native_macho_host("verify_against_system_as", "sys_smax_4s") {
        return;
    }
    verify(
        "smax.4s v5, v6, v7",
        Inst::SmaxV4S {
            rd: FpReg::new(5),
            rn: FpReg::new(6),
            rm: FpReg::new(7),
        },
    );
}
#[test]
fn sys_smaxp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_smaxp_4s") {
        return;
    }
    verify(
        "smaxp.4s v6, v7, v8",
        Inst::SmaxpV4S {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_smaxp_16b() {
    if !native_macho_host("verify_against_system_as", "sys_smaxp_16b") {
        return;
    }
    verify(
        "smaxp.16b v6, v7, v8",
        Inst::SmaxpV16B {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_smaxp_8h() {
    if !native_macho_host("verify_against_system_as", "sys_smaxp_8h") {
        return;
    }
    verify(
        "smaxp.8h v6, v7, v8",
        Inst::SmaxpV8H {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_smin_4s() {
    if !native_macho_host("verify_against_system_as", "sys_smin_4s") {
        return;
    }
    verify(
        "smin.4s v8, v9, v10",
        Inst::SminV4S {
            rd: FpReg::new(8),
            rn: FpReg::new(9),
            rm: FpReg::new(10),
        },
    );
}
#[test]
fn sys_sminp_16b() {
    if !native_macho_host("verify_against_system_as", "sys_sminp_16b") {
        return;
    }
    verify(
        "sminp.16b v9, v10, v11",
        Inst::SminpV16B {
            rd: FpReg::new(9),
            rn: FpReg::new(10),
            rm: FpReg::new(11),
        },
    );
}
#[test]
fn sys_sminp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_sminp_4s") {
        return;
    }
    verify(
        "sminp.4s v9, v10, v11",
        Inst::SminpV4S {
            rd: FpReg::new(9),
            rn: FpReg::new(10),
            rm: FpReg::new(11),
        },
    );
}
#[test]
fn sys_sminp_8h() {
    if !native_macho_host("verify_against_system_as", "sys_sminp_8h") {
        return;
    }
    verify(
        "sminp.8h v9, v10, v11",
        Inst::SminpV8H {
            rd: FpReg::new(9),
            rn: FpReg::new(10),
            rm: FpReg::new(11),
        },
    );
}
#[test]
fn sys_umax_4s() {
    if !native_macho_host("verify_against_system_as", "sys_umax_4s") {
        return;
    }
    verify(
        "umax.4s v0, v0, v1",
        Inst::UmaxV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
            rm: FpReg::new(1),
        },
    );
}
#[test]
fn sys_umaxp_16b() {
    if !native_macho_host("verify_against_system_as", "sys_umaxp_16b") {
        return;
    }
    verify(
        "umaxp.16b v0, v1, v2",
        Inst::UmaxpV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_umaxp_8h() {
    if !native_macho_host("verify_against_system_as", "sys_umaxp_8h") {
        return;
    }
    verify(
        "umaxp.8h v0, v1, v2",
        Inst::UmaxpV8H {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_umaxp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_umaxp_4s") {
        return;
    }
    verify(
        "umaxp.4s v0, v1, v2",
        Inst::UmaxpV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_uminp_16b() {
    if !native_macho_host("verify_against_system_as", "sys_uminp_16b") {
        return;
    }
    verify(
        "uminp.16b v3, v4, v5",
        Inst::UminpV16B {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_uminp_8h() {
    if !native_macho_host("verify_against_system_as", "sys_uminp_8h") {
        return;
    }
    verify(
        "uminp.8h v3, v4, v5",
        Inst::UminpV8H {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_umin_4s() {
    if !native_macho_host("verify_against_system_as", "sys_umin_4s") {
        return;
    }
    verify(
        "umin.4s v2, v3, v4",
        Inst::UminV4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
            rm: FpReg::new(4),
        },
    );
}
#[test]
fn sys_uminp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_uminp_4s") {
        return;
    }
    verify(
        "uminp.4s v3, v4, v5",
        Inst::UminpV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_addv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_addv_4s") {
        return;
    }
    verify(
        "addv.4s s0, v0",
        Inst::AddvV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_addv_16b() {
    if !native_macho_host("verify_against_system_as", "sys_addv_16b") {
        return;
    }
    verify(
        "addv.16b b0, v0",
        Inst::AddvV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_addv_8h() {
    if !native_macho_host("verify_against_system_as", "sys_addv_8h") {
        return;
    }
    verify(
        "addv.8h h0, v0",
        Inst::AddvV8H {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_faddp_2d() {
    if !native_macho_host("verify_against_system_as", "sys_faddp_2d") {
        return;
    }
    verify(
        "faddp.2d v0, v1, v2",
        Inst::FaddpV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_faddp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_faddp_4s") {
        return;
    }
    verify(
        "faddp.4s v0, v1, v2",
        Inst::FaddpV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fmaxp_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxp_2d") {
        return;
    }
    verify(
        "fmaxp.2d v3, v4, v5",
        Inst::FmaxpV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fmaxp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxp_4s") {
        return;
    }
    verify(
        "fmaxp.4s v0, v1, v2",
        Inst::FmaxpV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fminp_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fminp_2d") {
        return;
    }
    verify(
        "fminp.2d v6, v7, v8",
        Inst::FminpV2D {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_fminp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fminp_4s") {
        return;
    }
    verify(
        "fminp.4s v3, v4, v5",
        Inst::FminpV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fmaxnmp_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxnmp_2d") {
        return;
    }
    verify(
        "fmaxnmp.2d v0, v1, v2",
        Inst::FmaxnmpV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fmaxnmp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxnmp_4s") {
        return;
    }
    verify(
        "fmaxnmp.4s v0, v1, v2",
        Inst::FmaxnmpV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fminnmp_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fminnmp_2d") {
        return;
    }
    verify(
        "fminnmp.2d v3, v4, v5",
        Inst::FminnmpV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fminnmp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fminnmp_4s") {
        return;
    }
    verify(
        "fminnmp.4s v3, v4, v5",
        Inst::FminnmpV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fmla_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmla_4s") {
        return;
    }
    verify(
        "fmla.4s v0, v1, v2",
        Inst::FmlaV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fmla_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fmla_2d") {
        return;
    }
    verify(
        "fmla.2d v0, v1, v2",
        Inst::FmlaV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fmls_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmls_4s") {
        return;
    }
    verify(
        "fmls.4s v3, v4, v5",
        Inst::FmlsV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fmls_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fmls_2d") {
        return;
    }
    verify(
        "fmls.2d v3, v4, v5",
        Inst::FmlsV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_faddp_2s() {
    if !native_macho_host("verify_against_system_as", "sys_faddp_2s") {
        return;
    }
    verify(
        "faddp.2s s3, v4",
        Inst::FaddpV2S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_faddp_2d_scalar() {
    if !native_macho_host("verify_against_system_as", "sys_faddp_2d_scalar") {
        return;
    }
    verify(
        "faddp.2d d0, v0",
        Inst::FaddpV2DScalar {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_fmaxp_2d_scalar() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxp_2d_scalar") {
        return;
    }
    verify(
        "fmaxp.2d d1, v2",
        Inst::FmaxpV2DScalar {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fminp_2d_scalar() {
    if !native_macho_host("verify_against_system_as", "sys_fminp_2d_scalar") {
        return;
    }
    verify(
        "fminp.2d d3, v4",
        Inst::FminpV2DScalar {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_fmaxnmp_2d_scalar() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxnmp_2d_scalar") {
        return;
    }
    verify(
        "fmaxnmp.2d d0, v0",
        Inst::FmaxnmpV2DScalar {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_fminnmp_2d_scalar() {
    if !native_macho_host("verify_against_system_as", "sys_fminnmp_2d_scalar") {
        return;
    }
    verify(
        "fminnmp.2d d1, v2",
        Inst::FminnmpV2DScalar {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fmaxv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxv_4s") {
        return;
    }
    verify(
        "fmaxv.4s s1, v2",
        Inst::FmaxvV4S {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fmaxnmv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmaxnmv_4s") {
        return;
    }
    verify(
        "fmaxnmv.4s s1, v2",
        Inst::FmaxnmvV4S {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fminv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fminv_4s") {
        return;
    }
    verify(
        "fminv.4s s3, v4",
        Inst::FminvV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_fminnmv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fminnmv_4s") {
        return;
    }
    verify(
        "fminnmv.4s s3, v4",
        Inst::FminnmvV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_umaxv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_umaxv_4s") {
        return;
    }
    verify(
        "umaxv.4s s1, v2",
        Inst::UmaxvV4S {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_umaxv_16b() {
    if !native_macho_host("verify_against_system_as", "sys_umaxv_16b") {
        return;
    }
    verify(
        "umaxv.16b b0, v0",
        Inst::UmaxvV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_umaxv_8h() {
    if !native_macho_host("verify_against_system_as", "sys_umaxv_8h") {
        return;
    }
    verify(
        "umaxv.8h h0, v0",
        Inst::UmaxvV8H {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_smaxv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_smaxv_4s") {
        return;
    }
    verify(
        "smaxv.4s s3, v4",
        Inst::SmaxvV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_smaxv_16b() {
    if !native_macho_host("verify_against_system_as", "sys_smaxv_16b") {
        return;
    }
    verify(
        "smaxv.16b b0, v0",
        Inst::SmaxvV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_smaxv_8h() {
    if !native_macho_host("verify_against_system_as", "sys_smaxv_8h") {
        return;
    }
    verify(
        "smaxv.8h h0, v0",
        Inst::SmaxvV8H {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_uminv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_uminv_4s") {
        return;
    }
    verify(
        "uminv.4s s1, v2",
        Inst::UminvV4S {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_uminv_16b() {
    if !native_macho_host("verify_against_system_as", "sys_uminv_16b") {
        return;
    }
    verify(
        "uminv.16b b0, v0",
        Inst::UminvV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_uminv_8h() {
    if !native_macho_host("verify_against_system_as", "sys_uminv_8h") {
        return;
    }
    verify(
        "uminv.8h h0, v0",
        Inst::UminvV8H {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_sminv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_sminv_4s") {
        return;
    }
    verify(
        "sminv.4s s3, v4",
        Inst::SminvV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_sminv_16b() {
    if !native_macho_host("verify_against_system_as", "sys_sminv_16b") {
        return;
    }
    verify(
        "sminv.16b b0, v0",
        Inst::SminvV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_sminv_8h() {
    if !native_macho_host("verify_against_system_as", "sys_sminv_8h") {
        return;
    }
    verify(
        "sminv.8h h0, v0",
        Inst::SminvV8H {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_fsub_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fsub_2d") {
        return;
    }
    verify(
        "fsub.2d v3, v4, v5",
        Inst::FsubV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fsub_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fsub_4s") {
        return;
    }
    verify(
        "fsub.4s v3, v4, v5",
        Inst::FsubV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_sub_4s() {
    if !native_macho_host("verify_against_system_as", "sys_sub_4s") {
        return;
    }
    verify(
        "sub.4s v3, v4, v5",
        Inst::SubV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fmul_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fmul_2d") {
        return;
    }
    verify(
        "fmul.2d v6, v7, v8",
        Inst::FmulV2D {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_fmul_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fmul_4s") {
        return;
    }
    verify(
        "fmul.4s v6, v7, v8",
        Inst::FmulV4S {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_fdiv_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fdiv_2d") {
        return;
    }
    verify(
        "fdiv.2d v9, v10, v11",
        Inst::FdivV2D {
            rd: FpReg::new(9),
            rn: FpReg::new(10),
            rm: FpReg::new(11),
        },
    );
}
#[test]
fn sys_fabd_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fabd_2d") {
        return;
    }
    verify(
        "fabd.2d v0, v1, v2",
        Inst::FabdV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fdiv_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fdiv_4s") {
        return;
    }
    verify(
        "fdiv.4s v9, v10, v11",
        Inst::FdivV4S {
            rd: FpReg::new(9),
            rn: FpReg::new(10),
            rm: FpReg::new(11),
        },
    );
}
#[test]
fn sys_fabs_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fabs_4s") {
        return;
    }
    verify(
        "fabs.4s v0, v0",
        Inst::FabsV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_fneg_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fneg_4s") {
        return;
    }
    verify(
        "fneg.4s v6, v7",
        Inst::FnegV4S {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
        },
    );
}
#[test]
fn sys_fsqrt_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fsqrt_4s") {
        return;
    }
    verify(
        "fsqrt.4s v1, v2",
        Inst::FsqrtV4S {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fabs_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fabs_2d") {
        return;
    }
    verify(
        "fabs.2d v0, v0",
        Inst::FabsV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_fneg_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fneg_2d") {
        return;
    }
    verify(
        "fneg.2d v3, v4",
        Inst::FnegV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_fsqrt_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fsqrt_2d") {
        return;
    }
    verify(
        "fsqrt.2d v1, v2",
        Inst::FsqrtV2D {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_scvtf_2d() {
    if !native_macho_host("verify_against_system_as", "sys_scvtf_2d") {
        return;
    }
    verify(
        "scvtf.2d v0, v0",
        Inst::ScvtfV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_ucvtf_2d() {
    if !native_macho_host("verify_against_system_as", "sys_ucvtf_2d") {
        return;
    }
    verify(
        "ucvtf.2d v1, v2",
        Inst::UcvtfV2D {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fcvtzs_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fcvtzs_2d") {
        return;
    }
    verify(
        "fcvtzs.2d v3, v4",
        Inst::FcvtzsV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_fcvtzu_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fcvtzu_2d") {
        return;
    }
    verify(
        "fcvtzu.2d v5, v6",
        Inst::FcvtzuV2D {
            rd: FpReg::new(5),
            rn: FpReg::new(6),
        },
    );
}
#[test]
fn sys_frecpe_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frecpe_2d") {
        return;
    }
    verify(
        "frecpe.2d v0, v0",
        Inst::FrecpeV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_frecps_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frecps_2d") {
        return;
    }
    verify(
        "frecps.2d v1, v2, v3",
        Inst::FrecpsV2D {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
            rm: FpReg::new(3),
        },
    );
}
#[test]
fn sys_frsqrte_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frsqrte_2d") {
        return;
    }
    verify(
        "frsqrte.2d v4, v5",
        Inst::FrsqrteV2D {
            rd: FpReg::new(4),
            rn: FpReg::new(5),
        },
    );
}
#[test]
fn sys_frsqrts_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frsqrts_2d") {
        return;
    }
    verify(
        "frsqrts.2d v6, v7, v8",
        Inst::FrsqrtsV2D {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_frintn_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frintn_2d") {
        return;
    }
    verify(
        "frintn.2d v0, v0",
        Inst::FrintnV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_frintm_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frintm_2d") {
        return;
    }
    verify(
        "frintm.2d v1, v2",
        Inst::FrintmV2D {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_frintp_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frintp_2d") {
        return;
    }
    verify(
        "frintp.2d v3, v4",
        Inst::FrintpV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
        },
    );
}
#[test]
fn sys_frintz_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frintz_2d") {
        return;
    }
    verify(
        "frintz.2d v5, v6",
        Inst::FrintzV2D {
            rd: FpReg::new(5),
            rn: FpReg::new(6),
        },
    );
}
#[test]
fn sys_frinta_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frinta_2d") {
        return;
    }
    verify(
        "frinta.2d v7, v8",
        Inst::FrintaV2D {
            rd: FpReg::new(7),
            rn: FpReg::new(8),
        },
    );
}
#[test]
fn sys_frinti_2d() {
    if !native_macho_host("verify_against_system_as", "sys_frinti_2d") {
        return;
    }
    verify(
        "frinti.2d v9, v10",
        Inst::FrintiV2D {
            rd: FpReg::new(9),
            rn: FpReg::new(10),
        },
    );
}
#[test]
fn sys_scvtf_4s() {
    if !native_macho_host("verify_against_system_as", "sys_scvtf_4s") {
        return;
    }
    verify(
        "scvtf.4s v0, v1",
        Inst::ScvtfV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
        },
    );
}
#[test]
fn sys_ucvtf_4s() {
    if !native_macho_host("verify_against_system_as", "sys_ucvtf_4s") {
        return;
    }
    verify(
        "ucvtf.4s v2, v3",
        Inst::UcvtfV4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
        },
    );
}
#[test]
fn sys_fcvtzs_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fcvtzs_4s") {
        return;
    }
    verify(
        "fcvtzs.4s v4, v5",
        Inst::FcvtzsV4S {
            rd: FpReg::new(4),
            rn: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fcvtzu_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fcvtzu_4s") {
        return;
    }
    verify(
        "fcvtzu.4s v6, v7",
        Inst::FcvtzuV4S {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
        },
    );
}
#[test]
fn sys_frecpe_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frecpe_4s") {
        return;
    }
    verify(
        "frecpe.4s v0, v1",
        Inst::FrecpeV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
        },
    );
}
#[test]
fn sys_frecps_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frecps_4s") {
        return;
    }
    verify(
        "frecps.4s v2, v3, v4",
        Inst::FrecpsV4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
            rm: FpReg::new(4),
        },
    );
}
#[test]
fn sys_frsqrte_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frsqrte_4s") {
        return;
    }
    verify(
        "frsqrte.4s v5, v6",
        Inst::FrsqrteV4S {
            rd: FpReg::new(5),
            rn: FpReg::new(6),
        },
    );
}
#[test]
fn sys_frsqrts_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frsqrts_4s") {
        return;
    }
    verify(
        "frsqrts.4s v7, v8, v9",
        Inst::FrsqrtsV4S {
            rd: FpReg::new(7),
            rn: FpReg::new(8),
            rm: FpReg::new(9),
        },
    );
}
#[test]
fn sys_frintn_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frintn_4s") {
        return;
    }
    verify(
        "frintn.4s v0, v1",
        Inst::FrintnV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
        },
    );
}
#[test]
fn sys_frintm_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frintm_4s") {
        return;
    }
    verify(
        "frintm.4s v2, v3",
        Inst::FrintmV4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
        },
    );
}
#[test]
fn sys_frintp_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frintp_4s") {
        return;
    }
    verify(
        "frintp.4s v4, v5",
        Inst::FrintpV4S {
            rd: FpReg::new(4),
            rn: FpReg::new(5),
        },
    );
}
#[test]
fn sys_frintz_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frintz_4s") {
        return;
    }
    verify(
        "frintz.4s v6, v7",
        Inst::FrintzV4S {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
        },
    );
}
#[test]
fn sys_frinta_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frinta_4s") {
        return;
    }
    verify(
        "frinta.4s v0, v1",
        Inst::FrintaV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
        },
    );
}
#[test]
fn sys_frinti_4s() {
    if !native_macho_host("verify_against_system_as", "sys_frinti_4s") {
        return;
    }
    verify(
        "frinti.4s v2, v3",
        Inst::FrintiV4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
        },
    );
}
#[test]
fn sys_and_16b() {
    if !native_macho_host("verify_against_system_as", "sys_and_16b") {
        return;
    }
    verify(
        "and.16b v6, v7, v8",
        Inst::AndV16B {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_bif_16b() {
    if !native_macho_host("verify_against_system_as", "sys_bif_16b") {
        return;
    }
    verify(
        "bif.16b v0, v1, v2",
        Inst::BifV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_bit_16b() {
    if !native_macho_host("verify_against_system_as", "sys_bit_16b") {
        return;
    }
    verify(
        "bit.16b v3, v4, v5",
        Inst::BitV16B {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_bic_16b() {
    if !native_macho_host("verify_against_system_as", "sys_bic_16b") {
        return;
    }
    verify(
        "bic.16b v5, v6, v7",
        Inst::BicV16B {
            rd: FpReg::new(5),
            rn: FpReg::new(6),
            rm: FpReg::new(7),
        },
    );
}
#[test]
fn sys_bsl_16b() {
    if !native_macho_host("verify_against_system_as", "sys_bsl_16b") {
        return;
    }
    verify(
        "bsl.16b v6, v7, v8",
        Inst::BslV16B {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_cmeq_4s() {
    if !native_macho_host("verify_against_system_as", "sys_cmeq_4s") {
        return;
    }
    verify(
        "cmeq.4s v0, v0, v1",
        Inst::CmeqV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
            rm: FpReg::new(1),
        },
    );
}
#[test]
fn sys_fcmeq_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fcmeq_4s") {
        return;
    }
    verify(
        "fcmeq.4s v0, v1, v2",
        Inst::FcmeqV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fcmeq_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fcmeq_2d") {
        return;
    }
    verify(
        "fcmeq.2d v0, v1, v2",
        Inst::FcmeqV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_cmhs_4s() {
    if !native_macho_host("verify_against_system_as", "sys_cmhs_4s") {
        return;
    }
    verify(
        "cmhs.4s v0, v0, v1",
        Inst::CmhsV4S {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
            rm: FpReg::new(1),
        },
    );
}
#[test]
fn sys_cmhi_4s() {
    if !native_macho_host("verify_against_system_as", "sys_cmhi_4s") {
        return;
    }
    verify(
        "cmhi.4s v2, v3, v4",
        Inst::CmhiV4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
            rm: FpReg::new(4),
        },
    );
}
#[test]
fn sys_cmge_4s() {
    if !native_macho_host("verify_against_system_as", "sys_cmge_4s") {
        return;
    }
    verify(
        "cmge.4s v5, v6, v7",
        Inst::CmgeV4S {
            rd: FpReg::new(5),
            rn: FpReg::new(6),
            rm: FpReg::new(7),
        },
    );
}
#[test]
fn sys_fcmge_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fcmge_4s") {
        return;
    }
    verify(
        "fcmge.4s v3, v4, v5",
        Inst::FcmgeV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_fcmge_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fcmge_2d") {
        return;
    }
    verify(
        "fcmge.2d v3, v4, v5",
        Inst::FcmgeV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_cmgt_4s() {
    if !native_macho_host("verify_against_system_as", "sys_cmgt_4s") {
        return;
    }
    verify(
        "cmgt.4s v2, v3, v4",
        Inst::CmgtV4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
            rm: FpReg::new(4),
        },
    );
}
#[test]
fn sys_fcmgt_4s() {
    if !native_macho_host("verify_against_system_as", "sys_fcmgt_4s") {
        return;
    }
    verify(
        "fcmgt.4s v6, v7, v8",
        Inst::FcmgtV4S {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_fcmgt_2d() {
    if !native_macho_host("verify_against_system_as", "sys_fcmgt_2d") {
        return;
    }
    verify(
        "fcmgt.2d v6, v7, v8",
        Inst::FcmgtV2D {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_fcmge_2d_zero() {
    if !native_macho_host("verify_against_system_as", "sys_fcmge_2d_zero") {
        return;
    }
    verify(
        "fcmge.2d v0, v0, #0.0",
        Inst::FcmgeZeroV2D {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
        },
    );
}
#[test]
fn sys_fcmgt_2d_zero() {
    if !native_macho_host("verify_against_system_as", "sys_fcmgt_2d_zero") {
        return;
    }
    verify(
        "fcmgt.2d v1, v1, #0.0",
        Inst::FcmgtZeroV2D {
            rd: FpReg::new(1),
            rn: FpReg::new(1),
        },
    );
}
#[test]
fn sys_fcmle_2d_zero() {
    if !native_macho_host("verify_against_system_as", "sys_fcmle_2d_zero") {
        return;
    }
    verify(
        "fcmle.2d v2, v2, #0.0",
        Inst::FcmleZeroV2D {
            rd: FpReg::new(2),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_fcmlt_2d_zero() {
    if !native_macho_host("verify_against_system_as", "sys_fcmlt_2d_zero") {
        return;
    }
    verify(
        "fcmlt.2d v3, v3, #0.0",
        Inst::FcmltZeroV2D {
            rd: FpReg::new(3),
            rn: FpReg::new(3),
        },
    );
}
#[test]
fn sys_orr_16b() {
    if !native_macho_host("verify_against_system_as", "sys_orr_16b") {
        return;
    }
    verify(
        "orr.16b v9, v10, v11",
        Inst::OrrV16B {
            rd: FpReg::new(9),
            rn: FpReg::new(10),
            rm: FpReg::new(11),
        },
    );
}
#[test]
fn sys_eor_16b() {
    if !native_macho_host("verify_against_system_as", "sys_eor_16b") {
        return;
    }
    verify(
        "eor.16b v12, v13, v14",
        Inst::EorV16B {
            rd: FpReg::new(12),
            rn: FpReg::new(13),
            rm: FpReg::new(14),
        },
    );
}
#[test]
fn sys_ext_16b() {
    if !native_macho_host("verify_against_system_as", "sys_ext_16b") {
        return;
    }
    verify(
        "ext.16b v0, v0, v0, #8",
        Inst::ExtV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
            rm: FpReg::new(0),
            index: 8,
        },
    );
}
#[test]
fn sys_rev64_4s() {
    if !native_macho_host("verify_against_system_as", "sys_rev64_4s") {
        return;
    }
    verify(
        "rev64.4s v1, v2",
        Inst::Rev64V4S {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_zip1_4s() {
    if !native_macho_host("verify_against_system_as", "sys_zip1_4s") {
        return;
    }
    verify(
        "zip1.4s v0, v0, v1",
        Inst::Zip1V4S {
            rd: FpReg::new(0),
            rn: FpReg::new(0),
            rm: FpReg::new(1),
        },
    );
}
#[test]
fn sys_zip1_2d() {
    if !native_macho_host("verify_against_system_as", "sys_zip1_2d") {
        return;
    }
    verify(
        "zip1.2d v0, v1, v2",
        Inst::Zip1V2D {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            rm: FpReg::new(2),
        },
    );
}
#[test]
fn sys_zip2_4s() {
    if !native_macho_host("verify_against_system_as", "sys_zip2_4s") {
        return;
    }
    verify(
        "zip2.4s v2, v3, v4",
        Inst::Zip2V4S {
            rd: FpReg::new(2),
            rn: FpReg::new(3),
            rm: FpReg::new(4),
        },
    );
}
#[test]
fn sys_zip2_2d() {
    if !native_macho_host("verify_against_system_as", "sys_zip2_2d") {
        return;
    }
    verify(
        "zip2.2d v3, v4, v5",
        Inst::Zip2V2D {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_uzp1_4s() {
    if !native_macho_host("verify_against_system_as", "sys_uzp1_4s") {
        return;
    }
    verify(
        "uzp1.4s v5, v6, v7",
        Inst::Uzp1V4S {
            rd: FpReg::new(5),
            rn: FpReg::new(6),
            rm: FpReg::new(7),
        },
    );
}
#[test]
fn sys_uzp1_2d() {
    if !native_macho_host("verify_against_system_as", "sys_uzp1_2d") {
        return;
    }
    verify(
        "uzp1.2d v6, v7, v8",
        Inst::Uzp1V2D {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
            rm: FpReg::new(8),
        },
    );
}
#[test]
fn sys_uzp2_4s() {
    if !native_macho_host("verify_against_system_as", "sys_uzp2_4s") {
        return;
    }
    verify(
        "uzp2.4s v8, v9, v10",
        Inst::Uzp2V4S {
            rd: FpReg::new(8),
            rn: FpReg::new(9),
            rm: FpReg::new(10),
        },
    );
}
#[test]
fn sys_uzp2_2d() {
    if !native_macho_host("verify_against_system_as", "sys_uzp2_2d") {
        return;
    }
    verify(
        "uzp2.2d v9, v10, v11",
        Inst::Uzp2V2D {
            rd: FpReg::new(9),
            rn: FpReg::new(10),
            rm: FpReg::new(11),
        },
    );
}
#[test]
fn sys_trn1_4s() {
    if !native_macho_host("verify_against_system_as", "sys_trn1_4s") {
        return;
    }
    verify(
        "trn1.4s v11, v12, v13",
        Inst::Trn1V4S {
            rd: FpReg::new(11),
            rn: FpReg::new(12),
            rm: FpReg::new(13),
        },
    );
}
#[test]
fn sys_trn1_2d() {
    if !native_macho_host("verify_against_system_as", "sys_trn1_2d") {
        return;
    }
    verify(
        "trn1.2d v12, v13, v14",
        Inst::Trn1V2D {
            rd: FpReg::new(12),
            rn: FpReg::new(13),
            rm: FpReg::new(14),
        },
    );
}
#[test]
fn sys_trn2_4s() {
    if !native_macho_host("verify_against_system_as", "sys_trn2_4s") {
        return;
    }
    verify(
        "trn2.4s v3, v4, v5",
        Inst::Trn2V4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            rm: FpReg::new(5),
        },
    );
}
#[test]
fn sys_trn2_2d() {
    if !native_macho_host("verify_against_system_as", "sys_trn2_2d") {
        return;
    }
    verify(
        "trn2.2d v15, v16, v17",
        Inst::Trn2V2D {
            rd: FpReg::new(15),
            rn: FpReg::new(16),
            rm: FpReg::new(17),
        },
    );
}
#[test]
fn sys_tbl_16b_single() {
    if !native_macho_host("verify_against_system_as", "sys_tbl_16b_single") {
        return;
    }
    verify(
        "tbl.16b v0, { v1 }, v2",
        Inst::TblV16B {
            rd: FpReg::new(0),
            table: FpReg::new(1),
            table_len: 1,
            index: FpReg::new(2),
        },
    );
}
#[test]
fn sys_tbl_16b_pair() {
    if !native_macho_host("verify_against_system_as", "sys_tbl_16b_pair") {
        return;
    }
    verify(
        "tbl.16b v3, { v4, v5 }, v6",
        Inst::TblV16B {
            rd: FpReg::new(3),
            table: FpReg::new(4),
            table_len: 2,
            index: FpReg::new(6),
        },
    );
}
#[test]
fn sys_tbl_16b_triple() {
    if !native_macho_host("verify_against_system_as", "sys_tbl_16b_triple") {
        return;
    }
    verify(
        "tbl.16b v7, { v8, v9, v10 }, v11",
        Inst::TblV16B {
            rd: FpReg::new(7),
            table: FpReg::new(8),
            table_len: 3,
            index: FpReg::new(11),
        },
    );
}
#[test]
fn sys_tbl_16b_quad() {
    if !native_macho_host("verify_against_system_as", "sys_tbl_16b_quad") {
        return;
    }
    verify(
        "tbl.16b v12, { v13, v14, v15, v16 }, v17",
        Inst::TblV16B {
            rd: FpReg::new(12),
            table: FpReg::new(13),
            table_len: 4,
            index: FpReg::new(17),
        },
    );
}
#[test]
fn sys_tbx_16b_single() {
    if !native_macho_host("verify_against_system_as", "sys_tbx_16b_single") {
        return;
    }
    verify(
        "tbx.16b v18, { v19 }, v20",
        Inst::TbxV16B {
            rd: FpReg::new(18),
            table: FpReg::new(19),
            table_len: 1,
            index: FpReg::new(20),
        },
    );
}
#[test]
fn sys_tbx_16b_pair() {
    if !native_macho_host("verify_against_system_as", "sys_tbx_16b_pair") {
        return;
    }
    verify(
        "tbx.16b v21, { v22, v23 }, v24",
        Inst::TbxV16B {
            rd: FpReg::new(21),
            table: FpReg::new(22),
            table_len: 2,
            index: FpReg::new(24),
        },
    );
}
#[test]
fn sys_mov_16b() {
    if !native_macho_host("verify_against_system_as", "sys_mov_16b") {
        return;
    }
    verify(
        "mov.16b v0, v2",
        Inst::MovV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(2),
        },
    );
}
#[test]
fn sys_mov_8b() {
    if !native_macho_host("verify_against_system_as", "sys_mov_8b") {
        return;
    }
    verify(
        "mov.8b v1, v3",
        Inst::MovV8B {
            rd: FpReg::new(1),
            rn: FpReg::new(3),
        },
    );
}
#[test]
fn sys_mov_4s() {
    if !native_macho_host("verify_against_system_as", "sys_mov_4s") {
        return;
    }
    verify(
        "mov.4s v4, v5",
        Inst::MovV4S {
            rd: FpReg::new(4),
            rn: FpReg::new(5),
        },
    );
}
#[test]
fn sys_mov_2d() {
    if !native_macho_host("verify_against_system_as", "sys_mov_2d") {
        return;
    }
    verify(
        "mov.2d v6, v7",
        Inst::MovV2D {
            rd: FpReg::new(6),
            rn: FpReg::new(7),
        },
    );
}
#[test]
fn sys_fmov_reg_s() {
    if !native_macho_host("verify_against_system_as", "sys_fmov_reg_s") {
        return;
    }
    verify("fmov s1, s2", Inst::FmovRegS { rd: S1, rn: S2 });
}
#[test]
fn sys_fmov_reg_d() {
    if !native_macho_host("verify_against_system_as", "sys_fmov_reg_d") {
        return;
    }
    verify("fmov d1, d2", Inst::FmovRegD { rd: D1, rn: D2 });
}
#[test]
fn sys_fmov_to_s() {
    if !native_macho_host("verify_against_system_as", "sys_fmov_to_s") {
        return;
    }
    verify("fmov s0, w1", Inst::FmovToS { rd: S0, rn: W1 });
}
#[test]
fn sys_fmov_from_s() {
    if !native_macho_host("verify_against_system_as", "sys_fmov_from_s") {
        return;
    }
    verify("fmov w0, s1", Inst::FmovFromS { rd: W0, rn: S1 });
}
#[test]
fn sys_mov_from_lane_s() {
    if !native_macho_host("verify_against_system_as", "sys_mov_from_lane_s") {
        return;
    }
    verify(
        "mov s0, v1[2]",
        Inst::MovFromLaneS {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            index: 2,
        },
    );
}
#[test]
fn sys_mov_from_lane_d() {
    if !native_macho_host("verify_against_system_as", "sys_mov_from_lane_d") {
        return;
    }
    verify(
        "mov d3, v4[1]",
        Inst::MovFromLaneD {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            index: 1,
        },
    );
}
#[test]
fn sys_mov_lane_s() {
    if !native_macho_host("verify_against_system_as", "sys_mov_lane_s") {
        return;
    }
    verify(
        "mov.s v5[0], v6[0]",
        Inst::MovLaneS {
            rd: FpReg::new(5),
            rd_index: 0,
            rn: FpReg::new(6),
            rn_index: 0,
        },
    );
}
#[test]
fn sys_mov_lane_d() {
    if !native_macho_host("verify_against_system_as", "sys_mov_lane_d") {
        return;
    }
    verify(
        "mov.d v7[1], v8[1]",
        Inst::MovLaneD {
            rd: FpReg::new(7),
            rd_index: 1,
            rn: FpReg::new(8),
            rn_index: 1,
        },
    );
}
#[test]
fn sys_mov_lane_h() {
    if !native_macho_host("verify_against_system_as", "sys_mov_lane_h") {
        return;
    }
    verify(
        "mov.h v0[5], v1[0]",
        Inst::MovLaneH {
            rd: FpReg::new(0),
            rd_index: 5,
            rn: FpReg::new(1),
            rn_index: 0,
        },
    );
}
#[test]
fn sys_mov_lane_b() {
    if !native_macho_host("verify_against_system_as", "sys_mov_lane_b") {
        return;
    }
    verify(
        "mov.b v0[7], v1[0]",
        Inst::MovLaneB {
            rd: FpReg::new(0),
            rd_index: 7,
            rn: FpReg::new(1),
            rn_index: 0,
        },
    );
}
#[test]
fn sys_mov_from_lane_gp_s() {
    if !native_macho_host("verify_against_system_as", "sys_mov_from_lane_gp_s") {
        return;
    }
    verify(
        "mov.s w0, v1[2]",
        Inst::MovFromLaneGpS {
            rd: W0,
            rn: FpReg::new(1),
            index: 2,
        },
    );
}
#[test]
fn sys_mov_from_lane_gp_d() {
    if !native_macho_host("verify_against_system_as", "sys_mov_from_lane_gp_d") {
        return;
    }
    verify(
        "mov.d x0, v1[1]",
        Inst::MovFromLaneGpD {
            rd: X0,
            rn: FpReg::new(1),
            index: 1,
        },
    );
}
#[test]
fn sys_umov_h() {
    if !native_macho_host("verify_against_system_as", "sys_umov_h") {
        return;
    }
    verify(
        "umov.h w1, v2[5]",
        Inst::UmovFromLaneH {
            rd: W1,
            rn: FpReg::new(2),
            index: 5,
        },
    );
}
#[test]
fn sys_umov_b() {
    if !native_macho_host("verify_against_system_as", "sys_umov_b") {
        return;
    }
    verify(
        "umov.b w3, v4[7]",
        Inst::UmovFromLaneB {
            rd: W3,
            rn: FpReg::new(4),
            index: 7,
        },
    );
}
#[test]
fn sys_smov_h() {
    if !native_macho_host("verify_against_system_as", "sys_smov_h") {
        return;
    }
    verify(
        "smov.h w1, v2[3]",
        Inst::SmovFromLaneH {
            rd: W1,
            rn: FpReg::new(2),
            index: 3,
        },
    );
}
#[test]
fn sys_smov_b() {
    if !native_macho_host("verify_against_system_as", "sys_smov_b") {
        return;
    }
    verify(
        "smov.b w0, v0[0]",
        Inst::SmovFromLaneB {
            rd: W0,
            rn: FpReg::new(0),
            index: 0,
        },
    );
}
#[test]
fn sys_mov_lane_from_gp_s() {
    if !native_macho_host("verify_against_system_as", "sys_mov_lane_from_gp_s") {
        return;
    }
    verify(
        "mov.s v5[1], w6",
        Inst::MovLaneFromGpS {
            rd: FpReg::new(5),
            rd_index: 1,
            rn: W6,
        },
    );
}
#[test]
fn sys_mov_lane_from_gp_d() {
    if !native_macho_host("verify_against_system_as", "sys_mov_lane_from_gp_d") {
        return;
    }
    verify(
        "mov.d v0[1], x1",
        Inst::MovLaneFromGpD {
            rd: FpReg::new(0),
            rd_index: 1,
            rn: X1,
        },
    );
}
#[test]
fn sys_mov_lane_from_gp_h() {
    if !native_macho_host("verify_against_system_as", "sys_mov_lane_from_gp_h") {
        return;
    }
    verify(
        "mov.h v7[5], w8",
        Inst::MovLaneFromGpH {
            rd: FpReg::new(7),
            rd_index: 5,
            rn: W8,
        },
    );
}
#[test]
fn sys_mov_lane_from_gp_b() {
    if !native_macho_host("verify_against_system_as", "sys_mov_lane_from_gp_b") {
        return;
    }
    verify(
        "mov.b v9[7], w10",
        Inst::MovLaneFromGpB {
            rd: FpReg::new(9),
            rd_index: 7,
            rn: W10,
        },
    );
}
#[test]
fn sys_dup_16b() {
    if !native_macho_host("verify_against_system_as", "sys_dup_16b") {
        return;
    }
    verify(
        "dup.16b v0, v1[15]",
        Inst::DupV16B {
            rd: FpReg::new(0),
            rn: FpReg::new(1),
            index: 15,
        },
    );
}
#[test]
fn sys_dup_8h() {
    if !native_macho_host("verify_against_system_as", "sys_dup_8h") {
        return;
    }
    verify(
        "dup.8h v1, v2[5]",
        Inst::DupV8H {
            rd: FpReg::new(1),
            rn: FpReg::new(2),
            index: 5,
        },
    );
}
#[test]
fn sys_dup_4s() {
    if !native_macho_host("verify_against_system_as", "sys_dup_4s") {
        return;
    }
    verify(
        "dup.4s v3, v4[2]",
        Inst::DupV4S {
            rd: FpReg::new(3),
            rn: FpReg::new(4),
            index: 2,
        },
    );
}
#[test]
fn sys_dup_2d() {
    if !native_macho_host("verify_against_system_as", "sys_dup_2d") {
        return;
    }
    verify(
        "dup.2d v5, v6[1]",
        Inst::DupV2D {
            rd: FpReg::new(5),
            rn: FpReg::new(6),
            index: 1,
        },
    );
}
#[test]
fn sys_fneg_d() {
    if !native_macho_host("verify_against_system_as", "sys_fneg_d") {
        return;
    }
    verify("fneg d3, d4", Inst::FnegD { rd: D3, rn: D4 });
}
#[test]
fn sys_fabs_d() {
    if !native_macho_host("verify_against_system_as", "sys_fabs_d") {
        return;
    }
    verify("fabs d3, d4", Inst::FabsD { rd: D3, rn: D4 });
}
#[test]
fn sys_fsqrt_d() {
    if !native_macho_host("verify_against_system_as", "sys_fsqrt_d") {
        return;
    }
    verify("fsqrt d3, d4", Inst::FsqrtD { rd: D3, rn: D4 });
}
#[test]
fn sys_fcmp_d() {
    if !native_macho_host("verify_against_system_as", "sys_fcmp_d") {
        return;
    }
    verify("fcmp d3, d4", Inst::FcmpD { rn: D3, rm: D4 });
}
#[test]
fn sys_fmov_imm_d() {
    if !native_macho_host("verify_against_system_as", "sys_fmov_imm_d") {
        return;
    }
    verify("fmov d2, #3.50000000", Inst::FmovImmD { rd: D2, imm8: 12 });
}
#[test]
fn sys_fcsel_d() {
    if !native_macho_host("verify_against_system_as", "sys_fcsel_d") {
        return;
    }
    verify(
        "fcsel d0, d0, d1, mi",
        Inst::FcselD {
            rd: D0,
            rn: D0,
            rm: D1,
            cond: Cond::MI,
        },
    );
}
#[test]
fn sys_fmadd_d() {
    if !native_macho_host("verify_against_system_as", "sys_fmadd_d") {
        return;
    }
    verify(
        "fmadd d0, d1, d2, d3",
        Inst::FmaddD {
            rd: D0,
            rn: D1,
            rm: D2,
            ra: D3,
        },
    );
}

// ---- FP / integer conversion ----

#[test]
fn sys_fcvtzs() {
    if !native_macho_host("verify_against_system_as", "sys_fcvtzs") {
        return;
    }
    verify("fcvtzs x5, d6", Inst::FcvtzsD { rd: X5, rn: D6 });
}
#[test]
fn sys_scvtf() {
    if !native_macho_host("verify_against_system_as", "sys_scvtf") {
        return;
    }
    verify("scvtf d5, x6", Inst::ScvtfD { rd: D5, rn: X6 });
}
#[test]
fn sys_fmov_to() {
    if !native_macho_host("verify_against_system_as", "sys_fmov_to") {
        return;
    }
    verify("fmov d5, x6", Inst::FmovToD { rd: D5, rn: X6 });
}
#[test]
fn sys_fmov_from() {
    if !native_macho_host("verify_against_system_as", "sys_fmov_from") {
        return;
    }
    verify("fmov x5, d6", Inst::FmovFromD { rd: X5, rn: D6 });
}
#[test]
fn sys_fmov_imm_s() {
    if !native_macho_host("verify_against_system_as", "sys_fmov_imm_s") {
        return;
    }
    verify("fmov s2, #3.50000000", Inst::FmovImmS { rd: S2, imm8: 12 });
}
#[test]
fn sys_fcsel_s() {
    if !native_macho_host("verify_against_system_as", "sys_fcsel_s") {
        return;
    }
    verify(
        "fcsel s0, s0, s1, mi",
        Inst::FcselS {
            rd: S0,
            rn: S0,
            rm: S1,
            cond: Cond::MI,
        },
    );
}

// ---- System ----

#[test]
fn sys_svc() {
    if !native_macho_host("verify_against_system_as", "sys_svc") {
        return;
    }
    verify("svc #0x80", Inst::Svc { imm16: 0x80 });
}
#[test]
fn sys_nop() {
    if !native_macho_host("verify_against_system_as", "sys_nop") {
        return;
    }
    verify("nop", Inst::Nop);
}
#[test]
fn sys_yield() {
    if !native_macho_host("verify_against_system_as", "sys_yield") {
        return;
    }
    verify("yield", Inst::Yield);
}
#[test]
fn sys_wfe() {
    if !native_macho_host("verify_against_system_as", "sys_wfe") {
        return;
    }
    verify("wfe", Inst::Wfe);
}
#[test]
fn sys_wfi() {
    if !native_macho_host("verify_against_system_as", "sys_wfi") {
        return;
    }
    verify("wfi", Inst::Wfi);
}
#[test]
fn sys_sev() {
    if !native_macho_host("verify_against_system_as", "sys_sev") {
        return;
    }
    verify("sev", Inst::Sev);
}
#[test]
fn sys_sevl() {
    if !native_macho_host("verify_against_system_as", "sys_sevl") {
        return;
    }
    verify("sevl", Inst::Sevl);
}
#[test]
fn sys_isb() {
    if !native_macho_host("verify_against_system_as", "sys_isb") {
        return;
    }
    verify(
        "isb",
        Inst::Isb {
            option: BarrierOpt::Sy,
        },
    );
}
#[test]
fn sys_dmb_ish() {
    if !native_macho_host("verify_against_system_as", "sys_dmb_ish") {
        return;
    }
    verify(
        "dmb ish",
        Inst::Dmb {
            option: BarrierOpt::Ish,
        },
    );
}
#[test]
fn sys_dsb_ishst() {
    if !native_macho_host("verify_against_system_as", "sys_dsb_ishst") {
        return;
    }
    verify(
        "dsb ishst",
        Inst::Dsb {
            option: BarrierOpt::Ishst,
        },
    );
}
#[test]
fn sys_brk() {
    if !native_macho_host("verify_against_system_as", "sys_brk") {
        return;
    }
    verify("brk #42", Inst::Brk { imm16: 42 });
}
