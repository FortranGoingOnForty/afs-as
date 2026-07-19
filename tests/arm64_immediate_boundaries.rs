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
fn add_sub_immediates_canonicalize_without_field_spill() {
    for (source, expected) in [
        ("add x0, x1, #8192", 0x9140_0820),
        ("add x0, x1, #16773120", 0x917F_FC20),
        ("add x0, x1, #-1", 0xD100_0420),
        ("sub x2, x3, #-2", 0x9100_0862),
        ("cmp x4, #-3", 0xB100_0C9F),
        ("add x0, x1, #4095, lsl #12", 0x917F_FC20),
        ("add x0, x1, #42, lsl #0", 0x9100_A820),
    ] {
        assert_encoding(source, expected);
    }
}

#[test]
fn add_sub_immediates_reject_unencodable_values() {
    for source in [
        "add x0, x1, #4097",
        "add x0, x1, #16777216",
        "sub x0, x1, #-16777216",
        "add x0, x1, #4096, lsl #12",
        "add x0, x1, #1, lsl #1",
    ] {
        assert_rejected(source, "add/sub immediate");
    }
}

#[test]
fn move_wide_immediates_preserve_valid_boundaries() {
    for (source, expected) in [
        ("movz x0, #65535, lsl #48", 0xD2FF_FFE0),
        ("movk w1, #0, lsl #16", 0x72A0_0001),
        ("movn x2, #65535, lsl #32", 0x92DF_FFE2),
    ] {
        assert_encoding(source, expected);
    }
}

#[test]
fn move_wide_immediates_reject_narrowing_and_invalid_shifts() {
    for source in [
        "movz x0, #-1",
        "movk x0, #65536",
        "movn x0, #0, lsl #8",
        "movz x0, #0, lsl #64",
        "movk x0, #0, lsl #256",
        "movz w0, #0, lsl #32",
    ] {
        assert_rejected(source, "mov wide");
    }
}

#[test]
fn bitfield_alias_immediates_preserve_valid_boundaries() {
    for (source, expected) in [
        ("ubfiz x0, x1, #63, #1", 0xD341_0020),
        ("bfi w2, w3, #31, #1", 0x3301_0062),
        ("bfxil x4, x5, #0, #64", 0xB340_FCA4),
    ] {
        assert_encoding(source, expected);
    }
}

#[test]
fn bitfield_alias_immediates_reject_values_before_narrowing() {
    for (source, expected) in [
        ("ubfiz x0, x1, #-1, #1", "lsb -1"),
        ("ubfiz x0, x1, #64, #1", "lsb 64"),
        ("ubfiz x0, x1, #256, #1", "lsb 256"),
        ("bfi w0, w1, #0, #-1", "width must be at least 1"),
        ("bfi w0, w1, #0, #0", "width must be at least 1"),
        ("bfi w0, w1, #0, #256", "width 256"),
        ("bfxil x0, x1, #63, #2", "width 2"),
    ] {
        assert_rejected(source, expected);
    }
}

#[test]
fn w_logical_immediates_preserve_32_bit_source_values() {
    assert_encoding("and w0, w1, #-2", 0x121F_7820);
    assert_encoding("and w0, w1, #4294967294", 0x121F_7820);
}

#[test]
fn w_logical_immediates_reject_truncating_source_values() {
    for source in [
        "and w0, w1, #4294967296",
        "and w0, w1, #8589934590",
        "and w0, w1, #-2147483649",
        "and w0, w1, #-4294967298",
    ] {
        assert_rejected(source, "32-bit logical immediate");
    }
}

#[test]
fn system_immediates_validate_u16_boundaries() {
    for (source, expected) in [
        ("svc #0", 0xD400_0001),
        ("svc #65535", 0xD41F_FFE1),
        ("brk #0", 0xD420_0000),
        ("brk #65535", 0xD43F_FFE0),
    ] {
        assert_encoding(source, expected);
    }

    for source in ["svc #-1", "svc #65536", "brk #-1", "brk #65536"] {
        assert_rejected(source, "immediate");
    }
}

#[test]
fn pc_relative_immediates_preserve_signed_boundaries() {
    for (source, expected) in [
        ("b #-134217728", 0x1600_0000),
        ("b #134217724", 0x15FF_FFFF),
        ("bl #134217724", 0x95FF_FFFF),
        ("b.eq #1048572", 0x547F_FFE0),
        ("cbz x0, #-1048576", 0xB480_0000),
        ("tbz x0, #0, #32764", 0x3603_FFE0),
        ("ldr x1, #-1048576", 0x5880_0001),
        ("ldr d2, #1048572", 0x5C7F_FFE2),
        ("ldrsw x3, #1048572", 0x987F_FFE3),
        ("adr x0, #-1048576", 0x1080_0000),
        ("adr x1, #1048575", 0x707F_FFE1),
        ("adrp x0, #-4294967296", 0x9080_0000),
        ("adrp x1, #4294963200", 0xF07F_FFE1),
    ] {
        assert_encoding(source, expected);
    }
}

