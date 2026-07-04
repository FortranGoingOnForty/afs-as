#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;

fn assemble_fixture(name: &str) -> common::TempPaths {
    let paths = common::TempPaths::new("afs_metadata_tls_zerofill");
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
fn metadata_tls_zerofill_matches_system_as() {
    if !common::native_macho_host("metadata_tls_zerofill_corpus", "metadata_tls_zerofill_matches_system_as") {
        return;
    }
    let paths = assemble_fixture("metadata_tls_zerofill.s");

    let ours_header = common::object_header(&paths.obj);
    let ref_header = common::object_header(&paths.ref_obj);
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);
    let ours_symbols = common::object_symbols_verbose(&paths.obj);
    let ref_symbols = common::object_symbols_verbose(&paths.ref_obj);

    assert!(
        normalize_tool_output(&ours_load).contains("sectname __thread_bss"),
        "missing __thread_bss section\n{}",
        ours_load
    );
    assert!(
        normalize_tool_output(&ours_load).contains("sectname __thread_vars"),
        "missing __thread_vars section\n{}",
        ours_load
    );
    assert!(
        normalize_tool_output(&ours_load).contains("minos 14.1"),
        "missing 14.1 minos in load commands\n{}",
        ours_load
    );
    assert!(
        normalize_tool_output(&ours_symbols).contains("_tls_counter$tlv$init"),
        "missing tls init symbol\n{}",
        ours_symbols
    );
    assert!(
        normalize_tool_output(&ours_symbols).contains("_tls_counter"),
        "missing tls variable symbol\n{}",
        ours_symbols
    );

    assert_eq!(
        normalize_tool_output(&ours_header),
        normalize_tool_output(&ref_header)
    );
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
