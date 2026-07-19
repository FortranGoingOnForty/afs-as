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

#[test]
fn scalar_integer_fp_conversions_preserve_both_operand_widths() {
    for (source, expected) in [
        ("fcvtzs x0, d1", 0x9E78_0020),
        ("fcvtzs x0, s1", 0x9E38_0020),
        ("fcvtzs w0, d1", 0x1E78_0020),
        ("fcvtzs w0, s1", 0x1E38_0020),
        ("scvtf d0, x1", 0x9E62_0020),
        ("scvtf d0, w1", 0x1E62_0020),
        ("scvtf s0, x1", 0x9E22_0020),
        ("scvtf s0, w1", 0x1E22_0020),
    ] {
        assert_eq!(parse_inst(source).encode(), expected, "source: {source}");
    }
}
