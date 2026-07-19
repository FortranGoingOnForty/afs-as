use afs_as::assemble::assemble_source;

#[test]
fn relocation_targets_survive_symbol_sorting() {
    let source = ".data\n\
.extern _z_declared\n\
.extern _a_declared\n\
.quad _z_missing\n\
.quad _z_declared\n\
.quad _a_missing\n\
.quad _a_declared\n\
.quad _z_missing\n";
    let obj = assemble_source(source).expect("assemble relocation fixture");
    let data = obj.section("__DATA", "__data").expect("data section");
    let targets: Vec<_> = data
        .relocations
        .iter()
        .map(|relocation| {
            obj.symbols
                .get(relocation.symbol_idx as usize)
                .expect("relocation symbol index")
                .name
                .as_str()
        })
        .collect();

    assert_eq!(
        targets,
        [
            "_z_missing",
            "_z_declared",
            "_a_missing",
            "_a_declared",
            "_z_missing",
        ]
    );
    for name in ["_z_missing", "_z_declared", "_a_missing", "_a_declared"] {
        assert_eq!(
            obj.symbols
                .iter()
                .filter(|symbol| symbol.name == name)
                .count(),
            1,
            "symbol {name} was not deduplicated"
        );
    }
}

#[test]
fn attributed_absolute_relocation_keeps_one_symbol() {
    let source = ".global _answer\n\
.set _answer,42\n\
.data\n\
.quad _answer@GOT\n";
    let obj = assemble_source(source).expect("assemble absolute relocation fixture");
    let symbols: Vec<_> = obj
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "_answer")
        .collect();

    assert_eq!(symbols.len(), 1);
    assert!(symbols[0].absolute);
    assert!(!symbols[0].undefined);
}
