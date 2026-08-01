//! String-literal escapes must decode like gas. Earlier parser bugs
//! included one-digit octal decoding and missing `\x` hex support.

use afs_as::x86::assemble::assemble_x86;

/// Bytes of the `.data` section after assembling a single `.ascii` line.
fn data_of(literal: &str) -> Vec<u8> {
    let src = format!(".data\n.ascii \"{literal}\"\n");
    data_of_source(&src)
}

fn data_of_source(src: &str) -> Vec<u8> {
    let obj = assemble_x86(src, 0).unwrap_or_else(|e| panic!("{src:?}: {}", e.msg));
    obj.sections
        .iter()
        .find(|s| s.name == ".data")
        .map(|s| s.data.clone())
        .unwrap_or_default()
}

#[test]
fn string_directives_encode_each_operand_independently() {
    let data = data_of_source(
        ".data\n\
         .ascii \"A\",\"B,C\"\n\
         .asciz \"D\",\"E\"\n\
         .string \"F\",\"G\"\n",
    );

    assert_eq!(data, b"AB,CD\0E\0F\0G\0");
}

#[test]
fn string_operand_groups_preserve_gnu_terminator_boundaries() {
    let data = data_of_source(
        ".data\n\
         .ascii\n\
         .ascii ,\"A\" \"B\",,\"C\",\n\
         .asciz\n\
         .asciz ,\"D\" \"E\",,\"\",\n\
         .string \"F\" \"G\",\"H\"\n",
    );

    assert_eq!(data, b"ABCDE\0\0FG\0H\0");
}

#[test]
fn string_directives_reject_non_string_operands() {
    let error = assemble_x86(".data\n.ascii \"A\", 1\n", 0)
        .expect_err("numeric string operands must be rejected");

    assert_eq!(error.line, Some(2));
    assert_eq!(error.col, Some(1));
    assert_eq!(error.msg, "expected string literal, got '1'");
}

#[test]
fn octal_escapes_read_up_to_three_digits() {
    assert_eq!(data_of("\\012"), vec![0x0a]); // newline, not \0 then "12"
    assert_eq!(data_of("\\101\\102"), b"AB"); // 0o101='A', 0o102='B'
    assert_eq!(data_of("abc\\012"), b"abc\n");
    assert_eq!(data_of("\\0"), vec![0]); // one-digit octal still works
    assert_eq!(data_of("a\\0b"), vec![b'a', 0, b'b']);
    assert_eq!(data_of("\\777"), vec![0xff]); // 0o777 & 0xff
}

#[test]
fn hex_escapes_are_supported() {
    assert_eq!(data_of("\\x41"), b"A");
    assert_eq!(data_of("\\x0a"), vec![0x0a]);
    assert_eq!(data_of("\\xff"), vec![0xff]);
}

#[test]
fn named_escapes_match_gas() {
    assert_eq!(data_of("\\n\\t\\r"), vec![0x0a, 0x09, 0x0d]);
    assert_eq!(data_of("\\f\\b\\v"), vec![0x0c, 0x08, 0x0b]);
    assert_eq!(data_of("\\\\\\\""), vec![b'\\', b'"']);
}

#[test]
fn alert_escape_matches_gas_literal_a() {
    assert_eq!(data_of("x\\ay"), b"xay");
}
