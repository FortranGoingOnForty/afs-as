use std::panic::{self, AssertUnwindSafe};

use afs_as::assemble;
use afs_as::encode::Inst;
use afs_as::parse::{Directive, Stmt};
use afs_as::reg::{X0, X1, X30};

#[test]
fn assemble_source_reports_source_locations_for_invalid_input() {
    let err = assemble::assemble_source(".text\n.unknown_directive\nret\n").unwrap_err();
    assert_eq!(err.line, Some(2));
    assert!(err.col.is_some(), "expected a source column, got {err:?}");
    assert!(
        err.msg.contains("unsupported directive"),
        "expected unsupported-directive error, got {err}"
    );
}

#[test]
fn assemble_stmts_accepts_valid_preparsed_statements() {
    let stmts = vec![
        Stmt::Directive(Directive::Global("_main".into())),
        Stmt::Directive(Directive::Text),
        Stmt::Label("_main".into()),
        Stmt::Instruction(Inst::Ret { rn: X30 }),
    ];

    let obj = assemble::assemble_stmts(&stmts).expect("assemble valid stmts");

    assert_eq!(obj.text_section().data, [0xC0, 0x03, 0x5F, 0xD6]);
    assert_eq!(obj.text_section().size, 4);
    assert!(obj.symbols.iter().any(|sym| {
        sym.name == "_main" && sym.global && !sym.undefined && sym.section == 1 && sym.value == 0
    }));
}

#[test]
fn assemble_stmts_validates_without_source_context() {
    let stmts = vec![
        Stmt::Directive(Directive::Text),
        Stmt::Label("_duplicate".into()),
        Stmt::Label("_duplicate".into()),
    ];

    let err = assemble::assemble_stmts(&stmts).unwrap_err();
    assert_eq!(err.line, None);
    assert_eq!(err.col, None);
    assert!(
        err.msg.contains("duplicate label"),
        "expected duplicate-label validation error, got {err}"
    );
}

#[test]
fn assemble_instructions_emits_text_and_requested_globals() {
    let obj = assemble::assemble_instructions(&[Inst::Nop, Inst::Ret { rn: X30 }], &["_main"]);

    assert_eq!(
        obj.text_section().data,
        [0x1F, 0x20, 0x03, 0xD5, 0xC0, 0x03, 0x5F, 0xD6]
    );
    assert_eq!(obj.text_section().size, 8);
    assert!(obj.symbols.iter().any(|sym| {
        sym.name == "ltmp0" && !sym.global && !sym.undefined && sym.section == 1 && sym.value == 0
    }));
    assert!(obj.symbols.iter().any(|sym| {
        sym.name == "_main" && sym.global && !sym.undefined && sym.section == 1 && sym.value == 0
    }));
}

#[test]
fn assemble_instructions_trusts_callers_to_build_valid_insts() {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        assemble::assemble_instructions(
            &[Inst::AndImm {
                rd: X0,
                rn: X1,
                imm: 0,
                sf: true,
            }],
            &[],
        )
    }));

    assert!(
        result.is_err(),
        "invalid logical-immediate Inst should panic in the trusted fast path"
    );
}
