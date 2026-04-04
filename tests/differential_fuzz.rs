#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fmt::Write as _;
use std::fs;
use std::panic::{self, AssertUnwindSafe};

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0xA076_1D64_78BD_642F)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as u32
    }

    fn bounded(&mut self, upper: u32) -> u32 {
        self.next_u32() % upper
    }

    fn choose<'a>(&mut self, values: &'a [&'a str]) -> &'a str {
        values[self.bounded(values.len() as u32) as usize]
    }
}

fn style_mnemonic(rng: &mut Rng, mnemonic: &str) -> String {
    match rng.bounded(3) {
        0 => mnemonic.to_string(),
        1 => mnemonic.to_ascii_uppercase(),
        _ => {
            let mut chars = mnemonic.chars();
            let mut out = String::new();
            if let Some(first) = chars.next() {
                out.push(first.to_ascii_uppercase());
            }
            for ch in chars {
                out.push(ch);
            }
            out
        }
    }
}

fn emit_line(src: &mut String, rng: &mut Rng, mnemonic: &str, operands: &str) {
    let indent = if rng.bounded(2) == 0 { "    " } else { "\t" };
    let _ = writeln!(src, "{}{} {}", indent, style_mnemonic(rng, mnemonic), operands);
}

fn generate_supported_case(seed: u64) -> String {
    const CONDITIONS: &[&str] = &["eq", "ne", "gt", "lt", "mi", "pl"];
    const FP_IMMS: &[&str] = &["#1.0", "#2.0", "#-1.0", "#3.5"];

    let mut rng = Rng::new(seed);
    let cond0 = rng.choose(CONDITIONS);
    let cond1 = rng.choose(CONDITIONS);
    let logical_imm = match rng.bounded(4) {
        0 => "#0x7",
        1 => "#0xf",
        2 => "#0x3f",
        _ => "#0xff",
    };
    let add_imm = 1 + rng.bounded(64);
    let sub_imm = 1 + rng.bounded(64);
    let cmp_imm = 1 + rng.bounded(31);
    let bit_index = rng.bounded(32);
    let extend_shift = rng.bounded(3);
    let lsl_shift = rng.bounded(4);
    let ubfiz_lsb = rng.bounded(16);
    let ubfiz_width = 1 + rng.bounded(16 - ubfiz_lsb);
    let cstr_count = 2 + rng.bounded(3) as usize;
    let data_expr_count = 3 + rng.bounded(3) as usize;
    let include_tls = seed.is_multiple_of(2);
    let include_fp = !seed.is_multiple_of(3);
    let include_loh = !seed.is_multiple_of(2);
    let include_numeric_local = !seed.is_multiple_of(5);

    let mut src = String::new();
    let _ = writeln!(src, ".build_version macos, 11, 0 sdk_version 15, 5");
    let _ = writeln!(src, ".subsections_via_symbols");
    let _ = writeln!(src, ".globl _fuzz_{}", seed);
    let _ = writeln!(src, ".text");
    let _ = writeln!(src, ".p2align 2");
    let _ = writeln!(src, "_fuzz_{}:", seed);

    emit_line(&mut src, &mut rng, "mov", "x9, x0");
    emit_line(&mut src, &mut rng, "and", &format!("w8, w0, {}", logical_imm));
    emit_line(
        &mut src,
        &mut rng,
        "ubfiz",
        &format!("w10, w8, #{}, #{}", ubfiz_lsb, ubfiz_width),
    );
    emit_line(&mut src, &mut rng, "msub", "w11, w10, w8, w0");
    emit_line(&mut src, &mut rng, "add", &format!("x9, x9, #{}", add_imm));
    emit_line(&mut src, &mut rng, "sub", &format!("x12, x9, #{}", sub_imm));
    emit_line(
        &mut src,
        &mut rng,
        "add",
        &format!("x13, x12, w11, uxtw #{}", extend_shift),
    );
    emit_line(
        &mut src,
        &mut rng,
        "cmp",
        &format!("x13, x9, lsl #{}", lsl_shift),
    );
    emit_line(&mut src, &mut rng, "csel", &format!("x14, x13, x12, {}", cond0));
    emit_line(&mut src, &mut rng, "csinv", &format!("x15, x14, x13, {}", cond1));
    emit_line(&mut src, &mut rng, "cmp", &format!("w10, #{}", cmp_imm));

    if include_numeric_local {
        let _ = writeln!(src, "1:");
        emit_line(&mut src, &mut rng, "cbz", "w11, 2f");
        emit_line(&mut src, &mut rng, "tbz", &format!("x15, #{}, Lpage_{}", bit_index, seed));
        emit_line(&mut src, &mut rng, "b", &format!("Ldone_{}", seed));
        let _ = writeln!(src, "2:");
    } else {
        emit_line(&mut src, &mut rng, "cbz", &format!("w11, Lpage_{}", seed));
        emit_line(&mut src, &mut rng, "tbz", &format!("x15, #{}, Ldone_{}", bit_index, seed));
    }

    let _ = writeln!(src, "Lpage_{}:", seed);
    if include_loh {
        let _ = writeln!(src, "LlohP0_{}:", seed);
    }
    emit_line(
        &mut src,
        &mut rng,
        "adrp",
        &format!("x16, cstr0_{}@PAGE", seed),
    );
    if include_loh {
        let _ = writeln!(src, "LlohP1_{}:", seed);
    }
    emit_line(
        &mut src,
        &mut rng,
        "add",
        &format!("x16, x16, cstr0_{}@PAGEOFF", seed),
    );
    if include_loh {
        let _ = writeln!(src, "LlohG0_{}:", seed);
    }
    emit_line(
        &mut src,
        &mut rng,
        "adrp",
        &format!("x17, _ext_{}@GOTPAGE", seed),
    );
    if include_loh {
        let _ = writeln!(src, "LlohG1_{}:", seed);
    }
    emit_line(
        &mut src,
        &mut rng,
        "ldr",
        &format!("x17, [x17, _ext_{}@GOTPAGEOFF]", seed),
    );
    emit_line(&mut src, &mut rng, "ldr", "x18, [sp, #16]");
    emit_line(&mut src, &mut rng, "str", "x18, [sp, #8]");
    emit_line(&mut src, &mut rng, "ldp", "x19, x20, [sp]");
    emit_line(&mut src, &mut rng, "stp", "x19, x20, [sp, #-16]!");

    if include_tls {
        emit_line(
            &mut src,
            &mut rng,
            "adrp",
            &format!("x0, _tls_{}@TLVPPAGE", seed),
        );
        emit_line(
            &mut src,
            &mut rng,
            "ldr",
            &format!("x0, [x0, _tls_{}@TLVPPAGEOFF]", seed),
        );
    }

    if include_fp {
        let fp_imm = rng.choose(FP_IMMS).to_string();
        let fp_cond = rng.choose(CONDITIONS).to_string();
        emit_line(&mut src, &mut rng, "fmov", &format!("d0, {}", fp_imm));
        emit_line(&mut src, &mut rng, "ldr", "d1, [sp, #8]");
        emit_line(&mut src, &mut rng, "fcmp", "d1, d0");
        emit_line(&mut src, &mut rng, "fcsel", &format!("d0, d1, d0, {}", fp_cond));
    }

    emit_line(&mut src, &mut rng, "dmb", "ish");
    emit_line(&mut src, &mut rng, "isb", "sy");
    let _ = writeln!(src, "Ldone_{}:", seed);
    emit_line(&mut src, &mut rng, "ret", "");

    if include_loh {
        let _ = writeln!(src, ".loh AdrpAdd LlohP0_{}, LlohP1_{}", seed, seed);
        let _ = writeln!(src, ".loh AdrpLdrGot LlohG0_{}, LlohG1_{}", seed, seed);
    }

    let _ = writeln!(src, ".section __TEXT,__cstring,cstring_literals");
    for index in 0..cstr_count {
        let _ = writeln!(src, "cstr{}_{}:", index, seed);
        let _ = writeln!(
            src,
            "    .asciz \"diff-fuzz-{}-{}-{:08x}\"",
            seed,
            index,
            rng.next_u32()
        );
    }

    let _ = writeln!(src, ".section __TEXT,__const");
    let _ = writeln!(src, ".p2align 3");
    for index in 0..data_expr_count {
        let _ = writeln!(src, "const{}_{}:", index, seed);
        match index % 4 {
            0 => {
                let _ = writeln!(src, "    .quad data0_{}", seed);
            }
            1 => {
                let _ = writeln!(src, "    .quad _ext_{}", seed);
            }
            2 => {
                let _ = writeln!(
                    src,
                    "    .quad _other_{} - _ext_{} + {}",
                    seed,
                    seed,
                    4 * (1 + index as i64)
                );
            }
            _ => {
                let _ = writeln!(src, "    .quad _puts@GOT");
            }
        }
    }

    let _ = writeln!(src, ".data");
    let _ = writeln!(src, ".p2align 3");
    let _ = writeln!(src, "data0_{}:", seed);
    let _ = writeln!(src, "    .quad cstr0_{}", seed);
    let _ = writeln!(src, "    .quad _ext_{}", seed);
    let _ = writeln!(
        src,
        "    .quad _other_{} - _ext_{} + {}",
        seed,
        seed,
        8 + (seed as i64 % 3) * 4
    );

    if include_tls {
        let _ = writeln!(src, ".section __DATA,__thread_data,thread_local_regular");
        let _ = writeln!(src, ".p2align 2");
        let _ = writeln!(src, "_tls_{}$tlv$init:", seed);
        let _ = writeln!(src, "    .long {}", 1 + (seed % 17));
        let _ = writeln!(src, ".section __DATA,__thread_vars,thread_local_variables");
        let _ = writeln!(src, ".globl _tls_{}", seed);
        let _ = writeln!(src, "_tls_{}:", seed);
        let _ = writeln!(src, "    .quad __tlv_bootstrap");
        let _ = writeln!(src, "    .quad 0");
        let _ = writeln!(src, "    .quad _tls_{}$tlv$init", seed);
    }

    let _ = writeln!(
        src,
        ".zerofill __DATA,__bss,_scratch_{},{},4",
        seed,
        16 * (1 + seed % 4)
    );
    src
}

