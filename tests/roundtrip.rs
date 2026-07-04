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
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let s_path = dir.join(format!("afs_rt_{}_{}.s", pid, id));
    let o_path = dir.join(format!("afs_rt_{}_{}.o", pid, id));

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
fn rt_add_reg() {
    if !native_macho_host("roundtrip", "rt_add_reg") {
        return;
    }
    roundtrip(".text\nadd x0, x1, x2\n");
}
#[test]
fn rt_sub_reg() {
    if !native_macho_host("roundtrip", "rt_sub_reg") {
        return;
    }
    roundtrip(".text\nsub x10, x11, x12\n");
}
#[test]
fn rt_add_w() {
    if !native_macho_host("roundtrip", "rt_add_w") {
        return;
    }
    roundtrip(".text\nadd w3, w4, w5\n");
}
#[test]
fn rt_add_shift_reg() {
    if !native_macho_host("roundtrip", "rt_add_shift_reg") {
        return;
    }
    roundtrip(".text\nadd x0, x1, x2, lsl #3\n");
}
#[test]
fn rt_sub_shift_reg() {
    if !native_macho_host("roundtrip", "rt_sub_shift_reg") {
        return;
    }
    roundtrip(".text\nsub w3, w4, w5, asr #7\n");
}
#[test]
fn rt_cmp_shift_reg() {
    if !native_macho_host("roundtrip", "rt_cmp_shift_reg") {
        return;
    }
    roundtrip(".text\ncmp x6, x7, lsr #4\n");
}
#[test]
fn rt_add_ext_reg() {
    if !native_macho_host("roundtrip", "rt_add_ext_reg") {
        return;
    }
    roundtrip(".text\nadd x0, x0, w1, sxtw #3\n");
}
#[test]
fn rt_add_ext_reg_sp_base() {
    if !native_macho_host("roundtrip", "rt_add_ext_reg_sp_base") {
        return;
    }
    roundtrip(".text\nadd x11, sp, w12, sxtw #2\n");
}
#[test]
fn rt_sub_ext_reg() {
    if !native_macho_host("roundtrip", "rt_sub_ext_reg") {
        return;
    }
    roundtrip(".text\nsub x2, x3, w4, uxtw #2\n");
}
#[test]
fn rt_cmp_ext_reg() {
    if !native_macho_host("roundtrip", "rt_cmp_ext_reg") {
        return;
    }
    roundtrip(".text\ncmp x0, w1, sxtw\n");
}
#[test]
fn rt_add_imm() {
    if !native_macho_host("roundtrip", "rt_add_imm") {
        return;
    }
    roundtrip(".text\nadd x0, x1, #100\n");
}
#[test]
fn rt_sub_imm() {
    if !native_macho_host("roundtrip", "rt_sub_imm") {
        return;
    }
    roundtrip(".text\nsub x3, x4, #200\n");
}
#[test]
fn rt_mul() {
    if !native_macho_host("roundtrip", "rt_mul") {
        return;
    }
    roundtrip(".text\nmul x0, x1, x2\n");
}
#[test]
fn rt_madd() {
    if !native_macho_host("roundtrip", "rt_madd") {
        return;
    }
    roundtrip(".text\nmadd w0, w0, w0, w8\n");
}
#[test]
fn rt_msub() {
    if !native_macho_host("roundtrip", "rt_msub") {
        return;
    }
    roundtrip(".text\nmsub w9, w8, w1, w0\n");
}
#[test]
fn rt_umull() {
    if !native_macho_host("roundtrip", "rt_umull") {
        return;
    }
    roundtrip(".text\numull x9, w8, w9\n");
}
#[test]
fn rt_sdiv() {
    if !native_macho_host("roundtrip", "rt_sdiv") {
        return;
    }
    roundtrip(".text\nsdiv x3, x4, x5\n");
}
#[test]
fn rt_udiv() {
    if !native_macho_host("roundtrip", "rt_udiv") {
        return;
    }
    roundtrip(".text\nudiv x6, x7, x8\n");
}
#[test]
fn rt_and() {
    if !native_macho_host("roundtrip", "rt_and") {
        return;
    }
    roundtrip(".text\nand x0, x1, x2\n");
}
#[test]
fn rt_and_imm() {
    if !native_macho_host("roundtrip", "rt_and_imm") {
        return;
    }
    roundtrip(".text\nand w8, w8, #0x7\n");
}
#[test]
fn rt_orr() {
    if !native_macho_host("roundtrip", "rt_orr") {
        return;
    }
    roundtrip(".text\norr x0, x1, x2\n");
}
#[test]
fn rt_neg() {
    if !native_macho_host("roundtrip", "rt_neg") {
        return;
    }
    roundtrip(".text\nneg x0, x1\n");
}
#[test]
fn rt_mvn() {
    if !native_macho_host("roundtrip", "rt_mvn") {
        return;
    }
    roundtrip(".text\nmvn x0, x1\n");
}
#[test]
fn rt_eor() {
    if !native_macho_host("roundtrip", "rt_eor") {
        return;
    }
    roundtrip(".text\neor x0, x1, x2\n");
}
#[test]
fn rt_cmp_reg() {
    if !native_macho_host("roundtrip", "rt_cmp_reg") {
        return;
    }
    roundtrip(".text\ncmp x5, x6\n");
}
#[test]
fn rt_cmp_imm() {
    if !native_macho_host("roundtrip", "rt_cmp_imm") {
        return;
    }
    roundtrip(".text\ncmp x5, #42\n");
}
#[test]
fn rt_tst() {
    if !native_macho_host("roundtrip", "rt_tst") {
        return;
    }
    roundtrip(".text\ntst x0, x1\n");
}
#[test]
fn rt_tst_imm() {
    if !native_macho_host("roundtrip", "rt_tst_imm") {
        return;
    }
    roundtrip(".text\ntst w8, #0x7\n");
}
#[test]
fn rt_csel() {
    if !native_macho_host("roundtrip", "rt_csel") {
        return;
    }
    roundtrip(".text\ncsel w0, w0, w1, gt\n");
}
#[test]
fn rt_ccmp() {
    if !native_macho_host("roundtrip", "rt_ccmp") {
        return;
    }
    roundtrip(".text\nccmp w0, #3, #4, ne\n");
}
#[test]
fn rt_ccmn() {
    if !native_macho_host("roundtrip", "rt_ccmn") {
        return;
    }
    roundtrip(".text\nccmn x3, #9, #1, ge\n");
}
#[test]
fn rt_csinc() {
    if !native_macho_host("roundtrip", "rt_csinc") {
        return;
    }
    roundtrip(".text\ncsinc x2, x3, x4, ne\n");
}
#[test]
fn rt_csinv() {
    if !native_macho_host("roundtrip", "rt_csinv") {
        return;
    }
    roundtrip(".text\ncsinv x2, x3, x4, ne\n");
}
#[test]
fn rt_csneg() {
    if !native_macho_host("roundtrip", "rt_csneg") {
        return;
    }
    roundtrip(".text\ncsneg x5, x6, x7, gt\n");
}
#[test]
fn rt_cset() {
    if !native_macho_host("roundtrip", "rt_cset") {
        return;
    }
    roundtrip(".text\ncset x0, eq\n");
}
#[test]
fn rt_csetm() {
    if !native_macho_host("roundtrip", "rt_csetm") {
        return;
    }
    roundtrip(".text\ncsetm w8, eq\n");
}
#[test]
fn rt_cinc() {
    if !native_macho_host("roundtrip", "rt_cinc") {
        return;
    }
    roundtrip(".text\ncinc w2, w3, ne\n");
}
#[test]
fn rt_cinv() {
    if !native_macho_host("roundtrip", "rt_cinv") {
        return;
    }
    roundtrip(".text\ncinv w9, w10, mi\n");
}
#[test]
fn rt_cneg() {
    if !native_macho_host("roundtrip", "rt_cneg") {
        return;
    }
    roundtrip(".text\ncneg x11, x12, lt\n");
}
#[test]
fn rt_ubfiz() {
    if !native_macho_host("roundtrip", "rt_ubfiz") {
        return;
    }
    roundtrip(".text\nubfiz w8, w0, #5, #3\n");
}
#[test]
fn rt_bfi() {
    if !native_macho_host("roundtrip", "rt_bfi") {
        return;
    }
    roundtrip(".text\nbfi w0, w8, #5, #27\n");
}
#[test]
fn rt_bfxil() {
    if !native_macho_host("roundtrip", "rt_bfxil") {
        return;
    }
    roundtrip(".text\nbfxil w8, w0, #3, #5\n");
}
#[test]
fn rt_movz() {
    if !native_macho_host("roundtrip", "rt_movz") {
        return;
    }
    roundtrip(".text\nmovz x0, #0x1234\n");
}
#[test]
fn rt_movz_lsl16() {
    if !native_macho_host("roundtrip", "rt_movz_lsl16") {
        return;
    }
    roundtrip(".text\nmovz x0, #0x5678, lsl #16\n");
}
#[test]
fn rt_movk() {
    if !native_macho_host("roundtrip", "rt_movk") {
        return;
    }
    roundtrip(".text\nmovk x0, #0xABCD, lsl #32\n");
}
#[test]
fn rt_mov_neg_large() {
    if !native_macho_host("roundtrip", "rt_mov_neg_large") {
        return;
    }
    roundtrip(".text\nmov x0, #-65537\n");
}
#[test]
fn rt_lsl() {
    if !native_macho_host("roundtrip", "rt_lsl") {
        return;
    }
    roundtrip(".text\nlsl x0, x1, #7\n");
}
#[test]
fn rt_lsr() {
    if !native_macho_host("roundtrip", "rt_lsr") {
        return;
    }
    roundtrip(".text\nlsr x0, x1, #15\n");
}
#[test]
fn rt_asr() {
    if !native_macho_host("roundtrip", "rt_asr") {
        return;
    }
    roundtrip(".text\nasr x0, x1, #31\n");
}
#[test]
fn rt_b() {
    if !native_macho_host("roundtrip", "rt_b") {
        return;
    }
    roundtrip(".text\nb #20\n");
}
#[test]
fn rt_bl() {
    if !native_macho_host("roundtrip", "rt_bl") {
        return;
    }
    roundtrip(".text\nbl #40\n");
}
#[test]
fn rt_b_eq() {
    if !native_macho_host("roundtrip", "rt_b_eq") {
        return;
    }
    roundtrip(".text\nb.eq #24\n");
}
#[test]
fn rt_b_ne() {
    if !native_macho_host("roundtrip", "rt_b_ne") {
        return;
    }
    roundtrip(".text\nb.ne #32\n");
}
#[test]
fn rt_b_lt() {
    if !native_macho_host("roundtrip", "rt_b_lt") {
        return;
    }
    roundtrip(".text\nb.lt #16\n");
}
#[test]
fn rt_b_ge() {
    if !native_macho_host("roundtrip", "rt_b_ge") {
        return;
    }
    roundtrip(".text\nb.ge #8\n");
}
#[test]
fn rt_b_gt() {
    if !native_macho_host("roundtrip", "rt_b_gt") {
        return;
    }
    roundtrip(".text\nb.gt #12\n");
}
#[test]
fn rt_b_le() {
    if !native_macho_host("roundtrip", "rt_b_le") {
        return;
    }
    roundtrip(".text\nb.le #20\n");
}
#[test]
fn rt_cbz() {
    if !native_macho_host("roundtrip", "rt_cbz") {
        return;
    }
    roundtrip(".text\ncbz x5, #16\n");
}
#[test]
fn rt_cbnz() {
    if !native_macho_host("roundtrip", "rt_cbnz") {
        return;
    }
    roundtrip(".text\ncbnz x10, #24\n");
}
#[test]
fn rt_tbz() {
    if !native_macho_host("roundtrip", "rt_tbz") {
        return;
    }
    roundtrip(".text\ntbz x0, #5, #8\n");
}
#[test]
fn rt_tbnz() {
    if !native_macho_host("roundtrip", "rt_tbnz") {
        return;
    }
    roundtrip(".text\ntbnz x1, #33, #12\n");
}
#[test]
fn rt_ret() {
    if !native_macho_host("roundtrip", "rt_ret") {
        return;
    }
    roundtrip(".text\nret\n");
}
#[test]
fn rt_br() {
    if !native_macho_host("roundtrip", "rt_br") {
        return;
    }
    roundtrip(".text\nbr x8\n");
}
#[test]
fn rt_blr() {
    if !native_macho_host("roundtrip", "rt_blr") {
        return;
    }
    roundtrip(".text\nblr x9\n");
}
#[test]
fn rt_adr() {
    if !native_macho_host("roundtrip", "rt_adr") {
        return;
    }
    roundtrip(".text\nadr x0, #8\n");
}
#[test]
fn rt_adrp_tlvp() {
    if !native_macho_host("roundtrip", "rt_adrp_tlvp") {
        return;
    }
    roundtrip(".text\nadrp x0, _tls_counter@TLVPPAGE\n");
}
#[test]
fn rt_ldr64() {
    if !native_macho_host("roundtrip", "rt_ldr64") {
        return;
    }
    roundtrip(".text\nldr x0, [x1, #24]\n");
}
#[test]
fn rt_ldr64_tlvp_pageoff() {
    if !native_macho_host("roundtrip", "rt_ldr64_tlvp_pageoff") {
        return;
    }
    roundtrip(".text\nldr x0, [x0, _tls_counter@TLVPPAGEOFF]\n");
}
#[test]
fn rt_str64() {
    if !native_macho_host("roundtrip", "rt_str64") {
        return;
    }
    roundtrip(".text\nstr x2, [x3, #32]\n");
}
#[test]
fn rt_ldr32() {
    if !native_macho_host("roundtrip", "rt_ldr32") {
        return;
    }
    roundtrip(".text\nldr w4, [x5, #12]\n");
}
#[test]
fn rt_str32() {
    if !native_macho_host("roundtrip", "rt_str32") {
        return;
    }
    roundtrip(".text\nstr w6, [x7, #16]\n");
}
#[test]
fn rt_ldur64() {
    if !native_macho_host("roundtrip", "rt_ldur64") {
        return;
    }
    roundtrip(".text\nldur x9, [x29, #-8]\n");
}
#[test]
fn rt_stur32() {
    if !native_macho_host("roundtrip", "rt_stur32") {
        return;
    }
    roundtrip(".text\nstur w6, [x7, #-4]\n");
}
#[test]
fn rt_ldr_negative_alias() {
    if !native_macho_host("roundtrip", "rt_ldr_negative_alias") {
        return;
    }
    roundtrip(".text\nldr x0, [x1, #-8]\n");
}
#[test]
fn rt_ldr_post32() {
    if !native_macho_host("roundtrip", "rt_ldr_post32") {
        return;
    }
    roundtrip(".text\nldr w0, [x1], #4\n");
}
#[test]
fn rt_str_pre32() {
    if !native_macho_host("roundtrip", "rt_str_pre32") {
        return;
    }
    roundtrip(".text\nstr w2, [x3, #-4]!\n");
}
#[test]
fn rt_ldr_lit64() {
    if !native_macho_host("roundtrip", "rt_ldr_lit64") {
        return;
    }
    roundtrip(".text\nldr x0, #8\n");
}
#[test]
fn rt_ldr_lit32() {
    if !native_macho_host("roundtrip", "rt_ldr_lit32") {
        return;
    }
    roundtrip(".text\nldr w1, #12\n");
}
#[test]
fn rt_ldr64_reg() {
    if !native_macho_host("roundtrip", "rt_ldr64_reg") {
        return;
    }
    roundtrip(".text\nldr x0, [x1, x2]\n");
}
#[test]
fn rt_ldr64_reg_uxtw() {
    if !native_macho_host("roundtrip", "rt_ldr64_reg_uxtw") {
        return;
    }
    roundtrip(".text\nldr x6, [x7, w8, uxtw #3]\n");
}
#[test]
fn rt_str64_reg() {
    if !native_macho_host("roundtrip", "rt_str64_reg") {
        return;
    }
    roundtrip(".text\nstr x12, [x13, x14]\n");
}
#[test]
fn rt_ldrb() {
    if !native_macho_host("roundtrip", "rt_ldrb") {
        return;
    }
    roundtrip(".text\nldrb w0, [x1, #3]\n");
}
#[test]
fn rt_ldrsb() {
    if !native_macho_host("roundtrip", "rt_ldrsb") {
        return;
    }
    roundtrip(".text\nldrsb w0, [x1, #3]\n");
}
#[test]
fn rt_ldrb_post() {
    if !native_macho_host("roundtrip", "rt_ldrb_post") {
        return;
    }
    roundtrip(".text\nldrb w9, [x1], #1\n");
}
#[test]
fn rt_ldrsb_post() {
    if !native_macho_host("roundtrip", "rt_ldrsb_post") {
        return;
    }
    roundtrip(".text\nldrsb x9, [x1], #1\n");
}
#[test]
fn rt_ldrh() {
    if !native_macho_host("roundtrip", "rt_ldrh") {
        return;
    }
    roundtrip(".text\nldrh w2, [x3, #6]\n");
}
#[test]
fn rt_ldrsh() {
    if !native_macho_host("roundtrip", "rt_ldrsh") {
        return;
    }
    roundtrip(".text\nldrsh x0, [x1, #4]\n");
}
#[test]
fn rt_strb() {
    if !native_macho_host("roundtrip", "rt_strb") {
        return;
    }
    roundtrip(".text\nstrb w8, [x9]\n");
}
#[test]
fn rt_strb_post() {
    if !native_macho_host("roundtrip", "rt_strb_post") {
        return;
    }
    roundtrip(".text\nstrb w9, [x8], #1\n");
}
#[test]
fn rt_strh_pre() {
    if !native_macho_host("roundtrip", "rt_strh_pre") {
        return;
    }
    roundtrip(".text\nstrh w5, [x6, #2]!\n");
}
#[test]
fn rt_ldrsw() {
    if !native_macho_host("roundtrip", "rt_ldrsw") {
        return;
    }
    roundtrip(".text\nldrsw x0, [x1, #8]\n");
}
#[test]
fn rt_ldrh_reg() {
    if !native_macho_host("roundtrip", "rt_ldrh_reg") {
        return;
    }
    roundtrip(".text\nldrh w3, [x4, w5, uxtw #1]\n");
}
#[test]
fn rt_ldrsh_reg() {
    if !native_macho_host("roundtrip", "rt_ldrsh_reg") {
        return;
    }
    roundtrip(".text\nldrsh w3, [x4, w5, uxtw #1]\n");
}
#[test]
fn rt_ldrb_reg() {
    if !native_macho_host("roundtrip", "rt_ldrb_reg") {
        return;
    }
    roundtrip(".text\nldrb w0, [x1, x2]\n");
}
#[test]
fn rt_ldrsw_reg() {
    if !native_macho_host("roundtrip", "rt_ldrsw_reg") {
        return;
    }
    roundtrip(".text\nldrsw x6, [x7, w8, sxtw #2]\n");
}
#[test]
fn rt_ldaprb() {
    if !native_macho_host("roundtrip", "rt_ldaprb") {
        return;
    }
    roundtrip(".text\nldaprb w0, [x1]\n");
}
#[test]
fn rt_ldaprh() {
    if !native_macho_host("roundtrip", "rt_ldaprh") {
        return;
    }
    roundtrip(".text\nldaprh w2, [x3]\n");
}
#[test]
fn rt_ldapr() {
    if !native_macho_host("roundtrip", "rt_ldapr") {
        return;
    }
    roundtrip(".text\nldapr w8, [x9]\n");
}
#[test]
fn rt_stlrb() {
    if !native_macho_host("roundtrip", "rt_stlrb") {
        return;
    }
    roundtrip(".text\nstlrb w4, [x5]\n");
}
#[test]
fn rt_stlrh() {
    if !native_macho_host("roundtrip", "rt_stlrh") {
        return;
    }
    roundtrip(".text\nstlrh w6, [x7]\n");
}
#[test]
fn rt_stlr() {
    if !native_macho_host("roundtrip", "rt_stlr") {
        return;
    }
    roundtrip(".text\nstlr x10, [x11]\n");
}
#[test]
fn rt_ldaddalb() {
    if !native_macho_host("roundtrip", "rt_ldaddalb") {
        return;
    }
    roundtrip(".text\nldaddalb w0, w1, [x2]\n");
}
#[test]
fn rt_ldaddalh() {
    if !native_macho_host("roundtrip", "rt_ldaddalh") {
        return;
    }
    roundtrip(".text\nldaddalh w3, w4, [x5]\n");
}
#[test]
fn rt_ldumaxalb() {
    if !native_macho_host("roundtrip", "rt_ldumaxalb") {
        return;
    }
    roundtrip(".text\nldumaxalb w0, w1, [x2]\n");
}
#[test]
fn rt_ldumaxalh() {
    if !native_macho_host("roundtrip", "rt_ldumaxalh") {
        return;
    }
    roundtrip(".text\nldumaxalh w3, w4, [x5]\n");
}
#[test]
fn rt_ldsmaxalb() {
    if !native_macho_host("roundtrip", "rt_ldsmaxalb") {
        return;
    }
    roundtrip(".text\nldsmaxalb w18, w19, [x20]\n");
}
#[test]
fn rt_ldsmaxalh() {
    if !native_macho_host("roundtrip", "rt_ldsmaxalh") {
        return;
    }
    roundtrip(".text\nldsmaxalh w24, w25, [x26]\n");
}
#[test]
fn rt_ldaddal() {
    if !native_macho_host("roundtrip", "rt_ldaddal") {
        return;
    }
    roundtrip(".text\nldaddal w0, w8, [x8]\n");
}
#[test]
fn rt_ldumaxal() {
    if !native_macho_host("roundtrip", "rt_ldumaxal") {
        return;
    }
    roundtrip(".text\nldumaxal x9, x10, [x11]\n");
}
#[test]
fn rt_ldsmaxal() {
    if !native_macho_host("roundtrip", "rt_ldsmaxal") {
        return;
    }
    roundtrip(".text\nldsmaxal w0, w1, [x2]\n");
}
#[test]
fn rt_ldsminal() {
    if !native_macho_host("roundtrip", "rt_ldsminal") {
        return;
    }
    roundtrip(".text\nldsminal x9, x10, [x11]\n");
}
#[test]
fn rt_lduminalb() {
    if !native_macho_host("roundtrip", "rt_lduminalb") {
        return;
    }
    roundtrip(".text\nlduminalb w12, w13, [x14]\n");
}
#[test]
fn rt_lduminalh() {
    if !native_macho_host("roundtrip", "rt_lduminalh") {
        return;
    }
    roundtrip(".text\nlduminalh w15, w16, [x17]\n");
}
#[test]
fn rt_ldsminalb() {
    if !native_macho_host("roundtrip", "rt_ldsminalb") {
        return;
    }
    roundtrip(".text\nldsminalb w21, w22, [x23]\n");
}
#[test]
fn rt_ldsminalh() {
    if !native_macho_host("roundtrip", "rt_ldsminalh") {
        return;
    }
    roundtrip(".text\nldsminalh w27, w28, [x29]\n");
}
#[test]
fn rt_lduminal() {
    if !native_macho_host("roundtrip", "rt_lduminal") {
        return;
    }
    roundtrip(".text\nlduminal w18, w19, [x20]\n");
}
#[test]
fn rt_ldclral() {
    if !native_macho_host("roundtrip", "rt_ldclral") {
        return;
    }
    roundtrip(".text\nldclral w12, w13, [x14]\n");
}
#[test]
fn rt_ldclralb() {
    if !native_macho_host("roundtrip", "rt_ldclralb") {
        return;
    }
    roundtrip(".text\nldclralb w6, w7, [x8]\n");
}
#[test]
fn rt_ldclralh() {
    if !native_macho_host("roundtrip", "rt_ldclralh") {
        return;
    }
    roundtrip(".text\nldclralh w15, w16, [x17]\n");
}
#[test]
fn rt_ldeoral() {
    if !native_macho_host("roundtrip", "rt_ldeoral") {
        return;
    }
    roundtrip(".text\nldeoral x9, x10, [x11]\n");
}
#[test]
fn rt_ldeoralb() {
    if !native_macho_host("roundtrip", "rt_ldeoralb") {
        return;
    }
    roundtrip(".text\nldeoralb w3, w4, [x5]\n");
}
#[test]
fn rt_ldeoralh() {
    if !native_macho_host("roundtrip", "rt_ldeoralh") {
        return;
    }
    roundtrip(".text\nldeoralh w12, w13, [x14]\n");
}
#[test]
fn rt_ldsetal() {
    if !native_macho_host("roundtrip", "rt_ldsetal") {
        return;
    }
    roundtrip(".text\nldsetal w0, w1, [x2]\n");
}
#[test]
fn rt_ldsetalb() {
    if !native_macho_host("roundtrip", "rt_ldsetalb") {
        return;
    }
    roundtrip(".text\nldsetalb w0, w1, [x2]\n");
}
#[test]
fn rt_ldsetalh() {
    if !native_macho_host("roundtrip", "rt_ldsetalh") {
        return;
    }
    roundtrip(".text\nldsetalh w9, w10, [x11]\n");
}
#[test]
fn rt_swpal() {
    if !native_macho_host("roundtrip", "rt_swpal") {
        return;
    }
    roundtrip(".text\nswpal w0, w0, [x8]\n");
}
#[test]
fn rt_swpalb() {
    if !native_macho_host("roundtrip", "rt_swpalb") {
        return;
    }
    roundtrip(".text\nswpalb w8, w9, [x10]\n");
}
#[test]
fn rt_swpalh() {
    if !native_macho_host("roundtrip", "rt_swpalh") {
        return;
    }
    roundtrip(".text\nswpalh w11, w12, [x13]\n");
}
#[test]
fn rt_casalb() {
    if !native_macho_host("roundtrip", "rt_casalb") {
        return;
    }
    roundtrip(".text\ncasalb w6, w7, [x8]\n");
}
#[test]
fn rt_casalh() {
    if !native_macho_host("roundtrip", "rt_casalh") {
        return;
    }
    roundtrip(".text\ncasalh w9, w10, [x11]\n");
}
#[test]
fn rt_subs_uxtb() {
    if !native_macho_host("roundtrip", "rt_subs_uxtb") {
        return;
    }
    roundtrip(".text\nsubs w10, w8, w9, uxtb\n");
}
#[test]
fn rt_subs_uxth() {
    if !native_macho_host("roundtrip", "rt_subs_uxth") {
        return;
    }
    roundtrip(".text\nsubs w11, w12, w13, uxth\n");
}
#[test]
fn rt_subs_sxtb() {
    if !native_macho_host("roundtrip", "rt_subs_sxtb") {
        return;
    }
    roundtrip(".text\nsubs x14, x15, w16, sxtb\n");
}
#[test]
fn rt_subs_sxth() {
    if !native_macho_host("roundtrip", "rt_subs_sxth") {
        return;
    }
    roundtrip(".text\nsubs x17, x18, w19, sxth #1\n");
}
#[test]
fn rt_swpal_x() {
    if !native_macho_host("roundtrip", "rt_swpal_x") {
        return;
    }
    roundtrip(".text\nswpal x1, x2, [x3]\n");
}
#[test]
fn rt_casal() {
    if !native_macho_host("roundtrip", "rt_casal") {
        return;
    }
    roundtrip(".text\ncasal w4, w5, [x6]\n");
}
#[test]
fn rt_casal_x() {
    if !native_macho_host("roundtrip", "rt_casal_x") {
        return;
    }
    roundtrip(".text\ncasal x7, x8, [x9]\n");
}
#[test]
fn rt_ldrsw_lit() {
    if !native_macho_host("roundtrip", "rt_ldrsw_lit") {
        return;
    }
    roundtrip(".text\nldrsw x1, #8\n");
}
#[test]
fn rt_ldr_d() {
    if !native_macho_host("roundtrip", "rt_ldr_d") {
        return;
    }
    roundtrip(".text\nldr d0, [x1]\n");
}
#[test]
fn rt_ldr_q() {
    if !native_macho_host("roundtrip", "rt_ldr_q") {
        return;
    }
    roundtrip(".text\nldr q0, [sp, #16]\n");
}
#[test]
fn rt_ldr_h() {
    if !native_macho_host("roundtrip", "rt_ldr_h") {
        return;
    }
    roundtrip(".text\nldr h2, [sp, #14]\n");
}
#[test]
fn rt_ldr_b() {
    if !native_macho_host("roundtrip", "rt_ldr_b") {
        return;
    }
    roundtrip(".text\nldr b2, [sp, #15]\n");
}
#[test]
fn rt_str_q() {
    if !native_macho_host("roundtrip", "rt_str_q") {
        return;
    }
    roundtrip(".text\nstr q1, [x0]\n");
}
#[test]
fn rt_str_h() {
    if !native_macho_host("roundtrip", "rt_str_h") {
        return;
    }
    roundtrip(".text\nstr h2, [sp, #14]\n");
}
#[test]
fn rt_str_b() {
    if !native_macho_host("roundtrip", "rt_str_b") {
        return;
    }
    roundtrip(".text\nstr b2, [sp, #15]\n");
}
#[test]
fn rt_ldr_q_lit() {
    if !native_macho_host("roundtrip", "rt_ldr_q_lit") {
        return;
    }
    roundtrip(".text\nldr q0, #16\n");
}
#[test]
fn rt_ldr_q_reg() {
    if !native_macho_host("roundtrip", "rt_ldr_q_reg") {
        return;
    }
    roundtrip(".text\nldr q0, [x1, x2]\n");
}
#[test]
fn rt_str_q_reg_uxtw() {
    if !native_macho_host("roundtrip", "rt_str_q_reg_uxtw") {
        return;
    }
    roundtrip(".text\nstr q1, [x3, w4, uxtw #4]\n");
}
#[test]
fn rt_ldr_q_post() {
    if !native_macho_host("roundtrip", "rt_ldr_q_post") {
        return;
    }
    roundtrip(".text\nldr q0, [sp], #16\n");
}
#[test]
fn rt_str_q_pre() {
    if !native_macho_host("roundtrip", "rt_str_q_pre") {
        return;
    }
    roundtrip(".text\nstr q1, [sp, #-16]!\n");
}
#[test]
fn rt_str_d_off() {
    if !native_macho_host("roundtrip", "rt_str_d_off") {
        return;
    }
    roundtrip(".text\nstr d2, [x3, #16]\n");
}
#[test]
fn rt_ldr_s_reg() {
    if !native_macho_host("roundtrip", "rt_ldr_s_reg") {
        return;
    }
    roundtrip(".text\nldr s4, [x5, x6]\n");
}
#[test]
fn rt_str_s_reg_uxtw() {
    if !native_macho_host("roundtrip", "rt_str_s_reg_uxtw") {
        return;
    }
    roundtrip(".text\nstr s7, [x8, w9, uxtw #2]\n");
}
#[test]
fn rt_ldr_d_lit() {
    if !native_macho_host("roundtrip", "rt_ldr_d_lit") {
        return;
    }
    roundtrip(".text\nldr d10, #8\n");
}
#[test]
fn rt_ldr_d_post() {
    if !native_macho_host("roundtrip", "rt_ldr_d_post") {
        return;
    }
    roundtrip(".text\nldr d0, [sp], #8\n");
}
#[test]
fn rt_str_s_pre() {
    if !native_macho_host("roundtrip", "rt_str_s_pre") {
        return;
    }
    roundtrip(".text\nstr s3, [sp, #-8]!\n");
}
#[test]
fn rt_stp_pre() {
    if !native_macho_host("roundtrip", "rt_stp_pre") {
        return;
    }
    roundtrip(".text\nstp x29, x30, [sp, #-16]!\n");
}
#[test]
fn rt_stp_post() {
    if !native_macho_host("roundtrip", "rt_stp_post") {
        return;
    }
    roundtrip(".text\nstp x29, x30, [sp], #16\n");
}
#[test]
fn rt_ldp_pre() {
    if !native_macho_host("roundtrip", "rt_ldp_pre") {
        return;
    }
    roundtrip(".text\nldp x29, x30, [sp, #-16]!\n");
}
#[test]
fn rt_ldp_post() {
    if !native_macho_host("roundtrip", "rt_ldp_post") {
        return;
    }
    roundtrip(".text\nldp x29, x30, [sp], #16\n");
}
#[test]
fn rt_stp_off() {
    if !native_macho_host("roundtrip", "rt_stp_off") {
        return;
    }
    roundtrip(".text\nstp x19, x20, [sp, #16]\n");
}
#[test]
fn rt_ldp_off() {
    if !native_macho_host("roundtrip", "rt_ldp_off") {
        return;
    }
    roundtrip(".text\nldp x21, x22, [sp, #48]\n");
}
#[test]
fn rt_ldp_off32() {
    if !native_macho_host("roundtrip", "rt_ldp_off32") {
        return;
    }
    roundtrip(".text\nldp w9, w8, [x8]\n");
}
#[test]
fn rt_stp_off32() {
    if !native_macho_host("roundtrip", "rt_stp_off32") {
        return;
    }
    roundtrip(".text\nstp w1, w2, [sp, #16]\n");
}
#[test]
fn rt_ldp_post32() {
    if !native_macho_host("roundtrip", "rt_ldp_post32") {
        return;
    }
    roundtrip(".text\nldp w9, w8, [sp], #8\n");
}
#[test]
fn rt_ldp_pre32() {
    if !native_macho_host("roundtrip", "rt_ldp_pre32") {
        return;
    }
    roundtrip(".text\nldp w9, w8, [sp, #-8]!\n");
}
#[test]
fn rt_ldp_d_pre() {
    if !native_macho_host("roundtrip", "rt_ldp_d_pre") {
        return;
    }
    roundtrip(".text\nldp d8, d9, [sp, #-16]!\n");
}
#[test]
fn rt_stp_d_post() {
    if !native_macho_host("roundtrip", "rt_stp_d_post") {
        return;
    }
    roundtrip(".text\nstp d10, d11, [sp], #16\n");
}
#[test]
fn rt_ldp_d_off() {
    if !native_macho_host("roundtrip", "rt_ldp_d_off") {
        return;
    }
    roundtrip(".text\nldp d12, d13, [sp, #32]\n");
}
#[test]
fn rt_stp_s_post() {
    if !native_macho_host("roundtrip", "rt_stp_s_post") {
        return;
    }
    roundtrip(".text\nstp s0, s1, [sp], #8\n");
}
#[test]
fn rt_ldp_s_pre() {
    if !native_macho_host("roundtrip", "rt_ldp_s_pre") {
        return;
    }
    roundtrip(".text\nldp s2, s3, [sp, #-8]!\n");
}
#[test]
fn rt_stp_q_pre() {
    if !native_macho_host("roundtrip", "rt_stp_q_pre") {
        return;
    }
    roundtrip(".text\nstp q0, q1, [sp, #-32]!\n");
}
#[test]
fn rt_ldp_q_post() {
    if !native_macho_host("roundtrip", "rt_ldp_q_post") {
        return;
    }
    roundtrip(".text\nldp q2, q3, [sp], #32\n");
}
#[test]
fn rt_ldp_q_off() {
    if !native_macho_host("roundtrip", "rt_ldp_q_off") {
        return;
    }
    roundtrip(".text\nldp q4, q5, [sp, #64]\n");
}
#[test]
fn rt_ldp_s_off() {
    if !native_macho_host("roundtrip", "rt_ldp_s_off") {
        return;
    }
    roundtrip(".text\nldp s4, s5, [sp, #16]\n");
}
#[test]
fn rt_fadd_d() {
    if !native_macho_host("roundtrip", "rt_fadd_d") {
        return;
    }
    roundtrip(".text\nfadd d0, d1, d2\n");
}
#[test]
fn rt_fsub_d() {
    if !native_macho_host("roundtrip", "rt_fsub_d") {
        return;
    }
    roundtrip(".text\nfsub d3, d4, d5\n");
}
#[test]
fn rt_fmul_d() {
    if !native_macho_host("roundtrip", "rt_fmul_d") {
        return;
    }
    roundtrip(".text\nfmul d6, d7, d8\n");
}
#[test]
fn rt_fdiv_d() {
    if !native_macho_host("roundtrip", "rt_fdiv_d") {
        return;
    }
    roundtrip(".text\nfdiv d9, d10, d11\n");
}
#[test]
fn rt_fadd_s() {
    if !native_macho_host("roundtrip", "rt_fadd_s") {
        return;
    }
    roundtrip(".text\nfadd s0, s1, s2\n");
}
#[test]
fn rt_fadd_2d() {
    if !native_macho_host("roundtrip", "rt_fadd_2d") {
        return;
    }
    roundtrip(".text\nfadd.2d v0, v1, v2\n");
}
#[test]
fn rt_fadd_4s() {
    if !native_macho_host("roundtrip", "rt_fadd_4s") {
        return;
    }
    roundtrip(".text\nfadd.4s v0, v1, v2\n");
}
#[test]
fn rt_add_4s() {
    if !native_macho_host("roundtrip", "rt_add_4s") {
        return;
    }
    roundtrip(".text\nadd.4s v0, v1, v2\n");
}
#[test]
fn rt_addp_2d() {
    if !native_macho_host("roundtrip", "rt_addp_2d") {
        return;
    }
    roundtrip(".text\naddp.2d v0, v1, v2\n");
}
#[test]
fn rt_addp_16b() {
    if !native_macho_host("roundtrip", "rt_addp_16b") {
        return;
    }
    roundtrip(".text\naddp.16b v6, v7, v8\n");
}
#[test]
fn rt_addp_8h() {
    if !native_macho_host("roundtrip", "rt_addp_8h") {
        return;
    }
    roundtrip(".text\naddp.8h v0, v1, v2\n");
}
#[test]
fn rt_addp_4s() {
    if !native_macho_host("roundtrip", "rt_addp_4s") {
        return;
    }
    roundtrip(".text\naddp.4s v0, v1, v2\n");
}
#[test]
fn rt_smaxp_8h() {
    if !native_macho_host("roundtrip", "rt_smaxp_8h") {
        return;
    }
    roundtrip(".text\nsmaxp.8h v6, v7, v8\n");
}
#[test]
fn rt_smaxp_16b() {
    if !native_macho_host("roundtrip", "rt_smaxp_16b") {
        return;
    }
    roundtrip(".text\nsmaxp.16b v6, v7, v8\n");
}
#[test]
fn rt_smaxp_4s() {
    if !native_macho_host("roundtrip", "rt_smaxp_4s") {
        return;
    }
    roundtrip(".text\nsmaxp.4s v6, v7, v8\n");
}
#[test]
fn rt_sminp_8h() {
    if !native_macho_host("roundtrip", "rt_sminp_8h") {
        return;
    }
    roundtrip(".text\nsminp.8h v9, v10, v11\n");
}
#[test]
fn rt_sminp_16b() {
    if !native_macho_host("roundtrip", "rt_sminp_16b") {
        return;
    }
    roundtrip(".text\nsminp.16b v9, v10, v11\n");
}
#[test]
fn rt_sminp_4s() {
    if !native_macho_host("roundtrip", "rt_sminp_4s") {
        return;
    }
    roundtrip(".text\nsminp.4s v9, v10, v11\n");
}
#[test]
fn rt_fmax_2d() {
    if !native_macho_host("roundtrip", "rt_fmax_2d") {
        return;
    }
    roundtrip(".text\nfmax.2d v0, v0, v1\n");
}
#[test]
fn rt_fmax_4s() {
    if !native_macho_host("roundtrip", "rt_fmax_4s") {
        return;
    }
    roundtrip(".text\nfmax.4s v0, v0, v1\n");
}
#[test]
fn rt_fmaxnm_4s() {
    if !native_macho_host("roundtrip", "rt_fmaxnm_4s") {
        return;
    }
    roundtrip(".text\nfmaxnm.4s v0, v1, v2\n");
}
#[test]
fn rt_fmaxnm_2d() {
    if !native_macho_host("roundtrip", "rt_fmaxnm_2d") {
        return;
    }
    roundtrip(".text\nfmaxnm.2d v0, v0, v1\n");
}
#[test]
fn rt_fmin_2d() {
    if !native_macho_host("roundtrip", "rt_fmin_2d") {
        return;
    }
    roundtrip(".text\nfmin.2d v2, v3, v4\n");
}
#[test]
fn rt_fmin_4s() {
    if !native_macho_host("roundtrip", "rt_fmin_4s") {
        return;
    }
    roundtrip(".text\nfmin.4s v2, v3, v4\n");
}
#[test]
fn rt_fminnm_4s() {
    if !native_macho_host("roundtrip", "rt_fminnm_4s") {
        return;
    }
    roundtrip(".text\nfminnm.4s v3, v4, v5\n");
}
#[test]
fn rt_fminnm_2d() {
    if !native_macho_host("roundtrip", "rt_fminnm_2d") {
        return;
    }
    roundtrip(".text\nfminnm.2d v2, v3, v4\n");
}
#[test]
fn rt_smax_4s() {
    if !native_macho_host("roundtrip", "rt_smax_4s") {
        return;
    }
    roundtrip(".text\nsmax.4s v5, v6, v7\n");
}
#[test]
fn rt_smin_4s() {
    if !native_macho_host("roundtrip", "rt_smin_4s") {
        return;
    }
    roundtrip(".text\nsmin.4s v8, v9, v10\n");
}
#[test]
fn rt_umax_4s() {
    if !native_macho_host("roundtrip", "rt_umax_4s") {
        return;
    }
    roundtrip(".text\numax.4s v0, v0, v1\n");
}
#[test]
fn rt_umaxp_8h() {
    if !native_macho_host("roundtrip", "rt_umaxp_8h") {
        return;
    }
    roundtrip(".text\numaxp.8h v0, v1, v2\n");
}
#[test]
fn rt_umaxp_16b() {
    if !native_macho_host("roundtrip", "rt_umaxp_16b") {
        return;
    }
    roundtrip(".text\numaxp.16b v0, v1, v2\n");
}
#[test]
fn rt_umaxp_4s() {
    if !native_macho_host("roundtrip", "rt_umaxp_4s") {
        return;
    }
    roundtrip(".text\numaxp.4s v0, v1, v2\n");
}
#[test]
fn rt_umin_4s() {
    if !native_macho_host("roundtrip", "rt_umin_4s") {
        return;
    }
    roundtrip(".text\numin.4s v2, v3, v4\n");
}
#[test]
fn rt_uminp_8h() {
    if !native_macho_host("roundtrip", "rt_uminp_8h") {
        return;
    }
    roundtrip(".text\numinp.8h v3, v4, v5\n");
}
#[test]
fn rt_uminp_16b() {
    if !native_macho_host("roundtrip", "rt_uminp_16b") {
        return;
    }
    roundtrip(".text\numinp.16b v3, v4, v5\n");
}
#[test]
fn rt_uminp_4s() {
    if !native_macho_host("roundtrip", "rt_uminp_4s") {
        return;
    }
    roundtrip(".text\numinp.4s v3, v4, v5\n");
}
#[test]
fn rt_addv_4s() {
    if !native_macho_host("roundtrip", "rt_addv_4s") {
        return;
    }
    roundtrip(".text\naddv.4s s0, v0\n");
}
#[test]
fn rt_addv_16b() {
    if !native_macho_host("roundtrip", "rt_addv_16b") {
        return;
    }
    roundtrip(".text\naddv.16b b0, v0\n");
}
#[test]
fn rt_addv_8h() {
    if !native_macho_host("roundtrip", "rt_addv_8h") {
        return;
    }
    roundtrip(".text\naddv.8h h0, v0\n");
}
#[test]
fn rt_faddp_2d() {
    if !native_macho_host("roundtrip", "rt_faddp_2d") {
        return;
    }
    roundtrip(".text\nfaddp.2d v0, v1, v2\n");
}
#[test]
fn rt_faddp_4s() {
    if !native_macho_host("roundtrip", "rt_faddp_4s") {
        return;
    }
    roundtrip(".text\nfaddp.4s v0, v1, v2\n");
}
#[test]
fn rt_fmaxp_2d() {
    if !native_macho_host("roundtrip", "rt_fmaxp_2d") {
        return;
    }
    roundtrip(".text\nfmaxp.2d v3, v4, v5\n");
}
#[test]
fn rt_fmaxp_4s() {
    if !native_macho_host("roundtrip", "rt_fmaxp_4s") {
        return;
    }
    roundtrip(".text\nfmaxp.4s v0, v1, v2\n");
}
#[test]
fn rt_fminp_2d() {
    if !native_macho_host("roundtrip", "rt_fminp_2d") {
        return;
    }
    roundtrip(".text\nfminp.2d v6, v7, v8\n");
}
#[test]
fn rt_fminp_4s() {
    if !native_macho_host("roundtrip", "rt_fminp_4s") {
        return;
    }
    roundtrip(".text\nfminp.4s v3, v4, v5\n");
}
#[test]
fn rt_fmaxnmp_2d() {
    if !native_macho_host("roundtrip", "rt_fmaxnmp_2d") {
        return;
    }
    roundtrip(".text\nfmaxnmp.2d v0, v1, v2\n");
}
#[test]
fn rt_fmaxnmp_4s() {
    if !native_macho_host("roundtrip", "rt_fmaxnmp_4s") {
        return;
    }
    roundtrip(".text\nfmaxnmp.4s v0, v1, v2\n");
}
#[test]
fn rt_fminnmp_2d() {
    if !native_macho_host("roundtrip", "rt_fminnmp_2d") {
        return;
    }
    roundtrip(".text\nfminnmp.2d v3, v4, v5\n");
}
#[test]
fn rt_fminnmp_4s() {
    if !native_macho_host("roundtrip", "rt_fminnmp_4s") {
        return;
    }
    roundtrip(".text\nfminnmp.4s v3, v4, v5\n");
}
#[test]
fn rt_fmla_4s() {
    if !native_macho_host("roundtrip", "rt_fmla_4s") {
        return;
    }
    roundtrip(".text\nfmla.4s v0, v1, v2\n");
}
#[test]
fn rt_fmla_2d() {
    if !native_macho_host("roundtrip", "rt_fmla_2d") {
        return;
    }
    roundtrip(".text\nfmla.2d v0, v1, v2\n");
}
#[test]
fn rt_fmls_4s() {
    if !native_macho_host("roundtrip", "rt_fmls_4s") {
        return;
    }
    roundtrip(".text\nfmls.4s v3, v4, v5\n");
}
#[test]
fn rt_fmls_2d() {
    if !native_macho_host("roundtrip", "rt_fmls_2d") {
        return;
    }
    roundtrip(".text\nfmls.2d v3, v4, v5\n");
}
#[test]
fn rt_faddp_2s() {
    if !native_macho_host("roundtrip", "rt_faddp_2s") {
        return;
    }
    roundtrip(".text\nfaddp.2s s3, v4\n");
}
#[test]
fn rt_faddp_2d_scalar() {
    if !native_macho_host("roundtrip", "rt_faddp_2d_scalar") {
        return;
    }
    roundtrip(".text\nfaddp.2d d0, v0\n");
}
#[test]
fn rt_fmaxp_2d_scalar() {
    if !native_macho_host("roundtrip", "rt_fmaxp_2d_scalar") {
        return;
    }
    roundtrip(".text\nfmaxp.2d d1, v2\n");
}
#[test]
fn rt_fminp_2d_scalar() {
    if !native_macho_host("roundtrip", "rt_fminp_2d_scalar") {
        return;
    }
    roundtrip(".text\nfminp.2d d3, v4\n");
}
#[test]
fn rt_fmaxnmp_2d_scalar() {
    if !native_macho_host("roundtrip", "rt_fmaxnmp_2d_scalar") {
        return;
    }
    roundtrip(".text\nfmaxnmp.2d d0, v0\n");
}
#[test]
fn rt_fminnmp_2d_scalar() {
    if !native_macho_host("roundtrip", "rt_fminnmp_2d_scalar") {
        return;
    }
    roundtrip(".text\nfminnmp.2d d1, v2\n");
}
#[test]
fn rt_fmaxv_4s() {
    if !native_macho_host("roundtrip", "rt_fmaxv_4s") {
        return;
    }
    roundtrip(".text\nfmaxv.4s s1, v2\n");
}
#[test]
fn rt_fmaxnmv_4s() {
    if !native_macho_host("roundtrip", "rt_fmaxnmv_4s") {
        return;
    }
    roundtrip(".text\nfmaxnmv.4s s1, v2\n");
}
#[test]
fn rt_fminv_4s() {
    if !native_macho_host("roundtrip", "rt_fminv_4s") {
        return;
    }
    roundtrip(".text\nfminv.4s s3, v4\n");
}
#[test]
fn rt_fminnmv_4s() {
    if !native_macho_host("roundtrip", "rt_fminnmv_4s") {
        return;
    }
    roundtrip(".text\nfminnmv.4s s3, v4\n");
}
#[test]
fn rt_umaxv_4s() {
    if !native_macho_host("roundtrip", "rt_umaxv_4s") {
        return;
    }
    roundtrip(".text\numaxv.4s s1, v2\n");
}
#[test]
fn rt_umaxv_16b() {
    if !native_macho_host("roundtrip", "rt_umaxv_16b") {
        return;
    }
    roundtrip(".text\numaxv.16b b0, v0\n");
}
#[test]
fn rt_umaxv_8h() {
    if !native_macho_host("roundtrip", "rt_umaxv_8h") {
        return;
    }
    roundtrip(".text\numaxv.8h h0, v0\n");
}
#[test]
fn rt_smaxv_4s() {
    if !native_macho_host("roundtrip", "rt_smaxv_4s") {
        return;
    }
    roundtrip(".text\nsmaxv.4s s3, v4\n");
}
#[test]
fn rt_smaxv_16b() {
    if !native_macho_host("roundtrip", "rt_smaxv_16b") {
        return;
    }
    roundtrip(".text\nsmaxv.16b b0, v0\n");
}
#[test]
fn rt_smaxv_8h() {
    if !native_macho_host("roundtrip", "rt_smaxv_8h") {
        return;
    }
    roundtrip(".text\nsmaxv.8h h0, v0\n");
}
#[test]
fn rt_uminv_4s() {
    if !native_macho_host("roundtrip", "rt_uminv_4s") {
        return;
    }
    roundtrip(".text\numinv.4s s1, v2\n");
}
#[test]
fn rt_uminv_16b() {
    if !native_macho_host("roundtrip", "rt_uminv_16b") {
        return;
    }
    roundtrip(".text\numinv.16b b0, v0\n");
}
#[test]
fn rt_uminv_8h() {
    if !native_macho_host("roundtrip", "rt_uminv_8h") {
        return;
    }
    roundtrip(".text\numinv.8h h0, v0\n");
}
#[test]
fn rt_sminv_4s() {
    if !native_macho_host("roundtrip", "rt_sminv_4s") {
        return;
    }
    roundtrip(".text\nsminv.4s s3, v4\n");
}
#[test]
fn rt_sminv_16b() {
    if !native_macho_host("roundtrip", "rt_sminv_16b") {
        return;
    }
    roundtrip(".text\nsminv.16b b0, v0\n");
}
#[test]
fn rt_sminv_8h() {
    if !native_macho_host("roundtrip", "rt_sminv_8h") {
        return;
    }
    roundtrip(".text\nsminv.8h h0, v0\n");
}
#[test]
fn rt_fsub_2d() {
    if !native_macho_host("roundtrip", "rt_fsub_2d") {
        return;
    }
    roundtrip(".text\nfsub.2d v3, v4, v5\n");
}
#[test]
fn rt_fsub_4s() {
    if !native_macho_host("roundtrip", "rt_fsub_4s") {
        return;
    }
    roundtrip(".text\nfsub.4s v3, v4, v5\n");
}
#[test]
fn rt_sub_4s() {
    if !native_macho_host("roundtrip", "rt_sub_4s") {
        return;
    }
    roundtrip(".text\nsub.4s v3, v4, v5\n");
}
#[test]
fn rt_fmul_2d() {
    if !native_macho_host("roundtrip", "rt_fmul_2d") {
        return;
    }
    roundtrip(".text\nfmul.2d v6, v7, v8\n");
}
#[test]
fn rt_fmul_4s() {
    if !native_macho_host("roundtrip", "rt_fmul_4s") {
        return;
    }
    roundtrip(".text\nfmul.4s v6, v7, v8\n");
}
#[test]
fn rt_fdiv_2d() {
    if !native_macho_host("roundtrip", "rt_fdiv_2d") {
        return;
    }
    roundtrip(".text\nfdiv.2d v9, v10, v11\n");
}
#[test]
fn rt_fabd_2d() {
    if !native_macho_host("roundtrip", "rt_fabd_2d") {
        return;
    }
    roundtrip(".text\nfabd.2d v0, v1, v2\n");
}
#[test]
fn rt_fdiv_4s() {
    if !native_macho_host("roundtrip", "rt_fdiv_4s") {
        return;
    }
    roundtrip(".text\nfdiv.4s v9, v10, v11\n");
}
#[test]
fn rt_fabs_4s() {
    if !native_macho_host("roundtrip", "rt_fabs_4s") {
        return;
    }
    roundtrip(".text\nfabs.4s v0, v0\n");
}
#[test]
fn rt_fneg_4s() {
    if !native_macho_host("roundtrip", "rt_fneg_4s") {
        return;
    }
    roundtrip(".text\nfneg.4s v6, v7\n");
}
#[test]
fn rt_fsqrt_4s() {
    if !native_macho_host("roundtrip", "rt_fsqrt_4s") {
        return;
    }
    roundtrip(".text\nfsqrt.4s v1, v2\n");
}
#[test]
fn rt_fabs_2d() {
    if !native_macho_host("roundtrip", "rt_fabs_2d") {
        return;
    }
    roundtrip(".text\nfabs.2d v0, v0\n");
}
#[test]
fn rt_fneg_2d() {
    if !native_macho_host("roundtrip", "rt_fneg_2d") {
        return;
    }
    roundtrip(".text\nfneg.2d v3, v4\n");
}
#[test]
fn rt_fsqrt_2d() {
    if !native_macho_host("roundtrip", "rt_fsqrt_2d") {
        return;
    }
    roundtrip(".text\nfsqrt.2d v1, v2\n");
}
#[test]
fn rt_scvtf_2d() {
    if !native_macho_host("roundtrip", "rt_scvtf_2d") {
        return;
    }
    roundtrip(".text\nscvtf.2d v0, v0\n");
}
#[test]
fn rt_ucvtf_2d() {
    if !native_macho_host("roundtrip", "rt_ucvtf_2d") {
        return;
    }
    roundtrip(".text\nucvtf.2d v1, v2\n");
}
#[test]
fn rt_fcvtzs_2d() {
    if !native_macho_host("roundtrip", "rt_fcvtzs_2d") {
        return;
    }
    roundtrip(".text\nfcvtzs.2d v3, v4\n");
}
#[test]
fn rt_fcvtzu_2d() {
    if !native_macho_host("roundtrip", "rt_fcvtzu_2d") {
        return;
    }
    roundtrip(".text\nfcvtzu.2d v5, v6\n");
}
#[test]
fn rt_frecpe_2d() {
    if !native_macho_host("roundtrip", "rt_frecpe_2d") {
        return;
    }
    roundtrip(".text\nfrecpe.2d v0, v0\n");
}
#[test]
fn rt_frecps_2d() {
    if !native_macho_host("roundtrip", "rt_frecps_2d") {
        return;
    }
    roundtrip(".text\nfrecps.2d v1, v2, v3\n");
}
#[test]
fn rt_frsqrte_2d() {
    if !native_macho_host("roundtrip", "rt_frsqrte_2d") {
        return;
    }
    roundtrip(".text\nfrsqrte.2d v4, v5\n");
}
#[test]
fn rt_frsqrts_2d() {
    if !native_macho_host("roundtrip", "rt_frsqrts_2d") {
        return;
    }
    roundtrip(".text\nfrsqrts.2d v6, v7, v8\n");
}
#[test]
fn rt_frintn_2d() {
    if !native_macho_host("roundtrip", "rt_frintn_2d") {
        return;
    }
    roundtrip(".text\nfrintn.2d v0, v0\n");
}
#[test]
fn rt_frintm_2d() {
    if !native_macho_host("roundtrip", "rt_frintm_2d") {
        return;
    }
    roundtrip(".text\nfrintm.2d v1, v2\n");
}
#[test]
fn rt_frintp_2d() {
    if !native_macho_host("roundtrip", "rt_frintp_2d") {
        return;
    }
    roundtrip(".text\nfrintp.2d v3, v4\n");
}
#[test]
fn rt_frintz_2d() {
    if !native_macho_host("roundtrip", "rt_frintz_2d") {
        return;
    }
    roundtrip(".text\nfrintz.2d v5, v6\n");
}
#[test]
fn rt_frinta_2d() {
    if !native_macho_host("roundtrip", "rt_frinta_2d") {
        return;
    }
    roundtrip(".text\nfrinta.2d v7, v8\n");
}
#[test]
fn rt_frinti_2d() {
    if !native_macho_host("roundtrip", "rt_frinti_2d") {
        return;
    }
    roundtrip(".text\nfrinti.2d v9, v10\n");
}
#[test]
fn rt_scvtf_4s() {
    if !native_macho_host("roundtrip", "rt_scvtf_4s") {
        return;
    }
    roundtrip(".text\nscvtf.4s v0, v1\n");
}
#[test]
fn rt_ucvtf_4s() {
    if !native_macho_host("roundtrip", "rt_ucvtf_4s") {
        return;
    }
    roundtrip(".text\nucvtf.4s v2, v3\n");
}
#[test]
fn rt_fcvtzs_4s() {
    if !native_macho_host("roundtrip", "rt_fcvtzs_4s") {
        return;
    }
    roundtrip(".text\nfcvtzs.4s v4, v5\n");
}
#[test]
fn rt_fcvtzu_4s() {
    if !native_macho_host("roundtrip", "rt_fcvtzu_4s") {
        return;
    }
    roundtrip(".text\nfcvtzu.4s v6, v7\n");
}
#[test]
fn rt_frecpe_4s() {
    if !native_macho_host("roundtrip", "rt_frecpe_4s") {
        return;
    }
    roundtrip(".text\nfrecpe.4s v0, v1\n");
}
#[test]
fn rt_frecps_4s() {
    if !native_macho_host("roundtrip", "rt_frecps_4s") {
        return;
    }
    roundtrip(".text\nfrecps.4s v2, v3, v4\n");
}
#[test]
fn rt_frsqrte_4s() {
    if !native_macho_host("roundtrip", "rt_frsqrte_4s") {
        return;
    }
    roundtrip(".text\nfrsqrte.4s v5, v6\n");
}
#[test]
fn rt_frsqrts_4s() {
    if !native_macho_host("roundtrip", "rt_frsqrts_4s") {
        return;
    }
    roundtrip(".text\nfrsqrts.4s v7, v8, v9\n");
}
#[test]
fn rt_frintn_4s() {
    if !native_macho_host("roundtrip", "rt_frintn_4s") {
        return;
    }
    roundtrip(".text\nfrintn.4s v0, v1\n");
}
#[test]
fn rt_frintm_4s() {
    if !native_macho_host("roundtrip", "rt_frintm_4s") {
        return;
    }
    roundtrip(".text\nfrintm.4s v2, v3\n");
}
#[test]
fn rt_frintp_4s() {
    if !native_macho_host("roundtrip", "rt_frintp_4s") {
        return;
    }
    roundtrip(".text\nfrintp.4s v4, v5\n");
}
#[test]
fn rt_frintz_4s() {
    if !native_macho_host("roundtrip", "rt_frintz_4s") {
        return;
    }
    roundtrip(".text\nfrintz.4s v6, v7\n");
}
#[test]
fn rt_frinta_4s() {
    if !native_macho_host("roundtrip", "rt_frinta_4s") {
        return;
    }
    roundtrip(".text\nfrinta.4s v0, v1\n");
}
#[test]
fn rt_frinti_4s() {
    if !native_macho_host("roundtrip", "rt_frinti_4s") {
        return;
    }
    roundtrip(".text\nfrinti.4s v2, v3\n");
}
#[test]
fn rt_and_16b() {
    if !native_macho_host("roundtrip", "rt_and_16b") {
        return;
    }
    roundtrip(".text\nand.16b v6, v7, v8\n");
}
#[test]
fn rt_bic_16b() {
    if !native_macho_host("roundtrip", "rt_bic_16b") {
        return;
    }
    roundtrip(".text\nbic.16b v5, v6, v7\n");
}
#[test]
fn rt_bif_16b() {
    if !native_macho_host("roundtrip", "rt_bif_16b") {
        return;
    }
    roundtrip(".text\nbif.16b v0, v1, v2\n");
}
#[test]
fn rt_bit_16b() {
    if !native_macho_host("roundtrip", "rt_bit_16b") {
        return;
    }
    roundtrip(".text\nbit.16b v3, v4, v5\n");
}
#[test]
fn rt_bsl_16b() {
    if !native_macho_host("roundtrip", "rt_bsl_16b") {
        return;
    }
    roundtrip(".text\nbsl.16b v6, v7, v8\n");
}
#[test]
fn rt_cmeq_4s() {
    if !native_macho_host("roundtrip", "rt_cmeq_4s") {
        return;
    }
    roundtrip(".text\ncmeq.4s v0, v0, v1\n");
}
#[test]
fn rt_fcmeq_4s() {
    if !native_macho_host("roundtrip", "rt_fcmeq_4s") {
        return;
    }
    roundtrip(".text\nfcmeq.4s v0, v1, v2\n");
}
#[test]
fn rt_fcmeq_2d() {
    if !native_macho_host("roundtrip", "rt_fcmeq_2d") {
        return;
    }
    roundtrip(".text\nfcmeq.2d v0, v1, v2\n");
}
#[test]
fn rt_cmhs_4s() {
    if !native_macho_host("roundtrip", "rt_cmhs_4s") {
        return;
    }
    roundtrip(".text\ncmhs.4s v0, v0, v1\n");
}
#[test]
fn rt_cmhi_4s() {
    if !native_macho_host("roundtrip", "rt_cmhi_4s") {
        return;
    }
    roundtrip(".text\ncmhi.4s v2, v3, v4\n");
}
#[test]
fn rt_cmge_4s() {
    if !native_macho_host("roundtrip", "rt_cmge_4s") {
        return;
    }
    roundtrip(".text\ncmge.4s v5, v6, v7\n");
}
#[test]
fn rt_fcmge_4s() {
    if !native_macho_host("roundtrip", "rt_fcmge_4s") {
        return;
    }
    roundtrip(".text\nfcmge.4s v3, v4, v5\n");
}
#[test]
fn rt_fcmge_2d() {
    if !native_macho_host("roundtrip", "rt_fcmge_2d") {
        return;
    }
    roundtrip(".text\nfcmge.2d v3, v4, v5\n");
}
#[test]
fn rt_cmgt_4s() {
    if !native_macho_host("roundtrip", "rt_cmgt_4s") {
        return;
    }
    roundtrip(".text\ncmgt.4s v2, v3, v4\n");
}
#[test]
fn rt_fcmgt_4s() {
    if !native_macho_host("roundtrip", "rt_fcmgt_4s") {
        return;
    }
    roundtrip(".text\nfcmgt.4s v6, v7, v8\n");
}
#[test]
fn rt_fcmgt_2d() {
    if !native_macho_host("roundtrip", "rt_fcmgt_2d") {
        return;
    }
    roundtrip(".text\nfcmgt.2d v6, v7, v8\n");
}
#[test]
fn rt_fcmge_2d_zero() {
    if !native_macho_host("roundtrip", "rt_fcmge_2d_zero") {
        return;
    }
    roundtrip(".text\nfcmge.2d v0, v0, #0.0\n");
}
#[test]
fn rt_fcmgt_2d_zero() {
    if !native_macho_host("roundtrip", "rt_fcmgt_2d_zero") {
        return;
    }
    roundtrip(".text\nfcmgt.2d v1, v1, #0.0\n");
}
#[test]
fn rt_fcmle_2d_zero() {
    if !native_macho_host("roundtrip", "rt_fcmle_2d_zero") {
        return;
    }
    roundtrip(".text\nfcmle.2d v2, v2, #0.0\n");
}
#[test]
fn rt_fcmlt_2d_zero() {
    if !native_macho_host("roundtrip", "rt_fcmlt_2d_zero") {
        return;
    }
    roundtrip(".text\nfcmlt.2d v3, v3, #0.0\n");
}
#[test]
fn rt_orr_16b() {
    if !native_macho_host("roundtrip", "rt_orr_16b") {
        return;
    }
    roundtrip(".text\norr.16b v9, v10, v11\n");
}
#[test]
fn rt_eor_16b() {
    if !native_macho_host("roundtrip", "rt_eor_16b") {
        return;
    }
    roundtrip(".text\neor.16b v12, v13, v14\n");
}
#[test]
fn rt_ext_16b() {
    if !native_macho_host("roundtrip", "rt_ext_16b") {
        return;
    }
    roundtrip(".text\next.16b v0, v0, v0, #8\n");
}
#[test]
fn rt_rev64_4s() {
    if !native_macho_host("roundtrip", "rt_rev64_4s") {
        return;
    }
    roundtrip(".text\nrev64.4s v1, v2\n");
}
#[test]
fn rt_zip1_4s() {
    if !native_macho_host("roundtrip", "rt_zip1_4s") {
        return;
    }
    roundtrip(".text\nzip1.4s v0, v0, v1\n");
}
#[test]
fn rt_zip1_2d() {
    if !native_macho_host("roundtrip", "rt_zip1_2d") {
        return;
    }
    roundtrip(".text\nzip1.2d v0, v1, v2\n");
}
#[test]
fn rt_zip2_4s() {
    if !native_macho_host("roundtrip", "rt_zip2_4s") {
        return;
    }
    roundtrip(".text\nzip2.4s v2, v3, v4\n");
}
#[test]
fn rt_zip2_2d() {
    if !native_macho_host("roundtrip", "rt_zip2_2d") {
        return;
    }
    roundtrip(".text\nzip2.2d v3, v4, v5\n");
}
#[test]
fn rt_uzp1_4s() {
    if !native_macho_host("roundtrip", "rt_uzp1_4s") {
        return;
    }
    roundtrip(".text\nuzp1.4s v5, v6, v7\n");
}
#[test]
fn rt_uzp1_2d() {
    if !native_macho_host("roundtrip", "rt_uzp1_2d") {
        return;
    }
    roundtrip(".text\nuzp1.2d v6, v7, v8\n");
}
#[test]
fn rt_uzp2_4s() {
    if !native_macho_host("roundtrip", "rt_uzp2_4s") {
        return;
    }
    roundtrip(".text\nuzp2.4s v8, v9, v10\n");
}
#[test]
fn rt_uzp2_2d() {
    if !native_macho_host("roundtrip", "rt_uzp2_2d") {
        return;
    }
    roundtrip(".text\nuzp2.2d v9, v10, v11\n");
}
#[test]
fn rt_trn1_4s() {
    if !native_macho_host("roundtrip", "rt_trn1_4s") {
        return;
    }
    roundtrip(".text\ntrn1.4s v11, v12, v13\n");
}
#[test]
fn rt_trn1_2d() {
    if !native_macho_host("roundtrip", "rt_trn1_2d") {
        return;
    }
    roundtrip(".text\ntrn1.2d v12, v13, v14\n");
}
#[test]
fn rt_trn2_4s() {
    if !native_macho_host("roundtrip", "rt_trn2_4s") {
        return;
    }
    roundtrip(".text\ntrn2.4s v3, v4, v5\n");
}
#[test]
fn rt_trn2_2d() {
    if !native_macho_host("roundtrip", "rt_trn2_2d") {
        return;
    }
    roundtrip(".text\ntrn2.2d v15, v16, v17\n");
}
#[test]
fn rt_tbl_16b_single() {
    if !native_macho_host("roundtrip", "rt_tbl_16b_single") {
        return;
    }
    roundtrip(".text\ntbl.16b v0, { v1 }, v2\n");
}
#[test]
fn rt_tbl_16b_pair() {
    if !native_macho_host("roundtrip", "rt_tbl_16b_pair") {
        return;
    }
    roundtrip(".text\ntbl.16b v3, { v4, v5 }, v6\n");
}
#[test]
fn rt_tbl_16b_triple() {
    if !native_macho_host("roundtrip", "rt_tbl_16b_triple") {
        return;
    }
    roundtrip(".text\ntbl.16b v7, { v8, v9, v10 }, v11\n");
}
#[test]
fn rt_tbl_16b_quad() {
    if !native_macho_host("roundtrip", "rt_tbl_16b_quad") {
        return;
    }
    roundtrip(".text\ntbl.16b v12, { v13, v14, v15, v16 }, v17\n");
}
#[test]
fn rt_tbx_16b_single() {
    if !native_macho_host("roundtrip", "rt_tbx_16b_single") {
        return;
    }
    roundtrip(".text\ntbx.16b v18, { v19 }, v20\n");
}
#[test]
fn rt_tbx_16b_pair() {
    if !native_macho_host("roundtrip", "rt_tbx_16b_pair") {
        return;
    }
    roundtrip(".text\ntbx.16b v21, { v22, v23 }, v24\n");
}
#[test]
fn rt_mov_16b() {
    if !native_macho_host("roundtrip", "rt_mov_16b") {
        return;
    }
    roundtrip(".text\nmov.16b v0, v2\n");
}
#[test]
fn rt_mov_8b() {
    if !native_macho_host("roundtrip", "rt_mov_8b") {
        return;
    }
    roundtrip(".text\nmov.8b v1, v3\n");
}
#[test]
fn rt_mov_4s() {
    if !native_macho_host("roundtrip", "rt_mov_4s") {
        return;
    }
    roundtrip(".text\nmov.4s v4, v5\n");
}
#[test]
fn rt_mov_2d() {
    if !native_macho_host("roundtrip", "rt_mov_2d") {
        return;
    }
    roundtrip(".text\nmov.2d v6, v7\n");
}
#[test]
fn rt_fmov_reg_s() {
    if !native_macho_host("roundtrip", "rt_fmov_reg_s") {
        return;
    }
    roundtrip(".text\nfmov s1, s2\n");
}
#[test]
fn rt_fmov_reg_d() {
    if !native_macho_host("roundtrip", "rt_fmov_reg_d") {
        return;
    }
    roundtrip(".text\nfmov d1, d2\n");
}
#[test]
fn rt_fmov_to_s() {
    if !native_macho_host("roundtrip", "rt_fmov_to_s") {
        return;
    }
    roundtrip(".text\nfmov s0, w1\n");
}
#[test]
fn rt_fmov_from_s() {
    if !native_macho_host("roundtrip", "rt_fmov_from_s") {
        return;
    }
    roundtrip(".text\nfmov w0, s1\n");
}
#[test]
fn rt_mov_from_lane_s() {
    if !native_macho_host("roundtrip", "rt_mov_from_lane_s") {
        return;
    }
    roundtrip(".text\nmov s0, v1[2]\n");
}
#[test]
fn rt_mov_from_lane_d() {
    if !native_macho_host("roundtrip", "rt_mov_from_lane_d") {
        return;
    }
    roundtrip(".text\nmov d3, v4[1]\n");
}
#[test]
fn rt_mov_lane_s() {
    if !native_macho_host("roundtrip", "rt_mov_lane_s") {
        return;
    }
    roundtrip(".text\nmov.s v5[0], v6[0]\n");
}
#[test]
fn rt_mov_lane_d() {
    if !native_macho_host("roundtrip", "rt_mov_lane_d") {
        return;
    }
    roundtrip(".text\nmov.d v7[1], v8[1]\n");
}
#[test]
fn rt_mov_lane_h() {
    if !native_macho_host("roundtrip", "rt_mov_lane_h") {
        return;
    }
    roundtrip(".text\nmov.h v0[5], v1[0]\n");
}
#[test]
fn rt_mov_lane_b() {
    if !native_macho_host("roundtrip", "rt_mov_lane_b") {
        return;
    }
    roundtrip(".text\nmov.b v0[7], v1[0]\n");
}
#[test]
fn rt_mov_from_lane_gp_s() {
    if !native_macho_host("roundtrip", "rt_mov_from_lane_gp_s") {
        return;
    }
    roundtrip(".text\nmov.s w0, v1[2]\n");
}
#[test]
fn rt_mov_from_lane_gp_d() {
    if !native_macho_host("roundtrip", "rt_mov_from_lane_gp_d") {
        return;
    }
    roundtrip(".text\nmov.d x0, v1[1]\n");
}
#[test]
fn rt_umov_h() {
    if !native_macho_host("roundtrip", "rt_umov_h") {
        return;
    }
    roundtrip(".text\numov.h w1, v2[5]\n");
}
#[test]
fn rt_umov_b() {
    if !native_macho_host("roundtrip", "rt_umov_b") {
        return;
    }
    roundtrip(".text\numov.b w3, v4[7]\n");
}
#[test]
fn rt_smov_h() {
    if !native_macho_host("roundtrip", "rt_smov_h") {
        return;
    }
    roundtrip(".text\nsmov.h w1, v2[3]\n");
}
#[test]
fn rt_smov_b() {
    if !native_macho_host("roundtrip", "rt_smov_b") {
        return;
    }
    roundtrip(".text\nsmov.b w0, v0[0]\n");
}
#[test]
fn rt_mov_lane_from_gp_s() {
    if !native_macho_host("roundtrip", "rt_mov_lane_from_gp_s") {
        return;
    }
    roundtrip(".text\nmov.s v5[1], w6\n");
}
#[test]
fn rt_mov_lane_from_gp_d() {
    if !native_macho_host("roundtrip", "rt_mov_lane_from_gp_d") {
        return;
    }
    roundtrip(".text\nmov.d v0[1], x1\n");
}
#[test]
fn rt_mov_lane_from_gp_h() {
    if !native_macho_host("roundtrip", "rt_mov_lane_from_gp_h") {
        return;
    }
    roundtrip(".text\nmov.h v7[5], w8\n");
}
#[test]
fn rt_mov_lane_from_gp_b() {
    if !native_macho_host("roundtrip", "rt_mov_lane_from_gp_b") {
        return;
    }
    roundtrip(".text\nmov.b v9[7], w10\n");
}
#[test]
fn rt_dup_16b() {
    if !native_macho_host("roundtrip", "rt_dup_16b") {
        return;
    }
    roundtrip(".text\ndup.16b v0, v1[15]\n");
}
#[test]
fn rt_dup_8h() {
    if !native_macho_host("roundtrip", "rt_dup_8h") {
        return;
    }
    roundtrip(".text\ndup.8h v1, v2[5]\n");
}
#[test]
fn rt_dup_4s() {
    if !native_macho_host("roundtrip", "rt_dup_4s") {
        return;
    }
    roundtrip(".text\ndup.4s v3, v4[2]\n");
}
#[test]
fn rt_dup_2d() {
    if !native_macho_host("roundtrip", "rt_dup_2d") {
        return;
    }
    roundtrip(".text\ndup.2d v5, v6[1]\n");
}
#[test]
fn rt_fneg_d() {
    if !native_macho_host("roundtrip", "rt_fneg_d") {
        return;
    }
    roundtrip(".text\nfneg d3, d4\n");
}
#[test]
fn rt_fabs_d() {
    if !native_macho_host("roundtrip", "rt_fabs_d") {
        return;
    }
    roundtrip(".text\nfabs d3, d4\n");
}
#[test]
fn rt_fsqrt_d() {
    if !native_macho_host("roundtrip", "rt_fsqrt_d") {
        return;
    }
    roundtrip(".text\nfsqrt d3, d4\n");
}
#[test]
fn rt_fcmp_d() {
    if !native_macho_host("roundtrip", "rt_fcmp_d") {
        return;
    }
    roundtrip(".text\nfcmp d3, d4\n");
}
#[test]
fn rt_fmov_imm_d() {
    if !native_macho_host("roundtrip", "rt_fmov_imm_d") {
        return;
    }
    roundtrip(".text\nfmov d2, #3.50000000\n");
}
#[test]
fn rt_fcsel_d() {
    if !native_macho_host("roundtrip", "rt_fcsel_d") {
        return;
    }
    roundtrip(".text\nfcsel d0, d0, d1, mi\n");
}
#[test]
fn rt_fmadd_d() {
    if !native_macho_host("roundtrip", "rt_fmadd_d") {
        return;
    }
    roundtrip(".text\nfmadd d0, d1, d2, d3\n");
}
#[test]
fn rt_fcvtzs() {
    if !native_macho_host("roundtrip", "rt_fcvtzs") {
        return;
    }
    roundtrip(".text\nfcvtzs x0, d1\n");
}
#[test]
fn rt_scvtf() {
    if !native_macho_host("roundtrip", "rt_scvtf") {
        return;
    }
    roundtrip(".text\nscvtf d0, x1\n");
}
#[test]
fn rt_fmov_to() {
    if !native_macho_host("roundtrip", "rt_fmov_to") {
        return;
    }
    roundtrip(".text\nfmov d5, x6\n");
}
#[test]
fn rt_fmov_from() {
    if !native_macho_host("roundtrip", "rt_fmov_from") {
        return;
    }
    roundtrip(".text\nfmov x5, d6\n");
}
#[test]
fn rt_svc() {
    if !native_macho_host("roundtrip", "rt_svc") {
        return;
    }
    roundtrip(".text\nsvc #0x80\n");
}
#[test]
fn rt_nop() {
    if !native_macho_host("roundtrip", "rt_nop") {
        return;
    }
    roundtrip(".text\nnop\n");
}
#[test]
fn rt_yield() {
    if !native_macho_host("roundtrip", "rt_yield") {
        return;
    }
    roundtrip(".text\nyield\n");
}
#[test]
fn rt_wfe() {
    if !native_macho_host("roundtrip", "rt_wfe") {
        return;
    }
    roundtrip(".text\nwfe\n");
}
#[test]
fn rt_wfi() {
    if !native_macho_host("roundtrip", "rt_wfi") {
        return;
    }
    roundtrip(".text\nwfi\n");
}
#[test]
fn rt_sev() {
    if !native_macho_host("roundtrip", "rt_sev") {
        return;
    }
    roundtrip(".text\nsev\n");
}
#[test]
fn rt_sevl() {
    if !native_macho_host("roundtrip", "rt_sevl") {
        return;
    }
    roundtrip(".text\nsevl\n");
}
#[test]
fn rt_isb() {
    if !native_macho_host("roundtrip", "rt_isb") {
        return;
    }
    roundtrip(".text\nisb\n");
}
#[test]
fn rt_dmb_ish() {
    if !native_macho_host("roundtrip", "rt_dmb_ish") {
        return;
    }
    roundtrip(".text\ndmb ish\n");
}
#[test]
fn rt_dsb_ishst() {
    if !native_macho_host("roundtrip", "rt_dsb_ishst") {
        return;
    }
    roundtrip(".text\ndsb ishst\n");
}
#[test]
fn rt_brk() {
    if !native_macho_host("roundtrip", "rt_brk") {
        return;
    }
    roundtrip(".text\nbrk #42\n");
}

