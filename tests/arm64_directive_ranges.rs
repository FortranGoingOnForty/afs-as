use afs_as::assemble::assemble_source;
use afs_as::parse::{parse, Directive, Stmt};

#[test]
fn directive_counts_and_alignments_reject_wrapping_values() {
    for (source, col, context, value, target) in [
        (".comm _x,-1", 10, "common size expression", -1_i64, "u64"),
        (
            ".comm _x,1,256",
            12,
            "common alignment expression",
            256,
            "u8",
        ),
        (".space -1", 8, "space expression", -1, "u64"),
        (".skip -1", 7, "space expression", -1, "u64"),
        (".zero -1", 7, "zero expression", -1, "u64"),
        (".fill -1,1,0", 7, "fill repeat expression", -1, "u64"),
        (".fill 1,256,0", 9, "fill size expression", 256, "u8"),
        (
            ".zerofill __DATA,__bss,_x,-1,0",
            27,
            "zerofill size expression",
            -1,
            "u64",
        ),
        (
            ".zerofill __DATA,__bss,_x,8,4294967296",
            29,
            "zerofill alignment expression",
            4_294_967_296_i64,
            "u32",
        ),
        (".tbss _x,-1,2", 10, "tbss size expression", -1, "u64"),
        (
            ".tbss _x,8,4294967296",
            12,
            "tbss alignment expression",
            4_294_967_296_i64,
            "u32",
        ),
        (
            ".align 4294967296",
            8,
            "alignment expression",
            4_294_967_296_i64,
            "u32",
        ),
        (
            ".p2align 2,0,-1",
            14,
            "alignment max-skip expression",
            -1,
            "u64",
        ),
    ] {
        let error = parse(source).expect_err("wrapping directive value unexpectedly parsed");
        assert_eq!((error.line, error.col), (1, col), "source: {source}");
        assert_eq!(
            error.msg,
            format!("{context} value {value} does not fit in {target}"),
            "source: {source}"
        );
    }
}

#[test]
fn directive_patterns_use_explicit_apple_truncation_semantics() {
    assert_eq!(
        parse(".align 2,-1\n.p2align 2,256").unwrap(),
        vec![
            Stmt::Directive(Directive::Align {
                power: 2,
                fill: Some(0xff),
                max_skip: None,
            }),
            Stmt::Directive(Directive::P2Align {
                power: 2,
                fill: Some(0),
                max_skip: None,
            }),
        ]
    );

    for (value, expected) in [
        ("-1", vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]),
        (
            "0x1122334455667788",
            vec![0x88, 0x77, 0x66, 0x55, 0, 0, 0, 0],
        ),
    ] {
        let object = assemble_source(&format!(".data\n.fill 1,8,{value}"))
            .expect("valid fill pattern unexpectedly rejected");
        assert_eq!(
            object.section("__DATA", "__data").unwrap().data,
            expected,
            "fill value: {value}"
        );
    }

    let apple_pattern = [0x88, 0x77, 0x66, 0x55, 0, 0, 0, 0];
    for size in 1..=8 {
        let object = assemble_source(&format!(".data\n.fill 1,{size},0x1122334455667788"))
            .expect("valid fill width unexpectedly rejected");
        assert_eq!(
            object.section("__DATA", "__data").unwrap().data,
            &apple_pattern[..size],
            "fill size: {size}"
        );
    }
}
