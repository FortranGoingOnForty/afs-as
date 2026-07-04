#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;

fn assemble_fixture(name: &str) -> common::TempPaths {
    let paths = common::TempPaths::new("afs_directive_inventory");
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
fn directive_inventory_extended_matches_system_as() {
    if !common::native_macho_host("directive_inventory_corpus", "directive_inventory_extended_matches_system_as") {
        return;
    }
    let paths = assemble_fixture("directive_inventory_extended.s");

    let ours_header = common::object_header(&paths.obj);
    let ref_header = common::object_header(&paths.ref_obj);
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_symbols = common::object_symbols_preserve_order(&paths.obj);
    let ref_symbols = common::object_symbols_preserve_order(&paths.ref_obj);

    for needle in ["_directive_inventory", "_common_buf", "_scratch_buf"] {
        assert!(
            normalize_tool_output(&ours_symbols).contains(needle),
            "missing {} in object metadata\nload:\n{}\nsymbols:\n{}",
            needle,
            ours_load,
            ours_symbols
        );
    }
    assert!(
        normalize_tool_output(&ours_header).contains("SUBSECTIONS_VIA_SYMBOLS"),
        "missing SUBSECTIONS_VIA_SYMBOLS in header\n{}",
        ours_header
    );

    assert_eq!(
        normalize_tool_output(&ours_header),
        normalize_tool_output(&ref_header)
    );
    assert_eq!(
        common::object_section_bytes(&paths.obj, "__DATA", "__data"),
        common::object_section_bytes(&paths.ref_obj, "__DATA", "__data")
    );
    assert_eq!(
        normalize_tool_output(&ours_load),
        normalize_tool_output(&ref_load)
    );
    assert_eq!(
        normalize_tool_output(&ours_symbols),
        normalize_tool_output(&ref_symbols)
    );
}
