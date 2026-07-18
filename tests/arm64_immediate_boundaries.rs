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
