#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;

fn assemble_fixture(name: &str) -> common::TempPaths {
    let paths = common::TempPaths::new("afs_section_inventory");
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
fn section_inventory_extended_matches_load_commands_and_symbols() {
    let paths = assemble_fixture("section_inventory_extended.s");

    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_symbols = common::object_symbols(&paths.obj);
    let ref_symbols = common::object_symbols(&paths.ref_obj);

    for sect in [
        "__cstring",
        "__const",
        "__literal16",
        "__thread_data",
        "__thread_vars",
        "__thread_bss",
        "__bss",
    ] {
        assert!(
            ours_load.contains(&format!("sectname {}", sect)),
            "missing {} section:\n{}",
            sect,
            ours_load
        );
    }
    assert!(
        ours_symbols.contains("_tls_value$tlv$init"),
        "missing thread data init symbol:\n{}",
        ours_symbols
    );
    assert!(
        ours_symbols.contains("_tls_counter$tlv$init"),
        "missing thread bss init symbol:\n{}",
        ours_symbols
    );
    assert!(
        ours_symbols.contains(" b scratch"),
        "missing bss symbol:\n{}",
        ours_symbols
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