#[test]
fn pc_relative_immediates_reject_range_and_alignment_errors() {
    for (source, expected) in [
        ("b #-134217732", "branch offset"),
        ("b #134217728", "branch offset"),
        ("b #2", "aligned"),
        ("bl #4294967296", "branch offset"),
        ("b.eq #-1048580", "conditional branch offset"),
        ("b.eq #1048576", "conditional branch offset"),
        ("cbnz x0, #2", "cbz/cbnz offset"),
        ("tbz x0, #0, #-32772", "tbz/tbnz offset"),
        ("tbnz x0, #0, #32768", "tbz/tbnz offset"),
        ("ldr x0, #1048576", "ldr literal offset"),
        ("ldr s0, #2", "ldr literal offset"),
        ("ldrsw x0, #-1048580", "ldrsw literal offset"),
        ("adr x0, #-1048577", "adr immediate"),
        ("adr x0, #1048576", "adr immediate"),
        ("adrp x0, #-4294971392", "adrp immediate"),
        ("adrp x0, #4294967296", "adrp immediate"),
        ("adrp x0, #1", "aligned"),
    ] {
        assert_rejected(source, expected);
    }
}

#[test]
fn memory_immediates_preserve_addressing_mode_boundaries() {
    for (source, expected) in [
        ("ldur x0, [x1, #-256]", 0xF850_0020),
        ("ldur w2, [x3, #255]", 0xB84F_F062),
        ("ldr x4, [x5, #32760]", 0xF97F_FCA4),
        ("ldr x6, [x7, #255]", 0xF84F_F0E6),
        ("str w8, [x9, #16380]", 0xB93F_FD28),
        ("ldr x10, [x11, #-256]!", 0xF850_0D6A),
        ("str w12, [x13], #255", 0xB80F_F5AC),
        ("ldr q0, [x1, #65520]", 0x3DFF_FC20),
        ("str d2, [x3, #255]", 0xFC0F_F062),
        ("ldr s4, [x5, #-256]!", 0xBC50_0CA4),
        ("str q6, [x7], #255", 0x3C8F_F4E6),
        ("ldrb w0, [x1, #4095]", 0x397F_FC20),
        ("strh w2, [x3, #8190]", 0x793F_FC62),
        ("ldrsb x4, [x5, #4095]", 0x39BF_FCA4),
        ("ldrsh w6, [x7, #8190]", 0x79FF_FCE6),
        ("ldrb w8, [x9, #-256]!", 0x3850_0D28),
        ("ldrsh x10, [x11], #255", 0x788F_F56A),
        ("ldrsw x12, [x13, #16380]", 0xB9BF_FDAC),
    ] {
        assert_encoding(source, expected);
    }
}

#[test]
fn pair_immediates_preserve_signed_scaled_boundaries() {
    for (source, expected) in [
        ("ldp x0, x1, [x2, #-512]", 0xA960_0440),
        ("stp x3, x4, [x5, #504]", 0xA91F_90A3),
        ("ldp w6, w7, [x8, #-256]!", 0x29E0_1D06),
        ("stp w9, w10, [x11], #252", 0x289F_A969),
        ("ldp s0, s1, [x2, #-256]", 0x2D60_0440),
        ("stp d2, d3, [x4, #504]!", 0x6D9F_8C82),
        ("ldp q4, q5, [x6], #1008", 0xACDF_94C4),
        ("ldr x0, [x1, x2, lsl #3]", 0xF862_7820),
    ] {
        assert_encoding(source, expected);
    }
}

#[test]
fn memory_immediates_reject_values_before_narrowing() {
    for (source, expected) in [
        ("ldur x0, [x1, #-257]", "memory offset"),
        ("stur w0, [x1, #256]", "memory offset"),
        ("ldur x0, [x1, #65536]", "memory offset"),
        ("ldr x0, [x1, #32768]", "memory offset"),
        ("ldr x0, [x1, #257]", "memory offset"),
        ("str w0, [x1, #16384]", "memory offset"),
        ("ldr x0, [x1, #256]!", "memory offset"),
        ("str w0, [x1], #-257", "post-index offset"),
        ("ldr q0, [x1, #65536]", "FP/SIMD memory offset"),
        ("str d0, [x1, #257]", "FP/SIMD memory offset"),
        ("ldr s0, [x1, #-257]!", "memory offset"),
        ("str q0, [x1], #256", "post-index offset"),
        ("ldrb w0, [x1, #4096]", "memory offset"),
        ("ldrh w0, [x1, #8192]", "memory offset"),
        ("ldrsh x0, [x1, #3]", "memory offset"),
        ("ldrb w0, [x1, #-257]!", "memory offset"),
        ("ldrsb x0, [x1], #256", "post-index offset"),
        ("ldrsw x0, [x1, #16384]", "memory offset"),
        ("ldrsw x0, [x1, #3]", "memory offset"),
        ("ldp x0, x1, [x2, #-520]", "pair offset"),
        ("stp x0, x1, [x2, #512]", "pair offset"),
        ("ldp w0, w1, [x2, #2]!", "pair offset"),
        ("stp w0, w1, [x2], #256", "pair post-index offset"),
        ("ldp s0, s1, [x2, #-260]", "pair offset"),
        ("stp d0, d1, [x2, #512]!", "pair offset"),
        ("ldp q0, q1, [x2], #1024", "pair post-index offset"),
        ("ldr x0, [x1, x2, lsl #256]", "register offset shift"),
    ] {
        assert_rejected(source, expected);
    }
}
