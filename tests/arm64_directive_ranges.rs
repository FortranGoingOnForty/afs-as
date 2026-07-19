use afs_as::assemble::{assemble_source, assemble_stmts};
use afs_as::expr::Expr;
use afs_as::macho::ARM64_RELOC_UNSIGNED;
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
        (".fill 1,-1,0", 9, "fill size expression", -1, "u64"),
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
        parse(".align 2,-1\n.p2align 2,256\n.align 2,0xffffffffffffffff").unwrap(),
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
            Stmt::Directive(Directive::Align {
                power: 2,
                fill: Some(0xff),
                max_skip: None,
            }),
        ]
    );

    for (value, expected) in [
        ("-1", vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0]),
        (
            "0xffffffffffffffff",
            vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0],
        ),
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

    for size in ["9", "256", "0xffffffffffffffff"] {
        let object = assemble_source(&format!(".data\n.fill 1,{size},0x1122334455667788"))
            .expect("oversized Apple fill width unexpectedly rejected");
        assert_eq!(
            object.section("__DATA", "__data").unwrap().data,
            apple_pattern,
            "fill size: {size}"
        );
    }
}

#[test]
fn narrow_data_operands_validate_signed_unsigned_and_bit_pattern_ranges() {
    for (source, col, context, value, bits) in [
        (".byte -129", 7, ".byte expression", "-129", 8),
        (".byte 1, 256", 10, ".byte expression", "256", 8),
        (
            ".byte 0xffffffffffffff7f",
            7,
            ".byte expression",
            "18446744073709551487",
            8,
        ),
        (".short -32769", 8, ".short expression", "-32769", 16),
        (".short 65536", 8, ".short expression", "65536", 16),
        (
            ".short 0xffffffffffff7fff",
            8,
            ".short expression",
            "18446744073709518847",
            16,
        ),
        (
            ".word -2147483649",
            7,
            ".word expression",
            "-2147483649",
            32,
        ),
        (".word 4294967296", 7, ".word expression", "4294967296", 32),
        (
            ".word 0xffffffff7fffffff",
            7,
            ".word expression",
            "18446744071562067967",
            32,
        ),
    ] {
        let error = parse(source).expect_err("out-of-range data value unexpectedly parsed");
        assert_eq!((error.line, error.col), (1, col), "source: {source}");
        assert_eq!(
            error.msg,
            format!("{context} value {value} is out of range for {bits}-bit data"),
            "source: {source}"
        );
    }

    let object = assemble_source(
        ".data\n\
         .byte -128,255,0xffffffffffffffff\n\
         .short -32768,65535,0xffffffffffffffff\n\
         .word -2147483648,4294967295,0xffffffffffffffff\n",
    )
    .expect("boundary data values unexpectedly rejected");
    assert_eq!(
        object.section("__DATA", "__data").unwrap().data,
        [
            0x80, 0xff, 0xff, 0x00, 0x80, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x80, 0xff,
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        ]
    );
}

#[test]
fn aggregate_zerofill_alignment_reports_overflow() {
    let error = assemble_source(
        ".zerofill __DATA,__bss,_a,9223372036854775807,0\n\
         .zerofill __DATA,__bss,_b,9223372036854775807,0\n\
         .zerofill __DATA,__bss,_c,1,2\n",
    )
    .expect_err("aggregate zerofill overflow unexpectedly assembled");
    assert_eq!((error.line, error.col), (Some(3), Some(1)));
    assert_eq!(error.msg, ".zerofill alignment overflows u64");
}

#[test]
fn cross_section_zerofill_overflow_reports_the_triggering_directive() {
    let error = assemble_source(
        ".zerofill __DATA,__bss,_a,9223372036854775807,0\n\
         .zerofill __DATA,__bss,_b,9223372036854775807,0\n\
         .zerofill __DATA,__thread_bss,_c,1,2\n",
    )
    .expect_err("cross-section zerofill overflow unexpectedly assembled");
    assert_eq!((error.line, error.col), (Some(3), Some(1)));
    assert_eq!(error.msg, "section layout alignment overflows u64");
}