// ---- Test gap coverage ----

// W-register shifts
#[test]
fn rt_lsl_w() {
    if !native_macho_host("roundtrip", "rt_lsl_w") {
        return;
    }
    roundtrip(".text\nlsl w0, w1, #3\n");
}
#[test]
fn rt_lsr_w() {
    if !native_macho_host("roundtrip", "rt_lsr_w") {
        return;
    }
    roundtrip(".text\nlsr w5, w6, #8\n");
}
#[test]
fn rt_asr_w() {
    if !native_macho_host("roundtrip", "rt_asr_w") {
        return;
    }
    roundtrip(".text\nasr w5, w6, #15\n");
}

// Negative branches
#[test]
fn rt_b_neg() {
    if !native_macho_host("roundtrip", "rt_b_neg") {
        return;
    }
    roundtrip(".text\nb #-8\n");
}
#[test]
fn rt_bl_neg() {
    if !native_macho_host("roundtrip", "rt_bl_neg") {
        return;
    }
    roundtrip(".text\nbl #-16\n");
}

// W-register CBZ/CBNZ
#[test]
fn rt_cbz_w() {
    if !native_macho_host("roundtrip", "rt_cbz_w") {
        return;
    }
    roundtrip(".text\ncbz w0, #8\n");
}
#[test]
fn rt_cbnz_w() {
    if !native_macho_host("roundtrip", "rt_cbnz_w") {
        return;
    }
    roundtrip(".text\ncbnz w5, #12\n");
}