fn generate_garbage_case(seed: u64) -> String {
    const TOKENS: &[&str] = &[
        ".", ",", ":", "#", "@", "@@PAGE", "\"", "'",
        "text", "ldr", "q99", "0xZZ", "(", ")", "[", "]",
        "Lx", ".unknown", ".cfi_bogus", "??", "!", "=", "-",
    ];

    let mut rng = Rng::new(seed ^ 0xE703_7ED1_A0B4_28DB);
    let mut src = String::new();
    let line_count = 1 + rng.bounded(24);
    for _ in 0..line_count {
        let token_count = 1 + rng.bounded(8);
        if rng.bounded(6) == 0 {
            let _ = writeln!(src);
            continue;
        }
        for index in 0..token_count {
            if index != 0 {
                src.push(if rng.bounded(2) == 0 { ' ' } else { '\t' });
            }
            src.push_str(rng.choose(TOKENS));
            if rng.bounded(5) == 0 {
                src.push_str(rng.choose(&["0", "1", "9", "42", "65536"]));
            }
        }
        let _ = writeln!(src);
    }
    src
}

#[test]
fn differential_seeded_surface_matches_system_as() {
    for seed in 1..=24u64 {
        let src = generate_supported_case(seed);
        let paths = common::TempPaths::new(&format!("afs_diff_fuzz_{}", seed));
        fs::write(&paths.asm, &src).expect("write differential fuzz assembly");
        common::assemble_with_ours(&src, &paths.obj);
        common::assemble_with_system(&paths.asm, &paths.ref_obj);
        let ours = fs::read(&paths.obj).expect("read afs-as object");
        let reference = fs::read(&paths.ref_obj).expect("read system object");
        assert_eq!(
            ours, reference,
            "raw object mismatch for differential fuzz seed {}\n---source---\n{}",
            seed, src
        );
    }
}

#[test]
fn differential_garbage_cases_do_not_panic() {
    for seed in 1..=256u64 {
        let src = generate_garbage_case(seed);
        let result = panic::catch_unwind(AssertUnwindSafe(|| afs_as::assemble::assemble_source(&src)));
        assert!(result.is_ok(), "panic for garbage seed {}\n---source---\n{}", seed, src);
    }
}