#[test]
fn generated_unwind_layout_overflow_reports_the_triggering_directive() {
    let error = assemble_source(
        ".text\n\
         _f:\n\
         .cfi_startproc\n\
         ret\n\
         .cfi_endproc\n\
         .zerofill __DATA,__bss,_a,9223372036854775807,0\n\
         .zerofill __DATA,__bss,_b,9223372036854775803,0\n",
    )
    .expect_err("unwind-plus-zerofill layout overflow unexpectedly assembled");
    assert_eq!((error.line, error.col), (Some(7), Some(1)));
    assert_eq!(error.msg, "section layout size overflows u64");
}

#[test]
fn large_unwind_layout_overflow_reports_the_triggering_directive() {
    let mut source = String::from(".text\n");
    for index in 0..4096 {
        source.push_str(&format!("_f{index}:\n.cfi_startproc\nret\n.cfi_endproc\n"));
    }
    let trigger_line = source.lines().count() as u32 + 2;
    source.push_str(".zerofill __DATA,__bss,_a,9223372036854775807,0\n");
    source.push_str(".zerofill __DATA,__bss,_b,9223372036854775803,0\n");

    let error = assemble_source(&source).expect_err("large unwind layout unexpectedly assembled");
    assert_eq!((error.line, error.col), (Some(trigger_line), Some(1)));
    assert_eq!(error.msg, "section layout size overflows u64");
}

#[test]
fn preparsed_directives_enforce_the_same_range_contract() {
    let error = assemble_stmts(&[
        Stmt::Directive(Directive::Data),
        Stmt::Directive(Directive::Byte(vec![Expr::Int(256)])),
    ])
    .expect_err("out-of-range preparsed byte unexpectedly assembled");
    assert_eq!(
        error.msg,
        ".byte expression value 256 is out of range for 8-bit data"
    );

    let object = assemble_stmts(&[
        Stmt::Directive(Directive::Data),
        Stmt::Directive(Directive::Fill {
            repeat: 1,
            size: 9,
            value: 0x11223344,
        }),
    ])
    .expect("preparsed Apple fill width unexpectedly rejected");
    assert_eq!(
        object.section("__DATA", "__data").unwrap().data,
        [0x44, 0x33, 0x22, 0x11, 0, 0, 0, 0]
    );

    for value in [0x1122334455667788, u64::MAX] {
        let object = assemble_stmts(&[
            Stmt::Directive(Directive::Data),
            Stmt::Directive(Directive::Fill {
                repeat: 1,
                size: 8,
                value,
            }),
        ])
        .expect("preparsed Apple fill pattern unexpectedly rejected");
        assert_eq!(
            object.section("__DATA", "__data").unwrap().data,
            (value as u32)
                .to_le_bytes()
                .into_iter()
                .chain([0; 4])
                .collect::<Vec<_>>(),
            "fill value: {value:#x}"
        );
    }
}

#[test]
fn reassigned_absolute_symbols_use_the_value_visible_at_each_data_directive() {
    let source_error = assemble_source(".set X,256\n.byte X\n.set X,1\n")
        .expect_err("out-of-range active assignment unexpectedly assembled");
    assert_eq!((source_error.line, source_error.col), (Some(2), Some(1)));
    assert_eq!(
        source_error.msg,
        ".byte expression value 256 is out of range for 8-bit data"
    );

    let direct_error = assemble_stmts(&[
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(256))),
        Stmt::Directive(Directive::Byte(vec![Expr::Symbol("X".into())])),
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(1))),
    ])
    .expect_err("preparsed active assignment unexpectedly used its final value");
    assert_eq!(direct_error.msg, source_error.msg);

    let source = assemble_source(".set X,1\n.byte X\n.set X,2\n")
        .expect("resolved assignment unexpectedly followed a later reassignment");
    assert_eq!(source.text_section().data, [1]);

    let direct = assemble_stmts(&[
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(1))),
        Stmt::Directive(Directive::Byte(vec![Expr::Symbol("X".into())])),
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(2))),
    ])
    .expect("preparsed resolved assignment unexpectedly followed a later reassignment");
    assert_eq!(direct.text_section().data, [1]);

    let source_forward = assemble_source(".byte X\n.set X,1\n")
        .expect("source forward absolute assignment unexpectedly rejected");
    assert_eq!(source_forward.text_section().data, [1]);

    let direct_forward = assemble_stmts(&[
        Stmt::Directive(Directive::Byte(vec![Expr::Symbol("X".into())])),
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(1))),
    ])
    .expect("preparsed forward absolute assignment unexpectedly rejected");
    assert_eq!(direct_forward.text_section().data, [1]);

    let alias = assemble_source(".set A,B\n.set B,2\n.byte A\n.set B,3\n")
        .expect("resolvable absolute alias unexpectedly followed a later reassignment");
    assert_eq!(alias.text_section().data, [2]);
}