// Single-precision FP
#[test]
fn rt_fneg_s() {
    if !native_macho_host("roundtrip", "rt_fneg_s") {
        return;
    }
    roundtrip(".text\nfneg s0, s1\n");
}
#[test]
fn rt_fabs_s() {
    if !native_macho_host("roundtrip", "rt_fabs_s") {
        return;
    }
    roundtrip(".text\nfabs s0, s1\n");
}
#[test]
fn rt_fsqrt_s() {
    if !native_macho_host("roundtrip", "rt_fsqrt_s") {
        return;
    }
    roundtrip(".text\nfsqrt s0, s1\n");
}
#[test]
fn rt_fcmp_s() {
    if !native_macho_host("roundtrip", "rt_fcmp_s") {
        return;
    }
    roundtrip(".text\nfcmp s0, s1\n");
}
#[test]
fn rt_fmov_imm_s() {
    if !native_macho_host("roundtrip", "rt_fmov_imm_s") {
        return;
    }
    roundtrip(".text\nfmov s2, #3.50000000\n");
}
#[test]
fn rt_fcsel_s() {
    if !native_macho_host("roundtrip", "rt_fcsel_s") {
        return;
    }
    roundtrip(".text\nfcsel s0, s0, s1, mi\n");
}
#[test]
fn rt_fmadd_s() {
    if !native_macho_host("roundtrip", "rt_fmadd_s") {
        return;
    }
    roundtrip(".text\nfmadd s0, s1, s2, s3\n");
}

