use afs_as::parse::parse;

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
fn fixed_width_operand_roles_reject_wrong_register_classes() {
    for source in [
        "br w0",
        "blr w0",
        "ret w0",
        "adr w0, #0",
        "adrp w0, #0",
        "ldrb x0, [x1]",
        "strh x0, [x1]",
        "ldr x0, [w1]",
        "str w0, [w1]",
        "ldur x0, [w1]",
        "stur w0, [w1]",
        "ldr d0, [w1]",
        "str q0, [w1]",
        "ldrsb x0, [w1]",
        "ldrsh w0, [w1]",
        "ldrsw x0, [w1]",
        "ldp x0, x1, [w2]",
        "stp d0, d1, [w2]",
        "ld1.s { v0 }[0], [w1]",
    ] {
        assert_rejected(source, "register");
    }
}
