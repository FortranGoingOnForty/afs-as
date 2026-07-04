#[path = "common/harness.rs"]
mod common;

use std::fs;

#[test]
fn harness_can_compare_text_bytes() {
    if !common::native_macho_host("differential_harness", "harness_can_compare_text_bytes") {
        return;
    }
    let paths = common::TempPaths::new("afs_harness_bytes");
    let asm = ".text\nadd x0, x1, x2\nret\n";

    fs::write(&paths.asm, asm).expect("write asm");
    common::assemble_with_ours(asm, &paths.obj);
    common::assemble_with_system(&paths.asm, &paths.ref_obj);

    let ours = common::object_text_bytes(&paths.obj);
    let theirs = common::object_text_bytes(&paths.ref_obj);
    assert_eq!(ours, theirs);
}

#[test]
fn harness_can_inspect_object_metadata() {
    if !common::native_macho_host("differential_harness", "harness_can_inspect_object_metadata") {
        return;
    }
    let paths = common::TempPaths::new("afs_harness_meta");
    let asm = ".global _main\n.text\n_main:\nret\n";

    fs::write(&paths.asm, asm).expect("write asm");
    common::assemble_with_ours(asm, &paths.obj);

    let load_commands = common::object_load_commands(&paths.obj);
    let symbols = common::object_symbols(&paths.obj);

    assert!(
        load_commands.contains("LC_SEGMENT_64"),
        "missing LC_SEGMENT_64:\n{}",
        load_commands
    );
    assert!(
        load_commands.contains("__text"),
        "missing __text section:\n{}",
        load_commands
    );
    assert!(
        symbols.contains("_main"),
        "missing _main symbol:\n{}",
        symbols
    );
}