#[test]
fn absolute_symbols_bind_to_the_earliest_visible_assignment() {
    let source = assemble_source(".byte X\n.set X,1\n.set X,2\n.byte X\n")
        .expect("forward assignment timeline unexpectedly rejected");
    assert_eq!(source.text_section().data, [1, 2]);

    let direct = assemble_stmts(&[
        Stmt::Directive(Directive::Byte(vec![Expr::Symbol("X".into())])),
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(1))),
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(2))),
        Stmt::Directive(Directive::Byte(vec![Expr::Symbol("X".into())])),
    ])
    .expect("preparsed forward assignment timeline unexpectedly rejected");
    assert_eq!(direct.text_section().data, source.text_section().data);

    let alias = assemble_source(
        ".set A,B\n\
         .set B,2\n\
         .byte A\n\
         .set B,3\n\
         .byte A\n",
    )
    .expect("absolute alias was not frozen when it first resolved");
    assert_eq!(alias.text_section().data, [2, 2]);
}

#[test]
fn quad_absolute_symbols_use_the_value_visible_at_each_directive() {
    let source = assemble_source(
        ".quad X\n\
         .set X,1\n\
         .quad X\n\
         .set X,2\n\
         .quad X\n\
         .set A,X\n\
         .set X,3\n\
         .quad A\n\
         .quad X\n",
    )
    .expect("source absolute-symbol timeline unexpectedly rejected");

    let direct = assemble_stmts(&[
        Stmt::Directive(Directive::Quad(vec![Expr::Symbol("X".into())])),
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(1))),
        Stmt::Directive(Directive::Quad(vec![Expr::Symbol("X".into())])),
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(2))),
        Stmt::Directive(Directive::Quad(vec![Expr::Symbol("X".into())])),
        Stmt::Directive(Directive::Set("A".into(), Expr::Symbol("X".into()))),
        Stmt::Directive(Directive::Set("X".into(), Expr::Int(3))),
        Stmt::Directive(Directive::Quad(vec![Expr::Symbol("A".into())])),
        Stmt::Directive(Directive::Quad(vec![Expr::Symbol("X".into())])),
    ])
    .expect("preparsed absolute-symbol timeline unexpectedly rejected");

    let expected: Vec<_> = [1_u64, 1, 2, 2, 3]
        .into_iter()
        .flat_map(u64::to_le_bytes)
        .collect();
    assert_eq!(source.text_section().data, expected);
    assert_eq!(direct.text_section().data, source.text_section().data);
}

#[test]
fn full_width_absolute_symbols_preserve_64_bit_patterns() {
    let object = assemble_source(
        ".set ALL_ONES,0xffffffffffffffff\n\
         .equ ALIAS,ALL_ONES\n\
         .set MIN,-9223372036854775808\n\
         .data\n\
         .quad ALL_ONES,ALIAS,MIN\n\
         .quad _external + ALL_ONES\n",
    )
    .expect("full-width absolute symbols unexpectedly rejected");

    let data = object.section("__DATA", "__data").unwrap();
    let expected: Vec<_> = [u64::MAX, u64::MAX, 1_u64 << 63, u64::MAX]
        .into_iter()
        .flat_map(u64::to_le_bytes)
        .collect();
    assert_eq!(data.data, expected);
    assert_eq!(data.relocations.len(), 1);
    assert_eq!(data.relocations[0].reloc_type, ARM64_RELOC_UNSIGNED);

    for (name, value) in [
        ("ALL_ONES", u64::MAX),
        ("ALIAS", u64::MAX),
        ("MIN", 1_u64 << 63),
    ] {
        let symbol = object
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap();
        assert!(symbol.absolute);
        assert_eq!(symbol.value, value, "symbol: {name}");
    }

    let direct = assemble_stmts(&[
        Stmt::Directive(Directive::Set("ALL_ONES".into(), Expr::Unsigned(u64::MAX))),
        Stmt::Directive(Directive::Quad(vec![Expr::Symbol("ALL_ONES".into())])),
    ])
    .expect("preparsed full-width assignment unexpectedly rejected");
    assert_eq!(direct.text_section().data, u64::MAX.to_le_bytes());
    let symbol = direct
        .symbols
        .iter()
        .find(|symbol| symbol.name == "ALL_ONES")
        .unwrap();
    assert!(symbol.absolute);
    assert_eq!(symbol.value, u64::MAX);
}