// LDP/STP missing variants
#[test]
fn rt_stp_post_32() {
    if !native_macho_host("roundtrip", "rt_stp_post_32") {
        return;
    }
    roundtrip(".text\nstp x19, x20, [sp], #32\n");
}
#[test]
fn rt_ldp_pre_m32() {
    if !native_macho_host("roundtrip", "rt_ldp_pre_m32") {
        return;
    }
    roundtrip(".text\nldp x19, x20, [sp, #-32]!\n");
}

// All condition codes through round-trip
#[test]
fn rt_b_ne_neg() {
    if !native_macho_host("roundtrip", "rt_b_ne_neg") {
        return;
    }
    roundtrip(".text\nb.ne #-4\n");
}
#[test]
fn rt_b_cs() {
    if !native_macho_host("roundtrip", "rt_b_cs") {
        return;
    }
    roundtrip(".text\nb.cs #8\n");
}
#[test]
fn rt_b_mi() {
    if !native_macho_host("roundtrip", "rt_b_mi") {
        return;
    }
    roundtrip(".text\nb.mi #12\n");
}
#[test]
fn rt_b_hi() {
    if !native_macho_host("roundtrip", "rt_b_hi") {
        return;
    }
    roundtrip(".text\nb.hi #16\n");
}
#[test]
fn rt_b_gt_20() {
    if !native_macho_host("roundtrip", "rt_b_gt_20") {
        return;
    }
    roundtrip(".text\nb.gt #20\n");
}

