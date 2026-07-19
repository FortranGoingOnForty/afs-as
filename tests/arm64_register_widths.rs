use afs_as::assemble::assemble_source;
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

fn assert_assembly_rejected(source: &str, expected: &str) {
    let error = assemble_source(source)
        .expect_err("source unexpectedly assembled")
        .to_string();
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
fn register_shaped_absolute_symbols_remain_immediate_operands() {
    for (value, source, canonical) in [
        (1, "add x0, x1, x32", "add x0, x1, #1"),
        (1, "sub w0, w1, x32", "sub w0, w1, #1"),
        (8, "ldr x0, [x1, x32]", "ldr x0, [x1, #8]"),
        (4, "str w0, [x1, x32]", "str w0, [x1, #4]"),
        (8, "ldr d0, [x1, x32]", "ldr d0, [x1, #8]"),
        (16, "str q0, [x1, x32]", "str q0, [x1, #16]"),
        (1, "ldrb w0, [x1, x32]", "ldrb w0, [x1, #1]"),
        (1, "strb w0, [x1, x32]", "strb w0, [x1, #1]"),
        (2, "ldrh w0, [x1, x32]", "ldrh w0, [x1, #2]"),
        (2, "strh w0, [x1, x32]", "strh w0, [x1, #2]"),
        (1, "ldrsb x0, [x1, x32]", "ldrsb x0, [x1, #1]"),
        (2, "ldrsh w0, [x1, x32]", "ldrsh w0, [x1, #2]"),
        (4, "ldrsw x0, [x1, x32]", "ldrsw x0, [x1, #4]"),
    ] {
        let with_symbol = format!(".set x32, {value}\n{source}");
        assert_eq!(
            parse_inst(&with_symbol).encode(),
            parse_inst(canonical).encode(),
            "source: {source}; canonical: {canonical}"
        );
    }
}

#[test]
fn leading_zero_register_shaped_symbols_remain_immediate_operands() {
    for (symbol, value, source, canonical) in [
        ("x01", 1, "add x0, x1, x01", "add x0, x1, #1"),
        ("w01", 1, "sub w0, w1, w01", "sub w0, w1, #1"),
        ("x01", 8, "ldr x0, [x1, x01]", "ldr x0, [x1, #8]"),
        ("w01", 4, "str w0, [x1, w01]", "str w0, [x1, #4]"),
        ("d01", 1, "add x0, x1, d01", "add x0, x1, #1"),
        ("v01", 1, "sub x0, x1, v01", "sub x0, x1, #1"),
    ] {
        let with_symbol = format!(".set {symbol}, {value}\n{source}");
        assert_eq!(
            parse_inst(&with_symbol).encode(),
            parse_inst(canonical).encode(),
            "source: {source}; canonical: {canonical}"
        );
    }
}

#[test]
fn leading_zero_register_spellings_are_rejected() {
    for source in [
        "add x0, x01, x2",
        "sub w0, w1, w01",
        "fadd d01, d1, d2",
        "fadd s0, s01, s2",
        "ldr q01, [x0]",
        "ld1.s { v01 }[0], [x0]",
    ] {
        assert_rejected(source, "register");
    }
}

#[test]
fn extended_add_sub_sources_follow_instruction_width() {
    let operations = [
        ("add w0, w1", "add x0, x1", false),
        ("sub w0, w1", "sub x0, x1", false),
        ("adds w0, w1", "adds x0, x1", true),
        ("subs w0, w1", "subs x0, x1", true),
        ("cmp w1", "cmp x1", true),
        ("cmn w1", "cmn x1", true),
    ];
    let extensions = [
        ("uxtb", false),
        ("uxth", false),
        ("uxtw", false),
        ("uxtx", true),
        ("sxtb", false),
        ("sxth", false),
        ("sxtw", false),
        ("sxtx", true),
    ];

    for (op32, op64, sets_flags) in operations {
        for (extension, uses_x_in_64_bit_form) in extensions {
            parse_inst(&format!("{op32}, w2, {extension}"));
            assert_rejected(&format!("{op32}, x2, {extension}"), "register operand");

            if uses_x_in_64_bit_form && sets_flags {
                let with_w = parse_inst(&format!("{op64}, w2, {extension}"));
                let with_x = parse_inst(&format!("{op64}, x2, {extension}"));
                assert_eq!(with_w.encode(), with_x.encode());
            } else {
                let (valid64, invalid64) = if uses_x_in_64_bit_form {
                    ("x2", "w2")
                } else {
                    ("w2", "x2")
                };
                parse_inst(&format!("{op64}, {valid64}, {extension}"));
                assert_rejected(
                    &format!("{op64}, {invalid64}, {extension}"),
                    "register operand",
                );
            }
        }
    }
}

#[test]
fn canonical_register_names_cannot_be_shadowed_as_branch_offsets() {
    for register in [
        "x0", "w0", "sp", "wsp", "xzr", "wzr", "x31", "w31", "b0", "h0", "s0", "d0", "q0", "v0",
    ] {
        let source = format!(".set {register}, 4\nb {register}");
        assert_assembly_rejected(&source, "architectural register");
    }
}

#[test]
fn canonical_register_labels_are_not_branch_operands() {
    for register in ["x0", "w0", "d0", "s0", "q0", "v0"] {
        let source = format!("{register}:\nnop\nb {register}");
        assert_assembly_rejected(&source, "architectural register");
    }
}

#[test]
fn noncanonical_register_shaped_symbols_remain_branch_offsets() {
    let expected = assemble_source("b #4").unwrap().text_section().data.clone();
    for symbol in ["x01", "w01", "d01", "v01"] {
        let source = format!(".set {symbol}, 4\nb {symbol}");
        assert_eq!(
            assemble_source(&source).unwrap().text_section().data,
            expected,
            "source: {source}"
        );

        let source = format!("{symbol}:\nnop\nb {symbol}");
        assemble_source(&source).unwrap();
    }
}

#[test]
fn canonical_register_names_remain_labels_in_address_generation_operands() {
    for register in [
        "x0", "w0", "sp", "wsp", "xzr", "wzr", "x31", "w31", "b0", "h0", "s0", "d0", "q0", "v0",
    ] {
        assemble_source(&format!("{register}:\nnop\nadr x8, {register}"))
            .expect("canonical register name should remain an ADR label");
        assemble_source(&format!("{register}:\nnop\nadrp x8, {register}@PAGE"))
            .expect("canonical register name should remain an ADRP label");
    }
}

#[test]
fn noncanonical_gp_register_shaped_symbols_remain_relocation_operands() {
    for source in [
        "ldr x0, x01\nnop\nx01:\nnop",
        "x32:\nnop\nldr x0, x32",
        "add x0, x0, x01@PAGEOFF",
        "x32:\nnop\nadd x0, x0, x32@PAGEOFF",
        "ldr x0, [x0, x01@PAGEOFF]",
        "ldr x0, [x0, w01@PAGEOFF]",
    ] {
        assemble_source(source).unwrap_or_else(|error| {
            panic!("noncanonical register-shaped symbol was rejected: {source}: {error}")
        });
    }
}

#[test]
fn architectural_register_names_take_priority_over_absolute_symbols() {
    for source in [
        ".set sp, 1\nadd x0, x1, sp",
        ".set wsp, 1\nsub w0, w1, wsp",
        ".set sp, 8\nldr x0, [x1, sp]",
        ".set wsp, 8\nstr w0, [x1, wsp]",
    ] {
        assert_rejected(source, "allow");
    }

    for (source, canonical) in [
        (".set xzr, 1\nand x0, x1, xzr", "and x0, x1, xzr"),
        (".set x31, 1\nand x0, x1, x31", "and x0, x1, xzr"),
        (".set wzr, 1\nand w0, w1, wzr", "and w0, w1, wzr"),
        (".set w31, 1\nand w0, w1, w31", "and w0, w1, wzr"),
    ] {
        assert_eq!(
            parse_inst(source).encode(),
            parse_inst(canonical).encode(),
            "source: {source}; canonical: {canonical}"
        );
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
        "fmsub d0, d1, d2, s3",
        "fnmsub s0, s1, d2, s3",
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