#[test]
fn full_width_assignments_keep_checked_arithmetic() {
    let source = assemble_source(".set X,--9223372036854775808\n")
        .expect_err("overflowing signed-minimum negation unexpectedly assembled");
    assert_eq!((source.line, source.col), (Some(1), Some(1)));
    assert_eq!(source.msg, "absolute symbol 'X': expression overflows i64");

    let direct = assemble_stmts(&[Stmt::Directive(Directive::Set(
        "X".into(),
        Expr::Add(Box::new(Expr::Unsigned(u64::MAX)), Box::new(Expr::Int(1))),
    ))])
    .expect_err("overflowing preparsed full-width arithmetic unexpectedly assembled");
    assert_eq!(direct.msg, "absolute symbol 'X': expression overflows i64");
}

#[test]
fn forward_absolute_aliases_work_in_parser_resolved_directives() {
    let fill = assemble_source(".set A,B\n.set B,2\n.data\n.fill 1,1,A\n")
        .expect("forward alias in .fill unexpectedly rejected");
    assert_eq!(fill.section("__DATA", "__data").unwrap().data, [2]);

    let space = assemble_source(".set A,B\n.set B,2\n.data\n.space A\n")
        .expect("forward alias in .space unexpectedly rejected");
    assert_eq!(space.section("__DATA", "__data").unwrap().data, [0, 0]);

    let align = assemble_source(".set A,B\n.set B,2\n.data\n.byte 1\n.align A,0xaa\n")
        .expect("forward alias in .align unexpectedly rejected");
    assert_eq!(
        align.section("__DATA", "__data").unwrap().data,
        [1, 0xaa, 0xaa, 0xaa]
    );
}

#[test]
fn absolute_assignment_cycles_report_the_definition() {
    for source in [".set A,B\n.set B,A\n", ".set A,B\n.set B,A\n.space A\n"] {
        let error = assemble_source(source).expect_err("cyclic assignments unexpectedly assembled");
        assert_eq!((error.line, error.col), (Some(1), Some(1)));
        assert_eq!(error.msg, "absolute symbol 'A' has a cyclic definition");
    }

    let direct = assemble_stmts(&[
        Stmt::Directive(Directive::Set("A".into(), Expr::Symbol("B".into()))),
        Stmt::Directive(Directive::Set("B".into(), Expr::Symbol("A".into()))),
    ])
    .expect_err("preparsed cyclic assignments unexpectedly assembled");
    assert_eq!(direct.msg, "absolute symbol 'A' has a cyclic definition");

    for source in [".set X,X\n", ".set X,X+1\n"] {
        let error = assemble_source(source).expect_err("self-reference unexpectedly assembled");
        assert_eq!((error.line, error.col), (Some(1), Some(1)));
        assert_eq!(error.msg, "absolute symbol 'X' has a cyclic definition");
    }

    for expression in [
        Expr::Symbol("X".into()),
        Expr::Add(Box::new(Expr::Symbol("X".into())), Box::new(Expr::Int(1))),
    ] {
        let error = assemble_stmts(&[Stmt::Directive(Directive::Set("X".into(), expression))])
            .expect_err("preparsed self-reference unexpectedly assembled");
        assert_eq!(error.msg, "absolute symbol 'X' has a cyclic definition");
    }

    let increment = assemble_source(".set X,1\n.set X,X+1\n.byte X\n")
        .expect("reassignment unexpectedly treated as self-reference");
    assert_eq!(increment.text_section().data, [2]);
}

