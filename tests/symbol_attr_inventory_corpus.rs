#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;

fn assemble_fixture(name: &str) -> common::TempPaths {
    let paths = common::TempPaths::new("afs_symbol_attr_inventory");
    let asm = common::read_fixture(name);
    fs::write(&paths.asm, &asm).expect("write fixture");
    common::assemble_with_ours(&asm, &paths.obj);
    common::assemble_with_system(&paths.asm, &paths.ref_obj);
    paths
}

fn normalize_tool_output(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_end().ends_with(".o:") && !line.trim_end().ends_with(":"))
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn symbol_attr_inventory_matches_system_as() {
    if !common::native_macho_host("symbol_attr_inventory_corpus", "symbol_attr_inventory_matches_system_as") {
        return;
    }
    let paths = assemble_fixture("symbol_attr_inventory.s");

    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);
    let ours_symbols = common::object_symbols_verbose(&paths.obj);
    let ref_symbols = common::object_symbols_verbose(&paths.ref_obj);

    for needle in [
        "external _entry",
        "private external _hidden",
        "weak external _puts",
        "non-external _weak_def",
    ] {
        assert!(
            normalize_tool_output(&ours_symbols).contains(needle),
            "missing {} in verbose symbols\n{}",
            needle,
            ours_symbols
        );
    }

    assert_eq!(
        normalize_tool_output(&ours_load),
        normalize_tool_output(&ref_load)
    );
    assert_eq!(
        normalize_tool_output(&ours_relocs),
        normalize_tool_output(&ref_relocs)
    );
    assert_eq!(
        normalize_tool_output(&ours_symbols),
        normalize_tool_output(&ref_symbols)
    );
}
