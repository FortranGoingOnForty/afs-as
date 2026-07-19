use afs_as::encode::Inst;
use afs_as::parse::{parse, Stmt};

fn parse_inst(source: &str) -> Inst {
    parse(source)
        .unwrap_or_else(|error| panic!("source: {source}; error: {error}"))
        .into_iter()
        .find_map(|stmt| match stmt {
            Stmt::Instruction(inst) => Some(inst),
            _ => None,
        })
        .unwrap_or_else(|| panic!("source did not produce an instruction: {source}"))
}

fn assert_encoding(source: &str, expected: u32) {
    assert_eq!(parse_inst(source).encode(), expected, "source: {source}");
}

fn assert_rejected(source: &str, expected: &str) {
    let error = match parse(source) {
        Ok(_) => panic!("source unexpectedly parsed: {source}"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains(expected),
        "source: {source}; expected {expected:?}; error: {error}"
    );
}

#[test]
fn gp_instruction_families_reject_mismatched_register_widths() {
    for source in [
        "add x0, w1, w2",
        "add x0, w1, #1",
        "sub w0, x1, w2",
        "cmp x0, w1",
        "cmn w0, x1",
        "neg x0, w1",
        "and x0, w1, #1",
        "orr x0, x1, w2",
        "eor w0, w1, x2",
        "ands x0, w1, x2",
        "tst x0, w1",
        "mvn x0, w1",
        "csel x0, w1, x2, eq",
        "csinc w0, w1, x2, ne",
        "csinv x0, x1, w2, lt",
        "csneg w0, x1, w2, ge",
        "cinc x0, w1, eq",
        "cinv w0, x1, ne",
        "cneg x0, w1, lt",
        "mul x0, w1, w2",
        "sdiv w0, x1, w2",
        "udiv x0, x1, w2",
        "madd x0, x1, w2, x3",
        "msub w0, w1, w2, x3",
        "mov x0, w1",
        "mov wsp, x0",
        "mov x0, wsp",
        "ubfiz x0, w1, #0, #1",
        "bfi w0, x1, #0, #1",
        "bfxil x0, w1, #0, #1",
    ] {
        assert_rejected(source, "width");
    }
}

#[test]
fn stack_pointer_move_aliases_preserve_register_width() {
    for (source, expected) in [
        ("mov wsp, w0", 0x1100_001F),
        ("mov w1, wsp", 0x1100_03E1),
        ("mov wsp, wsp", 0x1100_03FF),
        ("mov sp, x0", 0x9100_001F),
        ("mov x1, sp", 0x9100_03E1),
        ("mov sp, sp", 0x9100_03FF),
    ] {
        assert_encoding(source, expected);
    }
}

#[test]
fn stack_pointer_move_aliases_reject_unrepresentable_operands() {
    for source in [
        "mov wsp, #0",
        "mov sp, #0",
        "mov wzr, wsp",
        "mov wsp, wzr",
        "mov xzr, sp",
        "mov sp, xzr",
    ] {
        assert_rejected(source, "mov");
    }
}

#[test]
fn numeric_zero_register_aliases_match_canonical_spellings() {
    for (alias, canonical) in [
        ("and x0, x1, x31", "and x0, x1, xzr"),
        ("and w0, w1, w31", "and w0, w1, wzr"),
        ("br x31", "br xzr"),
        ("ldr x31, [x0]", "ldr xzr, [x0]"),
        ("fmov d0, x31", "fmov d0, xzr"),
        ("fcvtzs x31, d0", "fcvtzs xzr, d0"),
        ("mov.s w31, v0[0]", "mov.s wzr, v0[0]"),
        ("add x0, sp, x31", "add x0, sp, xzr"),
        ("lsl w0, w1, w31", "lsl w0, w1, wzr"),
        ("stlr w31, [x0]", "stlr wzr, [x0]"),
        ("ldr x0, [x1, x31]", "ldr x0, [x1, xzr]"),
        ("ldp x31, x0, [x1]", "ldp xzr, x0, [x1]"),
    ] {
        assert_eq!(
            parse_inst(alias).encode(),
            parse_inst(canonical).encode(),
            "alias: {alias}; canonical: {canonical}"
        );
    }
}

#[test]
fn stack_pointer_shift_operands_cannot_be_shadowed_as_constants() {
    for mnemonic in ["lsl", "lsr", "asr"] {
        for prefix in ["", ".set wsp, 1\n"] {
            let source = format!("{prefix}{mnemonic} w0, w1, wsp");
            assert_rejected(&source, "does not allow SP");
        }
    }
}

#[test]
fn scalar_fp_instruction_families_reject_mismatched_register_widths() {
    for source in [
        "fadd d0, s1, s2",
        "fsub s0, d1, s2",
        "fmul d0, d1, s2",
        "fdiv s0, s1, d2",
        "fneg d0, s1",
        "fabs s0, d1",
        "fsqrt d0, s1",
        "fcmp d0, s1",
        "fcsel d0, s1, d2, eq",
        "fmadd s0, s1, d2, s3",
    ] {
        assert_rejected(source, "width");
    }
}

#[test]
fn architecturally_mixed_width_forms_remain_valid() {
    for (source, expected) in [
        ("add x0, x1, w2, uxtw", 0x8B22_4020),
        ("sub x0, x1, w2, sxtw", 0xCB22_C020),
        ("ldr x0, [x1, w2, uxtw #3]", 0xF862_5820),
        ("fcvt d0, s1", 0x1E22_C020),
        ("fmov d0, x1", 0x9E67_0020),
        ("fmov w0, s1", 0x1E26_0020),
    ] {
        assert_encoding(source, expected);
    }
}