#[test]
fn definite_absolute_assignment_errors_report_the_definition() {
    for (source, message) in [
        (
            ".set X,9223372036854775807 + 1\n",
            "absolute symbol 'X': expression overflows i64",
        ),
        (
            ".set X,.\n",
            "absolute symbol 'X': expression is not representable as a pointer-to-GOT relocation",
        ),
        (
            ".set X,target@GOT\n",
            "absolute symbol 'X' must resolve to an absolute value",
        ),
        (
            ".set X,a+b\n",
            "absolute symbol 'X': expression is not representable as an absolute value or relocation",
        ),
        (
            ".set X,target\n",
            "absolute symbol 'X' must resolve to an absolute value",
        ),
    ] {
        let error = assemble_source(source)
            .expect_err("invalid absolute assignment unexpectedly assembled");
        assert_eq!((error.line, error.col), (Some(1), Some(1)));
        assert_eq!(error.msg, message);
    }
}

#[test]
fn dependent_assignment_errors_report_the_originating_definition() {
    let error = assemble_source(
        ".set ALIAS,X\n\
         .set X,9223372036854775807 + 1\n",
    )
    .expect_err("overflowing dependency unexpectedly assembled");
    assert_eq!((error.line, error.col), (Some(2), Some(1)));
    assert_eq!(error.msg, "absolute symbol 'X': expression overflows i64");
}

#[test]
fn label_dependent_absolute_assignments_are_deferred_to_assembly() {
    let object = assemble_source(
        ".set X,end-start\n\
         start:\n\
         .byte 0\n\
         end:\n\
         .byte X\n",
    )
    .expect("same-section label difference unexpectedly rejected during parsing");
    assert_eq!(object.text_section().data, [0, 1]);
}

#[test]
fn large_same_section_differences_cancel_before_signed_conversion() {
    let object = assemble_source(
        ".zerofill __DATA,__bss,prefix,1,0\n\
         .zerofill __DATA,__bss,start,9223372036854775807,0\n\
         .zerofill __DATA,__bss,end,1,0\n\
         .set X,end-start\n",
    )
    .expect("representable high-address label difference unexpectedly rejected");

    let value = object
        .symbols
        .iter()
        .find(|symbol| symbol.name == "X")
        .unwrap();
    assert!(value.absolute);
    assert_eq!(value.value, i64::MAX as u64);

    let start = object
        .symbols
        .iter()
        .find(|symbol| symbol.name == "start")
        .unwrap();
    let end = object
        .symbols
        .iter()
        .find(|symbol| symbol.name == "end")
        .unwrap();
    assert!(!start.absolute);
    assert!(!end.absolute);
    assert_eq!(start.section, end.section);
    assert_eq!(start.value, 1);
    assert_eq!(end.value, 1_u64 << 63);
}

#[test]
fn high_address_relocation_anchor_is_not_term_order_dependent() {
    let object = assemble_source(
        ".zerofill __DATA,__bss,low1,0,0\n\
         .zerofill __DATA,__bss,low2,9223372036854775807,0\n\
         .zerofill __DATA,__bss,pad,1,0\n\
         .zerofill __DATA,__bss,high,0,0\n\
         .data\n\
         .quad low1+high-low2\n\
         .quad high+low1-low2\n",
    )
    .expect("equivalent high-address relocation expressions unexpectedly rejected");

    let high_index = object
        .symbols
        .iter()
        .position(|symbol| symbol.name == "high")
        .unwrap() as u32;
    let data = object.section("__DATA", "__data").unwrap();
    assert_eq!(data.data, [0; 16]);
    assert_eq!(data.relocations.len(), 2);
    assert!(data.relocations.iter().all(|relocation| {
        relocation.reloc_type == ARM64_RELOC_UNSIGNED
            && relocation.symbol_idx == high_index
            && relocation.length == 3
    }));
}

#[test]
fn large_label_difference_overflow_reports_the_originating_assignment() {
    let error = assemble_source(
        ".set ALIAS,X\n\
         .zerofill __DATA,__bss,start,9223372036854775807,0\n\
         .zerofill __DATA,__bss,pad,1,0\n\
         .zerofill __DATA,__bss,end,1,0\n\
         .set X,end-start\n",
    )
    .expect_err("unrepresentable high-address label difference unexpectedly assembled");
    assert_eq!((error.line, error.col), (Some(5), Some(1)));
    assert_eq!(error.msg, "absolute symbol 'X': expression overflows i64");
}
