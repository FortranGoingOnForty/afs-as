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
        match stmt {
            Stmt::Instruction(inst) | Stmt::InstructionWithReloc(inst, _) => {
                let word = inst.encode();
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            _ => {}
        }
    }
    bytes
}

/// Compare our assembly output against the system assembler.
fn roundtrip(asm: &str) {
    let ours = our_assemble(asm);
    let theirs = system_assemble(asm);
    assert_eq!(
        ours.len(),
        theirs.len(),
        "byte count mismatch: ours={} theirs={}\n---input---\n{}",
        ours.len(),
        theirs.len(),
        asm
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

#[test]
fn rt_add_reg() {
    roundtrip(".text\nadd x0, x1, x2\n");
}
#[test]
fn rt_sub_reg() {
    roundtrip(".text\nsub x10, x11, x12\n");
}
#[test]
fn rt_add_w() {
    roundtrip(".text\nadd w3, w4, w5\n");
}
#[test]
fn rt_add_shift_reg() {
    roundtrip(".text\nadd x0, x1, x2, lsl #3\n");
}
#[test]
fn rt_sub_shift_reg() {
    roundtrip(".text\nsub w3, w4, w5, asr #7\n");
}
#[test]
fn rt_cmp_shift_reg() {
    roundtrip(".text\ncmp x6, x7, lsr #4\n");
}
#[test]
fn rt_add_ext_reg() {
    roundtrip(".text\nadd x0, x0, w1, sxtw #3\n");
}
#[test]
fn rt_add_ext_reg_sp_base() {
    roundtrip(".text\nadd x11, sp, w12, sxtw #2\n");
}
#[test]
fn rt_sub_ext_reg() {
    roundtrip(".text\nsub x2, x3, w4, uxtw #2\n");
}
#[test]
fn rt_cmp_ext_reg() {
    roundtrip(".text\ncmp x0, w1, sxtw\n");
}
#[test]
fn rt_add_imm() {
    roundtrip(".text\nadd x0, x1, #100\n");
}
#[test]
fn rt_sub_imm() {
    roundtrip(".text\nsub x3, x4, #200\n");
}
#[test]
fn rt_mul() {
    roundtrip(".text\nmul x0, x1, x2\n");
}
#[test]
fn rt_madd() {
    roundtrip(".text\nmadd w0, w0, w0, w8\n");
}
#[test]
fn rt_msub() {
    roundtrip(".text\nmsub w9, w8, w1, w0\n");
}
#[test]
fn rt_umull() {
    roundtrip(".text\numull x9, w8, w9\n");
}
#[test]
fn rt_sdiv() {
    roundtrip(".text\nsdiv x3, x4, x5\n");
}
#[test]
fn rt_udiv() {
    roundtrip(".text\nudiv x6, x7, x8\n");
}
#[test]
fn rt_and() {
    roundtrip(".text\nand x0, x1, x2\n");
}
#[test]
fn rt_and_imm() {
    roundtrip(".text\nand w8, w8, #0x7\n");
}
#[test]
fn rt_orr() {
    roundtrip(".text\norr x0, x1, x2\n");
}
#[test]
fn rt_neg() {
    roundtrip(".text\nneg x0, x1\n");
}
#[test]
fn rt_mvn() {
    roundtrip(".text\nmvn x0, x1\n");
}
#[test]
fn rt_eor() {
    roundtrip(".text\neor x0, x1, x2\n");
}
#[test]
fn rt_cmp_reg() {
    roundtrip(".text\ncmp x5, x6\n");
}
#[test]
fn rt_cmp_imm() {
    roundtrip(".text\ncmp x5, #42\n");
}
#[test]
fn rt_tst() {
    roundtrip(".text\ntst x0, x1\n");
}
#[test]
fn rt_tst_imm() {
    roundtrip(".text\ntst w8, #0x7\n");
}
#[test]
fn rt_csel() {
    roundtrip(".text\ncsel w0, w0, w1, gt\n");
}
#[test]
fn rt_ccmp() {
    roundtrip(".text\nccmp w0, #3, #4, ne\n");
}
#[test]
fn rt_ccmn() {
    roundtrip(".text\nccmn x3, #9, #1, ge\n");
}
#[test]
fn rt_csinc() {
    roundtrip(".text\ncsinc x2, x3, x4, ne\n");
}
#[test]
fn rt_csinv() {
    roundtrip(".text\ncsinv x2, x3, x4, ne\n");
}
#[test]
fn rt_csneg() {
    roundtrip(".text\ncsneg x5, x6, x7, gt\n");
}
#[test]
fn rt_cset() {
    roundtrip(".text\ncset x0, eq\n");
}
#[test]
fn rt_csetm() {
    roundtrip(".text\ncsetm w8, eq\n");
}
#[test]
fn rt_cinc() {
    roundtrip(".text\ncinc w2, w3, ne\n");
}
#[test]
fn rt_cinv() {
    roundtrip(".text\ncinv w9, w10, mi\n");
}
#[test]
fn rt_cneg() {
    roundtrip(".text\ncneg x11, x12, lt\n");
}
#[test]
fn rt_ubfiz() {
    roundtrip(".text\nubfiz w8, w0, #5, #3\n");
}
#[test]
fn rt_bfi() {
    roundtrip(".text\nbfi w0, w8, #5, #27\n");
}
#[test]
fn rt_bfxil() {
    roundtrip(".text\nbfxil w8, w0, #3, #5\n");
}
#[test]
fn rt_movz() {
    roundtrip(".text\nmovz x0, #0x1234\n");
}
#[test]
fn rt_movz_lsl16() {
    roundtrip(".text\nmovz x0, #0x5678, lsl #16\n");
}
#[test]
fn rt_movk() {
    roundtrip(".text\nmovk x0, #0xABCD, lsl #32\n");
}
#[test]
fn rt_mov_neg_large() {
    roundtrip(".text\nmov x0, #-65537\n");
}
#[test]
fn rt_lsl() {
    roundtrip(".text\nlsl x0, x1, #7\n");
}
#[test]
fn rt_lsr() {
    roundtrip(".text\nlsr x0, x1, #15\n");
}
#[test]
fn rt_asr() {
    roundtrip(".text\nasr x0, x1, #31\n");
}
#[test]
fn rt_b() {
    roundtrip(".text\nb #20\n");
}
#[test]
fn rt_bl() {
    roundtrip(".text\nbl #40\n");
}
#[test]
fn rt_b_eq() {
    roundtrip(".text\nb.eq #24\n");
}
#[test]
fn rt_b_ne() {
    roundtrip(".text\nb.ne #32\n");
}
#[test]
fn rt_b_lt() {
    roundtrip(".text\nb.lt #16\n");
}
#[test]
fn rt_b_ge() {
    roundtrip(".text\nb.ge #8\n");
}
#[test]
fn rt_b_gt() {
    roundtrip(".text\nb.gt #12\n");
}
#[test]
fn rt_b_le() {
    roundtrip(".text\nb.le #20\n");
}
#[test]
fn rt_cbz() {
    roundtrip(".text\ncbz x5, #16\n");
}
#[test]
fn rt_cbnz() {
    roundtrip(".text\ncbnz x10, #24\n");
}
#[test]
fn rt_tbz() {
    roundtrip(".text\ntbz x0, #5, #8\n");
}
#[test]
fn rt_tbnz() {
    roundtrip(".text\ntbnz x1, #33, #12\n");
}
#[test]
fn rt_ret() {
    roundtrip(".text\nret\n");
}
#[test]
fn rt_br() {
    roundtrip(".text\nbr x8\n");
}
#[test]
fn rt_blr() {
    roundtrip(".text\nblr x9\n");
}
#[test]
fn rt_adr() {
    roundtrip(".text\nadr x0, #8\n");
}
#[test]
fn rt_adrp_tlvp() {
    roundtrip(".text\nadrp x0, _tls_counter@TLVPPAGE\n");
}
#[test]
fn rt_ldr64() {
    roundtrip(".text\nldr x0, [x1, #24]\n");
}
#[test]
fn rt_ldr64_tlvp_pageoff() {
    roundtrip(".text\nldr x0, [x0, _tls_counter@TLVPPAGEOFF]\n");
}
#[test]
fn rt_str64() {
    roundtrip(".text\nstr x2, [x3, #32]\n");
}
#[test]
fn rt_ldr32() {
    roundtrip(".text\nldr w4, [x5, #12]\n");
}
#[test]
fn rt_str32() {
    roundtrip(".text\nstr w6, [x7, #16]\n");
}
#[test]
fn rt_ldur64() {
    roundtrip(".text\nldur x9, [x29, #-8]\n");
}
#[test]
fn rt_stur32() {
    roundtrip(".text\nstur w6, [x7, #-4]\n");
}
#[test]
fn rt_ldr_negative_alias() {
    roundtrip(".text\nldr x0, [x1, #-8]\n");
}
#[test]
fn rt_ldr_post32() {
    roundtrip(".text\nldr w0, [x1], #4\n");
}
#[test]
fn rt_str_pre32() {
    roundtrip(".text\nstr w2, [x3, #-4]!\n");
}
#[test]
fn rt_ldr_lit64() {
    roundtrip(".text\nldr x0, #8\n");
}
#[test]
fn rt_ldr_lit32() {
    roundtrip(".text\nldr w1, #12\n");
}
#[test]
fn rt_ldr64_reg() {
    roundtrip(".text\nldr x0, [x1, x2]\n");
}
#[test]
fn rt_ldr64_reg_uxtw() {
    roundtrip(".text\nldr x6, [x7, w8, uxtw #3]\n");
}
#[test]
fn rt_str64_reg() {
    roundtrip(".text\nstr x12, [x13, x14]\n");
}
#[test]
fn rt_ldrb() {
    roundtrip(".text\nldrb w0, [x1, #3]\n");
}
#[test]
fn rt_ldrsb() {
    roundtrip(".text\nldrsb w0, [x1, #3]\n");
}
#[test]
fn rt_ldrb_post() {
    roundtrip(".text\nldrb w9, [x1], #1\n");
}
#[test]
fn rt_ldrsb_post() {
    roundtrip(".text\nldrsb x9, [x1], #1\n");
}
#[test]
fn rt_ldrh() {
    roundtrip(".text\nldrh w2, [x3, #6]\n");
}
#[test]
fn rt_ldrsh() {
    roundtrip(".text\nldrsh x0, [x1, #4]\n");
}
#[test]
fn rt_strb() {
    roundtrip(".text\nstrb w8, [x9]\n");
}
#[test]
fn rt_strb_post() {
    roundtrip(".text\nstrb w9, [x8], #1\n");
}
#[test]
fn rt_strh_pre() {
    roundtrip(".text\nstrh w5, [x6, #2]!\n");
}
#[test]
fn rt_ldrsw() {
    roundtrip(".text\nldrsw x0, [x1, #8]\n");
}
#[test]
fn rt_ldrh_reg() {
    roundtrip(".text\nldrh w3, [x4, w5, uxtw #1]\n");
}
#[test]
fn rt_ldrsh_reg() {
    roundtrip(".text\nldrsh w3, [x4, w5, uxtw #1]\n");
}
#[test]
fn rt_ldrb_reg() {
    roundtrip(".text\nldrb w0, [x1, x2]\n");
}
#[test]
fn rt_ldrsw_reg() {
    roundtrip(".text\nldrsw x6, [x7, w8, sxtw #2]\n");
}
#[test]
fn rt_ldaprb() {
    roundtrip(".text\nldaprb w0, [x1]\n");
}
#[test]
fn rt_ldaprh() {
    roundtrip(".text\nldaprh w2, [x3]\n");
}
#[test]
fn rt_ldapr() {
    roundtrip(".text\nldapr w8, [x9]\n");
}
#[test]
fn rt_stlrb() {
    roundtrip(".text\nstlrb w4, [x5]\n");
}
#[test]
fn rt_stlrh() {
    roundtrip(".text\nstlrh w6, [x7]\n");
}
#[test]
fn rt_stlr() {
    roundtrip(".text\nstlr x10, [x11]\n");
}
#[test]
fn rt_ldaddalb() {
    roundtrip(".text\nldaddalb w0, w1, [x2]\n");
}
#[test]
fn rt_ldaddalh() {
    roundtrip(".text\nldaddalh w3, w4, [x5]\n");
}
#[test]
fn rt_ldumaxalb() {
    roundtrip(".text\nldumaxalb w0, w1, [x2]\n");
}
#[test]
fn rt_ldumaxalh() {
    roundtrip(".text\nldumaxalh w3, w4, [x5]\n");
}
#[test]
fn rt_ldsmaxalb() {
    roundtrip(".text\nldsmaxalb w18, w19, [x20]\n");
}
#[test]
fn rt_ldsmaxalh() {
    roundtrip(".text\nldsmaxalh w24, w25, [x26]\n");
}
#[test]
fn rt_ldaddal() {
    roundtrip(".text\nldaddal w0, w8, [x8]\n");
}
#[test]
fn rt_ldumaxal() {
    roundtrip(".text\nldumaxal x9, x10, [x11]\n");
}
#[test]
fn rt_ldsmaxal() {
    roundtrip(".text\nldsmaxal w0, w1, [x2]\n");
}
#[test]
fn rt_ldsminal() {
    roundtrip(".text\nldsminal x9, x10, [x11]\n");
}
#[test]
fn rt_lduminalb() {
    roundtrip(".text\nlduminalb w12, w13, [x14]\n");
}
#[test]
fn rt_lduminalh() {
    roundtrip(".text\nlduminalh w15, w16, [x17]\n");
}
#[test]
fn rt_ldsminalb() {
    roundtrip(".text\nldsminalb w21, w22, [x23]\n");
}
#[test]
fn rt_ldsminalh() {
    roundtrip(".text\nldsminalh w27, w28, [x29]\n");
}
#[test]
fn rt_lduminal() {
    roundtrip(".text\nlduminal w18, w19, [x20]\n");
}
#[test]
fn rt_ldclral() {
    roundtrip(".text\nldclral w12, w13, [x14]\n");
}
#[test]
fn rt_ldclralb() {
    roundtrip(".text\nldclralb w6, w7, [x8]\n");
}
#[test]
fn rt_ldclralh() {
    roundtrip(".text\nldclralh w15, w16, [x17]\n");
}
#[test]
fn rt_ldeoral() {
    roundtrip(".text\nldeoral x9, x10, [x11]\n");
}
#[test]
fn rt_ldeoralb() {
    roundtrip(".text\nldeoralb w3, w4, [x5]\n");
}
#[test]
fn rt_ldeoralh() {
    roundtrip(".text\nldeoralh w12, w13, [x14]\n");
}
#[test]
fn rt_ldsetal() {
    roundtrip(".text\nldsetal w0, w1, [x2]\n");
}
#[test]
fn rt_ldsetalb() {
    roundtrip(".text\nldsetalb w0, w1, [x2]\n");
}
#[test]
fn rt_ldsetalh() {
    roundtrip(".text\nldsetalh w9, w10, [x11]\n");
}
#[test]
fn rt_swpal() {
    roundtrip(".text\nswpal w0, w0, [x8]\n");
}
#[test]
fn rt_swpalb() {
    roundtrip(".text\nswpalb w8, w9, [x10]\n");
}
#[test]
fn rt_swpalh() {
    roundtrip(".text\nswpalh w11, w12, [x13]\n");
}
#[test]
fn rt_casalb() {
    roundtrip(".text\ncasalb w6, w7, [x8]\n");
}
#[test]
fn rt_casalh() {
    roundtrip(".text\ncasalh w9, w10, [x11]\n");
}
#[test]
fn rt_subs_uxtb() {
    roundtrip(".text\nsubs w10, w8, w9, uxtb\n");
}
#[test]
fn rt_subs_uxth() {
    roundtrip(".text\nsubs w11, w12, w13, uxth\n");
}
#[test]
fn rt_subs_sxtb() {
    roundtrip(".text\nsubs x14, x15, w16, sxtb\n");
}
#[test]
fn rt_subs_sxth() {
    roundtrip(".text\nsubs x17, x18, w19, sxth #1\n");
}
#[test]
fn rt_swpal_x() {
    roundtrip(".text\nswpal x1, x2, [x3]\n");
}
#[test]
fn rt_casal() {
    roundtrip(".text\ncasal w4, w5, [x6]\n");
}
#[test]
fn rt_casal_x() {
    roundtrip(".text\ncasal x7, x8, [x9]\n");
}
#[test]
fn rt_ldrsw_lit() {
    roundtrip(".text\nldrsw x1, #8\n");
}
#[test]
fn rt_ldr_d() {
    roundtrip(".text\nldr d0, [x1]\n");
}
#[test]
fn rt_ldr_q() {
    roundtrip(".text\nldr q0, [sp, #16]\n");
}
#[test]
fn rt_ldr_h() {
    roundtrip(".text\nldr h2, [sp, #14]\n");
}
#[test]
fn rt_ldr_b() {
    roundtrip(".text\nldr b2, [sp, #15]\n");
}
#[test]
fn rt_str_q() {
    roundtrip(".text\nstr q1, [x0]\n");
}
#[test]
fn rt_str_h() {
    roundtrip(".text\nstr h2, [sp, #14]\n");
}
#[test]
fn rt_str_b() {
    roundtrip(".text\nstr b2, [sp, #15]\n");
}
#[test]
fn rt_ldr_q_lit() {
    roundtrip(".text\nldr q0, #16\n");
}
#[test]
fn rt_ldr_q_reg() {
    roundtrip(".text\nldr q0, [x1, x2]\n");
}
#[test]
fn rt_str_q_reg_uxtw() {
    roundtrip(".text\nstr q1, [x3, w4, uxtw #4]\n");
}
#[test]
fn rt_ldr_q_post() {
    roundtrip(".text\nldr q0, [sp], #16\n");
}
#[test]
fn rt_str_q_pre() {
    roundtrip(".text\nstr q1, [sp, #-16]!\n");
}
#[test]
fn rt_str_d_off() {
    roundtrip(".text\nstr d2, [x3, #16]\n");
}
#[test]
fn rt_ldr_s_reg() {
    roundtrip(".text\nldr s4, [x5, x6]\n");
}
#[test]
fn rt_str_s_reg_uxtw() {
    roundtrip(".text\nstr s7, [x8, w9, uxtw #2]\n");
}
#[test]
fn rt_ldr_d_lit() {
    roundtrip(".text\nldr d10, #8\n");
}
#[test]
fn rt_ldr_d_post() {
    roundtrip(".text\nldr d0, [sp], #8\n");
}
#[test]
fn rt_str_s_pre() {
    roundtrip(".text\nstr s3, [sp, #-8]!\n");
}
#[test]
fn rt_stp_pre() {
    roundtrip(".text\nstp x29, x30, [sp, #-16]!\n");
}
#[test]
fn rt_stp_post() {
    roundtrip(".text\nstp x29, x30, [sp], #16\n");
}
#[test]
fn rt_ldp_pre() {
    roundtrip(".text\nldp x29, x30, [sp, #-16]!\n");
}
#[test]
fn rt_ldp_post() {
    roundtrip(".text\nldp x29, x30, [sp], #16\n");
}
#[test]
fn rt_stp_off() {
    roundtrip(".text\nstp x19, x20, [sp, #16]\n");
}
#[test]
fn rt_ldp_off() {
    roundtrip(".text\nldp x21, x22, [sp, #48]\n");
}
#[test]
fn rt_ldp_off32() {
    roundtrip(".text\nldp w9, w8, [x8]\n");
}
#[test]
fn rt_stp_off32() {
    roundtrip(".text\nstp w1, w2, [sp, #16]\n");
}
#[test]
fn rt_ldp_post32() {
    roundtrip(".text\nldp w9, w8, [sp], #8\n");
}
#[test]
fn rt_ldp_pre32() {
    roundtrip(".text\nldp w9, w8, [sp, #-8]!\n");
}
#[test]
fn rt_ldp_d_pre() {
    roundtrip(".text\nldp d8, d9, [sp, #-16]!\n");
}
#[test]
fn rt_stp_d_post() {
    roundtrip(".text\nstp d10, d11, [sp], #16\n");
}
#[test]
fn rt_ldp_d_off() {
    roundtrip(".text\nldp d12, d13, [sp, #32]\n");
}
#[test]
fn rt_stp_s_post() {
    roundtrip(".text\nstp s0, s1, [sp], #8\n");
}
#[test]
fn rt_ldp_s_pre() {
    roundtrip(".text\nldp s2, s3, [sp, #-8]!\n");
}
#[test]
fn rt_stp_q_pre() {
    roundtrip(".text\nstp q0, q1, [sp, #-32]!\n");
}
#[test]
fn rt_ldp_q_post() {
    roundtrip(".text\nldp q2, q3, [sp], #32\n");
}
#[test]
fn rt_ldp_q_off() {
    roundtrip(".text\nldp q4, q5, [sp, #64]\n");
}
#[test]
fn rt_ldp_s_off() {
    roundtrip(".text\nldp s4, s5, [sp, #16]\n");
}
#[test]
fn rt_fadd_d() {
    roundtrip(".text\nfadd d0, d1, d2\n");
}
#[test]
fn rt_fsub_d() {
    roundtrip(".text\nfsub d3, d4, d5\n");
}
#[test]
fn rt_fmul_d() {
    roundtrip(".text\nfmul d6, d7, d8\n");
}
#[test]
fn rt_fdiv_d() {
    roundtrip(".text\nfdiv d9, d10, d11\n");
}
#[test]
fn rt_fadd_s() {
    roundtrip(".text\nfadd s0, s1, s2\n");
}
#[test]
fn rt_fadd_4s() {
    roundtrip(".text\nfadd.4s v0, v1, v2\n");
}
#[test]
fn rt_add_4s() {
    roundtrip(".text\nadd.4s v0, v1, v2\n");
}
#[test]
fn rt_fmax_4s() {
    roundtrip(".text\nfmax.4s v0, v0, v1\n");
}
#[test]
fn rt_fmin_4s() {
    roundtrip(".text\nfmin.4s v2, v3, v4\n");
}
#[test]
fn rt_smax_4s() {
    roundtrip(".text\nsmax.4s v5, v6, v7\n");
}
#[test]
fn rt_smin_4s() {
    roundtrip(".text\nsmin.4s v8, v9, v10\n");
}
#[test]
fn rt_umax_4s() {
    roundtrip(".text\numax.4s v0, v0, v1\n");
}
#[test]
fn rt_umin_4s() {
    roundtrip(".text\numin.4s v2, v3, v4\n");
}
#[test]
fn rt_addv_4s() {
    roundtrip(".text\naddv.4s s0, v0\n");
}
#[test]
fn rt_faddp_4s() {
    roundtrip(".text\nfaddp.4s v0, v1, v2\n");
}
#[test]
fn rt_faddp_2s() {
    roundtrip(".text\nfaddp.2s s3, v4\n");
}
#[test]
fn rt_fmaxv_4s() {
    roundtrip(".text\nfmaxv.4s s1, v2\n");
}
#[test]
fn rt_fminv_4s() {
    roundtrip(".text\nfminv.4s s3, v4\n");
}
#[test]
fn rt_umaxv_4s() {
    roundtrip(".text\numaxv.4s s1, v2\n");
}
#[test]
fn rt_smaxv_4s() {
    roundtrip(".text\nsmaxv.4s s3, v4\n");
}
#[test]
fn rt_uminv_4s() {
    roundtrip(".text\numinv.4s s1, v2\n");
}
#[test]
fn rt_sminv_4s() {
    roundtrip(".text\nsminv.4s s3, v4\n");
}
#[test]
fn rt_fsub_4s() {
    roundtrip(".text\nfsub.4s v3, v4, v5\n");
}
#[test]
fn rt_sub_4s() {
    roundtrip(".text\nsub.4s v3, v4, v5\n");
}
#[test]
fn rt_fmul_4s() {
    roundtrip(".text\nfmul.4s v6, v7, v8\n");
}
#[test]
fn rt_fdiv_4s() {
    roundtrip(".text\nfdiv.4s v9, v10, v11\n");
}
#[test]
fn rt_and_16b() {
    roundtrip(".text\nand.16b v6, v7, v8\n");
}
#[test]
fn rt_bic_16b() {
    roundtrip(".text\nbic.16b v5, v6, v7\n");
}
#[test]
fn rt_bif_16b() {
    roundtrip(".text\nbif.16b v0, v1, v2\n");
}
#[test]
fn rt_bit_16b() {
    roundtrip(".text\nbit.16b v3, v4, v5\n");
}
#[test]
fn rt_bsl_16b() {
    roundtrip(".text\nbsl.16b v6, v7, v8\n");
}
#[test]
fn rt_cmeq_4s() {
    roundtrip(".text\ncmeq.4s v0, v0, v1\n");
}
#[test]
fn rt_cmhs_4s() {
    roundtrip(".text\ncmhs.4s v0, v0, v1\n");
}
#[test]
fn rt_cmhi_4s() {
    roundtrip(".text\ncmhi.4s v2, v3, v4\n");
}
#[test]
fn rt_cmge_4s() {
    roundtrip(".text\ncmge.4s v5, v6, v7\n");
}
#[test]
fn rt_cmgt_4s() {
    roundtrip(".text\ncmgt.4s v2, v3, v4\n");
}
#[test]
fn rt_orr_16b() {
    roundtrip(".text\norr.16b v9, v10, v11\n");
}
#[test]
fn rt_eor_16b() {
    roundtrip(".text\neor.16b v12, v13, v14\n");
}
#[test]
fn rt_ext_16b() {
    roundtrip(".text\next.16b v0, v0, v0, #8\n");
}
#[test]
fn rt_rev64_4s() {
    roundtrip(".text\nrev64.4s v1, v2\n");
}
#[test]
fn rt_zip1_4s() {
    roundtrip(".text\nzip1.4s v0, v0, v1\n");
}
#[test]
fn rt_zip2_4s() {
    roundtrip(".text\nzip2.4s v2, v3, v4\n");
}
#[test]
fn rt_uzp1_4s() {
    roundtrip(".text\nuzp1.4s v5, v6, v7\n");
}
#[test]
fn rt_uzp2_4s() {
    roundtrip(".text\nuzp2.4s v8, v9, v10\n");
}
#[test]
fn rt_trn1_4s() {
    roundtrip(".text\ntrn1.4s v11, v12, v13\n");
}
#[test]
fn rt_trn2_4s() {
    roundtrip(".text\ntrn2.4s v3, v4, v5\n");
}
#[test]
fn rt_tbl_16b_single() {
    roundtrip(".text\ntbl.16b v0, { v1 }, v2\n");
}
#[test]
fn rt_tbl_16b_pair() {
    roundtrip(".text\ntbl.16b v3, { v4, v5 }, v6\n");
}
#[test]
fn rt_tbl_16b_triple() {
    roundtrip(".text\ntbl.16b v7, { v8, v9, v10 }, v11\n");
}
#[test]
fn rt_tbl_16b_quad() {
    roundtrip(".text\ntbl.16b v12, { v13, v14, v15, v16 }, v17\n");
}
#[test]
fn rt_tbx_16b_single() {
    roundtrip(".text\ntbx.16b v18, { v19 }, v20\n");
}
#[test]
fn rt_tbx_16b_pair() {
    roundtrip(".text\ntbx.16b v21, { v22, v23 }, v24\n");
}
#[test]
fn rt_mov_16b() {
    roundtrip(".text\nmov.16b v0, v2\n");
}
#[test]
fn rt_mov_8b() {
    roundtrip(".text\nmov.8b v1, v3\n");
}
#[test]
fn rt_mov_4s() {
    roundtrip(".text\nmov.4s v4, v5\n");
}
#[test]
fn rt_mov_2d() {
    roundtrip(".text\nmov.2d v6, v7\n");
}
#[test]
fn rt_fmov_reg_s() {
    roundtrip(".text\nfmov s1, s2\n");
}
#[test]
fn rt_fmov_reg_d() {
    roundtrip(".text\nfmov d1, d2\n");
}
#[test]
fn rt_fmov_to_s() {
    roundtrip(".text\nfmov s0, w1\n");
}
#[test]
fn rt_fmov_from_s() {
    roundtrip(".text\nfmov w0, s1\n");
}
#[test]
fn rt_mov_from_lane_s() {
    roundtrip(".text\nmov s0, v1[2]\n");
}
#[test]
fn rt_mov_from_lane_d() {
    roundtrip(".text\nmov d3, v4[1]\n");
}
#[test]
fn rt_mov_lane_s() {
    roundtrip(".text\nmov.s v5[0], v6[0]\n");
}
#[test]
fn rt_mov_lane_d() {
    roundtrip(".text\nmov.d v7[1], v8[1]\n");
}
#[test]
fn rt_mov_lane_h() {
    roundtrip(".text\nmov.h v0[5], v1[0]\n");
}
#[test]
fn rt_mov_lane_b() {
    roundtrip(".text\nmov.b v0[7], v1[0]\n");
}
#[test]
fn rt_mov_from_lane_gp_s() {
    roundtrip(".text\nmov.s w0, v1[2]\n");
}
#[test]
fn rt_mov_from_lane_gp_d() {
    roundtrip(".text\nmov.d x0, v1[1]\n");
}
#[test]
fn rt_umov_h() {
    roundtrip(".text\numov.h w1, v2[5]\n");
}
#[test]
fn rt_umov_b() {
    roundtrip(".text\numov.b w3, v4[7]\n");
}
#[test]
fn rt_mov_lane_from_gp_s() {
    roundtrip(".text\nmov.s v5[1], w6\n");
}
#[test]
fn rt_mov_lane_from_gp_d() {
    roundtrip(".text\nmov.d v0[1], x1\n");
}
#[test]
fn rt_mov_lane_from_gp_h() {
    roundtrip(".text\nmov.h v7[5], w8\n");
}
#[test]
fn rt_mov_lane_from_gp_b() {
    roundtrip(".text\nmov.b v9[7], w10\n");
}
#[test]
fn rt_dup_16b() {
    roundtrip(".text\ndup.16b v0, v1[15]\n");
}
#[test]
fn rt_dup_8h() {
    roundtrip(".text\ndup.8h v1, v2[5]\n");
}
#[test]
fn rt_dup_4s() {
    roundtrip(".text\ndup.4s v3, v4[2]\n");
}
#[test]
fn rt_dup_2d() {
    roundtrip(".text\ndup.2d v5, v6[1]\n");
}
#[test]
fn rt_fneg_d() {
    roundtrip(".text\nfneg d3, d4\n");
}
#[test]
fn rt_fabs_d() {
    roundtrip(".text\nfabs d3, d4\n");
}
#[test]
fn rt_fsqrt_d() {
    roundtrip(".text\nfsqrt d3, d4\n");
}
#[test]
fn rt_fcmp_d() {
    roundtrip(".text\nfcmp d3, d4\n");
}
#[test]
fn rt_fmov_imm_d() {
    roundtrip(".text\nfmov d2, #3.50000000\n");
}
#[test]
fn rt_fcsel_d() {
    roundtrip(".text\nfcsel d0, d0, d1, mi\n");
}
#[test]
fn rt_fmadd_d() {
    roundtrip(".text\nfmadd d0, d1, d2, d3\n");
}
#[test]
fn rt_fcvtzs() {
    roundtrip(".text\nfcvtzs x0, d1\n");
}
#[test]
fn rt_scvtf() {
    roundtrip(".text\nscvtf d0, x1\n");
}
#[test]
fn rt_fmov_to() {
    roundtrip(".text\nfmov d5, x6\n");
}
#[test]
fn rt_fmov_from() {
    roundtrip(".text\nfmov x5, d6\n");
}
#[test]
fn rt_svc() {
    roundtrip(".text\nsvc #0x80\n");
}
#[test]
fn rt_nop() {
    roundtrip(".text\nnop\n");
}
#[test]
fn rt_yield() {
    roundtrip(".text\nyield\n");
}
#[test]
fn rt_wfe() {
    roundtrip(".text\nwfe\n");
}
#[test]
fn rt_wfi() {
    roundtrip(".text\nwfi\n");
}
#[test]
fn rt_sev() {
    roundtrip(".text\nsev\n");
}
#[test]
fn rt_sevl() {
    roundtrip(".text\nsevl\n");
}
#[test]
fn rt_isb() {
    roundtrip(".text\nisb\n");
}
#[test]
fn rt_dmb_ish() {
    roundtrip(".text\ndmb ish\n");
}
#[test]
fn rt_dsb_ishst() {
    roundtrip(".text\ndsb ishst\n");
}
#[test]
fn rt_brk() {
    roundtrip(".text\nbrk #42\n");
}

// ---- Test gap coverage ----

// W-register shifts
#[test]
fn rt_lsl_w() {
    roundtrip(".text\nlsl w0, w1, #3\n");
}
#[test]
fn rt_lsr_w() {
    roundtrip(".text\nlsr w5, w6, #8\n");
}
#[test]
fn rt_asr_w() {
    roundtrip(".text\nasr w5, w6, #15\n");
}

// Negative branches
#[test]
fn rt_b_neg() {
    roundtrip(".text\nb #-8\n");
}
#[test]
fn rt_bl_neg() {
    roundtrip(".text\nbl #-16\n");
}

// W-register CBZ/CBNZ
#[test]
fn rt_cbz_w() {
    roundtrip(".text\ncbz w0, #8\n");
}
#[test]
fn rt_cbnz_w() {
    roundtrip(".text\ncbnz w5, #12\n");
}

// Single-precision FP
#[test]
fn rt_fneg_s() {
    roundtrip(".text\nfneg s0, s1\n");
}
#[test]
fn rt_fabs_s() {
    roundtrip(".text\nfabs s0, s1\n");
}
#[test]
fn rt_fsqrt_s() {
    roundtrip(".text\nfsqrt s0, s1\n");
}
#[test]
fn rt_fcmp_s() {
    roundtrip(".text\nfcmp s0, s1\n");
}
#[test]
fn rt_fmov_imm_s() {
    roundtrip(".text\nfmov s2, #3.50000000\n");
}
#[test]
fn rt_fcsel_s() {
    roundtrip(".text\nfcsel s0, s0, s1, mi\n");
}
#[test]
fn rt_fmadd_s() {
    roundtrip(".text\nfmadd s0, s1, s2, s3\n");
}

// LDP/STP missing variants
#[test]
fn rt_stp_post_32() {
    roundtrip(".text\nstp x19, x20, [sp], #32\n");
}
#[test]
fn rt_ldp_pre_m32() {
    roundtrip(".text\nldp x19, x20, [sp, #-32]!\n");
}

// All condition codes through round-trip
#[test]
fn rt_b_ne_neg() {
    roundtrip(".text\nb.ne #-4\n");
}
#[test]
fn rt_b_cs() {
    roundtrip(".text\nb.cs #8\n");
}
#[test]
fn rt_b_mi() {
    roundtrip(".text\nb.mi #12\n");
}
#[test]
fn rt_b_hi() {
    roundtrip(".text\nb.hi #16\n");
}
#[test]
fn rt_b_gt_20() {
    roundtrip(".text\nb.gt #20\n");
}

// ---- Multi-instruction round-trips ----

#[test]
fn rt_function_prologue() {
    roundtrip(
        ".text\n\
stp x29, x30, [sp, #-16]!\n\
mov x29, sp\n\
",
    );
}

#[test]
fn rt_function_epilogue() {
    roundtrip(
        ".text\n\
ldp x29, x30, [sp], #16\n\
ret\n\
",
    );
}

#[test]
fn rt_arithmetic_sequence() {
    roundtrip(
        ".text\n\
add x0, x1, x2\n\
sub x3, x4, x5\n\
mul x6, x7, x8\n\
sdiv x9, x10, x11\n\
",
    );
}

#[test]
fn rt_branch_sequence() {
    roundtrip(
        ".text\n\
cmp x0, #0\n\
b.eq #12\n\
add x0, x0, #1\n\
b #8\n\
sub x0, x0, #1\n\
ret\n\
",
    );
}

#[test]
fn rt_fp_sequence() {
    roundtrip(
        ".text\n\
fadd d0, d1, d2\n\
fmul d3, d0, d4\n\
fsub d5, d3, d6\n\
fcmp d5, d7\n\
",
    );
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
        .args([
            "-S",
            "-O2",
            "-o",
            s_path.to_str().unwrap(),
            c_path.to_str().unwrap(),
        ])
        .status();

    if status.is_err() || !status.unwrap().success() {
        // clang not available — skip gracefully.
        return;
    }

    let asm = std::fs::read_to_string(&s_path).unwrap();
    // Just verify we can parse it without errors.
    let result = afs_as::parse::parse(&asm);
    assert!(
        result.is_ok(),
        "failed to parse clang output: {:?}",
        result.err()
    );
    let stmts = result.unwrap();
    let inst_count = stmts
        .iter()
        .filter(|s| matches!(s, Stmt::Instruction(_)))
        .count();
    assert!(
        inst_count > 0,
        "expected at least one instruction from clang output"
    );
}