// ---- Multi-instruction round-trips ----

#[test]
fn rt_function_prologue() {
    if !native_macho_host("roundtrip", "rt_function_prologue") {
        return;
    }
    roundtrip(
        ".text\n\
stp x29, x30, [sp, #-16]!\n\
mov x29, sp\n\
",
    );
}

#[test]
fn rt_function_epilogue() {
    if !native_macho_host("roundtrip", "rt_function_epilogue") {
        return;
    }
    roundtrip(
        ".text\n\
ldp x29, x30, [sp], #16\n\
ret\n\
",
    );
}

#[test]
fn rt_arithmetic_sequence() {
    if !native_macho_host("roundtrip", "rt_arithmetic_sequence") {
        return;
    }
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
    if !native_macho_host("roundtrip", "rt_branch_sequence") {
        return;
    }
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
    if !native_macho_host("roundtrip", "rt_fp_sequence") {
        return;
    }
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
    if !native_macho_host("roundtrip", "rt_clang_output") {
        return;
    }
    // Generate a real .s file from clang and parse it.
    let c_src = "int square(int x) { return x * x; }\n";
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir();
    let c_path = dir.join(format!("afs_rt_clang_{}_{}.c", pid, id));
    let s_path = dir.join(format!("afs_rt_clang_{}_{}.s", pid, id));

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
