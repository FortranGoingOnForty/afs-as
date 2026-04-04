#[path = "common/corpus.rs"]
mod common;

use std::fs;

fn assemble_fixture(name: &str) -> common::TempPaths {
    let paths = common::TempPaths::new("afs_corpus");
    let asm = common::read_fixture(name);
    fs::write(&paths.asm, &asm).expect("write fixture");
    common::assemble_with_ours(&asm, &paths.obj);
    common::assemble_with_system(&paths.asm, &paths.ref_obj);
    paths
}

#[test]
fn corpus_hello_world_matches_text_bytes_and_runs() {
    let paths = assemble_fixture("hello_world.s");

    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );

    common::link_with_system(&paths.obj, &paths.bin, "_main");
    let (code, stdout, stderr) = common::run_binary(&paths.bin);
    assert_eq!(code, 0, "stderr:\n{}", stderr);
    assert_eq!(stdout, "Hello, World!\n");
}

#[test]
fn corpus_local_branches_match_text_bytes() {
    let paths = assemble_fixture("local_branches.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_numeric_local_labels_match_text_bytes() {
    let paths = assemble_fixture("numeric_local_labels.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_addressing_surface_matches_text_bytes() {
    let paths = assemble_fixture("addressing_surface.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_fp_load_store_surface_matches_text_bytes() {
    let paths = assemble_fixture("fp_load_store_surface.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_fp_pair_surface_matches_text_bytes() {
    let paths = assemble_fixture("fp_pair_surface.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_system_hints_matches_text_bytes() {
    let paths = assemble_fixture("system_hints.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_branch_address_surface_matches_text_bytes() {
    let paths = assemble_fixture("branch_address_surface.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_shifted_addsub_matches_text_bytes() {
    let paths = assemble_fixture("shifted_addsub.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_extended_addsub_matches_text_bytes() {
    let paths = assemble_fixture("extended_addsub.s");
    assert_eq!(
        common::object_text_bytes(&paths.obj),
        common::object_text_bytes(&paths.ref_obj)
    );
}

#[test]
fn corpus_external_call_matches_relocations_and_symbols() {
    let paths = assemble_fixture("external_call.s");

    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);
    let ours_symbols = common::object_symbols(&paths.obj);
    let ref_symbols = common::object_symbols(&paths.ref_obj);

    assert!(ours_relocs.contains("BR26"), "missing BR26 relocation:\n{}", ours_relocs);
    assert!(ours_relocs.contains("_puts"), "missing _puts relocation:\n{}", ours_relocs);
    assert!(ours_symbols.contains(" U _puts"), "missing undefined _puts:\n{}", ours_symbols);

    assert_eq!(normalize_tool_output(&ours_relocs), normalize_tool_output(&ref_relocs));
    assert_eq!(normalize_tool_output(&ours_symbols), normalize_tool_output(&ref_symbols));
}

#[test]
fn corpus_external_branches_match_relocations_and_symbols() {
    let paths = assemble_fixture("external_branches.s");

    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);
    let ours_symbols = common::object_symbols(&paths.obj);
    let ref_symbols = common::object_symbols(&paths.ref_obj);

    assert!(ours_relocs.matches("BR26").count() >= 2, "missing BR26 relocations:\n{}", ours_relocs);
    assert!(ours_symbols.contains(" U _exit"), "missing undefined _exit:\n{}", ours_symbols);
    assert!(ours_symbols.contains(" U _puts"), "missing undefined _puts:\n{}", ours_symbols);

    assert_eq!(normalize_tool_output(&ours_relocs), normalize_tool_output(&ref_relocs));
    assert_eq!(normalize_tool_output(&ours_symbols), normalize_tool_output(&ref_symbols));
}

#[test]
fn fixture_paths_are_resolved_from_corpus_directory() {
    let path = common::fixture_path("hello_world.s");
    assert!(path.ends_with("tests/corpus/hello_world.s"));
}

#[test]
fn corpus_cstring_section_matches_load_commands_and_symbols() {
    let paths = assemble_fixture("cstring_data.s");

    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_symbols = common::object_symbols(&paths.obj);
    let ref_symbols = common::object_symbols(&paths.ref_obj);

    assert!(ours_load.contains("sectname __cstring"), "missing __cstring section:\n{}", ours_load);
    assert!(ours_symbols.contains(" s greeting"), "missing cstring symbol:\n{}", ours_symbols);

    assert_eq!(normalize_tool_output(&ours_load), normalize_tool_output(&ref_load));
    assert_eq!(normalize_tool_output(&ours_symbols), normalize_tool_output(&ref_symbols));
}

#[test]
fn corpus_section_inventory_matches_load_commands_and_symbols() {
    let paths = assemble_fixture("section_inventory.s");

    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_symbols = common::object_symbols(&paths.obj);
    let ref_symbols = common::object_symbols(&paths.ref_obj);

    assert!(ours_load.contains("sectname __cstring"), "missing __cstring section:\n{}", ours_load);
    assert!(ours_load.contains("sectname __const"), "missing __const section:\n{}", ours_load);
    assert!(ours_load.contains("sectname __bss"), "missing __bss section:\n{}", ours_load);
    assert!(ours_load.contains("flags 0x00000001"), "missing zerofill flag:\n{}", ours_load);
    assert!(ours_symbols.contains(" b scratch"), "missing bss symbol:\n{}", ours_symbols);

    assert_eq!(normalize_tool_output(&ours_load), normalize_tool_output(&ref_load));
    assert_eq!(normalize_tool_output(&ours_symbols), normalize_tool_output(&ref_symbols));
}

#[test]
fn corpus_symbol_attributes_match_nm_output() {
    let paths = assemble_fixture("symbol_attrs.s");

    let ours_symbols = common::object_symbols_verbose(&paths.obj);
    let ref_symbols = common::object_symbols_verbose(&paths.ref_obj);

    assert!(ours_symbols.contains("_helper"), "missing private extern helper:\n{}", ours_symbols);
    assert!(ours_symbols.contains("_entry"), "missing weak definition entry:\n{}", ours_symbols);
    assert!(ours_symbols.contains("_puts"), "missing weak reference puts:\n{}", ours_symbols);

    assert_eq!(normalize_tool_output(&ours_symbols), normalize_tool_output(&ref_symbols));
}

#[test]
fn corpus_expression_symbols_match_bytes_relocations_and_symbols() {
    let paths = assemble_fixture("expression_symbols.s");

    let ours_text = common::object_text_bytes(&paths.obj);
    let ref_text = common::object_text_bytes(&paths.ref_obj);
    let ours_data = common::object_section_bytes(&paths.obj, "__DATA", "__data");
    let ref_data = common::object_section_bytes(&paths.ref_obj, "__DATA", "__data");
    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);
    let ours_symbols = common::object_symbols_verbose(&paths.obj);
    let ref_symbols = common::object_symbols_verbose(&paths.ref_obj);

    assert_eq!(ours_text, ref_text);
    assert_eq!(ours_data, ref_data);
    assert_eq!(normalize_tool_output(&ours_relocs), normalize_tool_output(&ref_relocs));
    assert_eq!(normalize_tool_output(&ours_symbols), normalize_tool_output(&ref_symbols));
}

#[test]
fn corpus_storage_directives_match_bytes_sections_and_symbols() {
    let paths = assemble_fixture("storage_directives.s");

    let ours_text = common::object_text_bytes(&paths.obj);
    let ref_text = common::object_text_bytes(&paths.ref_obj);
    let ours_data = common::object_section_bytes(&paths.obj, "__DATA", "__data");
    let ref_data = common::object_section_bytes(&paths.ref_obj, "__DATA", "__data");
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_symbols = common::object_symbols_verbose(&paths.obj);
    let ref_symbols = common::object_symbols_verbose(&paths.ref_obj);

    assert!(ours_load.contains("sectname __bss"), "missing __bss section:\n{}", ours_load);
    assert!(ours_symbols.contains("(common)"), "missing common symbol:\n{}", ours_symbols);
    assert!(ours_symbols.contains("_scratch"), "missing zerofill symbol:\n{}", ours_symbols);

    assert_eq!(ours_text, ref_text);
    assert_eq!(ours_data, ref_data);
    assert_eq!(normalize_tool_output(&ours_load), normalize_tool_output(&ref_load));
    assert_eq!(normalize_tool_output(&ours_symbols), normalize_tool_output(&ref_symbols));
}

#[test]
fn corpus_alignment_directives_match_text_data_and_section_alignment() {
    let paths = assemble_fixture("alignment_directives.s");

    let ours_text = common::object_text_bytes(&paths.obj);
    let ref_text = common::object_text_bytes(&paths.ref_obj);
    let ours_data = common::object_section_bytes(&paths.obj, "__DATA", "__data");
    let ref_data = common::object_section_bytes(&paths.ref_obj, "__DATA", "__data");
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);

    assert!(ours_load.contains("align 2^5 (32)"), "missing text alignment:\n{}", ours_load);
    assert!(ours_load.contains("align 2^3 (8)"), "missing data alignment:\n{}", ours_load);

    assert_eq!(ours_text, ref_text);
    assert_eq!(ours_data, ref_data);
    assert_eq!(normalize_tool_output(&ours_load), normalize_tool_output(&ref_load));
}

#[test]
fn corpus_metadata_directives_match_header_and_load_commands() {
    let paths = assemble_fixture("metadata_directives.s");

    let ours_header = common::object_header(&paths.obj);
    let ref_header = common::object_header(&paths.ref_obj);
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);

    assert!(
        ours_header.contains("SUBSECTIONS_VIA_SYMBOLS"),
        "missing subsections flag:\n{}",
        ours_header
    );
    assert!(ours_load.contains("minos 11.0"), "missing minos:\n{}", ours_load);
    assert!(ours_load.contains("sdk 15.5"), "missing sdk:\n{}", ours_load);

    assert_eq!(normalize_tool_output(&ours_header), normalize_tool_output(&ref_header));
    assert_eq!(normalize_tool_output(&ours_load), normalize_tool_output(&ref_load));
}

#[test]
fn corpus_cfi_surface_matches_text_bytes_load_commands_and_relocations() {
    let paths = assemble_fixture("cfi_surface.s");

    let ours_text = common::object_text_bytes(&paths.obj);
    let ref_text = common::object_text_bytes(&paths.ref_obj);
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);

    assert!(ours_load.contains("sectname __compact_unwind"), "missing compact unwind section:\n{}", ours_load);
    assert!(ours_relocs.contains("__compact_unwind"), "missing compact unwind relocations:\n{}", ours_relocs);

    assert_eq!(ours_text, ref_text);
    assert_eq!(normalize_tool_output(&ours_load), normalize_tool_output(&ref_load));
    assert_eq!(normalize_tool_output(&ours_relocs), normalize_tool_output(&ref_relocs));
}

#[test]
fn corpus_cfi_nostack_matches_load_commands_and_relocations() {
    let paths = assemble_fixture("cfi_nostack.s");

    let ours_text = common::object_text_bytes(&paths.obj);
    let ref_text = common::object_text_bytes(&paths.ref_obj);
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);

    assert_eq!(ours_text, ref_text);
    assert_eq!(normalize_tool_output(&ours_load), normalize_tool_output(&ref_load));
    assert_eq!(normalize_tool_output(&ours_relocs), normalize_tool_output(&ref_relocs));
}

#[test]
fn corpus_cfi_saved_pairs_matches_load_commands_and_relocations() {
    let paths = assemble_fixture("cfi_saved_pairs.s");

    let ours_text = common::object_text_bytes(&paths.obj);
    let ref_text = common::object_text_bytes(&paths.ref_obj);
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);

    assert_eq!(ours_text, ref_text);
    assert_eq!(normalize_tool_output(&ours_load), normalize_tool_output(&ref_load));
    assert_eq!(normalize_tool_output(&ours_relocs), normalize_tool_output(&ref_relocs));
}

fn normalize_tool_output(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_end().ends_with(".o:"))
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}
