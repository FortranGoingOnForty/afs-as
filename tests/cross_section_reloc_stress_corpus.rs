#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;

fn assemble_fixture(name: &str) -> common::TempPaths {
    let paths = common::TempPaths::new("afs_cross_section_reloc_stress");
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
fn cross_section_reloc_stress_matches_system_as() {
    let paths = assemble_fixture("cross_section_reloc_stress.s");

    let ours_text = common::object_text_bytes(&paths.obj);
    let ref_text = common::object_text_bytes(&paths.ref_obj);
    let ours_cstring = common::object_section_bytes(&paths.obj, "__TEXT", "__cstring");
    let ref_cstring = common::object_section_bytes(&paths.ref_obj, "__TEXT", "__cstring");
    let ours_const = common::object_section_bytes(&paths.obj, "__TEXT", "__const");
    let ref_const = common::object_section_bytes(&paths.ref_obj, "__TEXT", "__const");
    let ours_literal16 = common::object_section_bytes(&paths.obj, "__TEXT", "__literal16");
    let ref_literal16 = common::object_section_bytes(&paths.ref_obj, "__TEXT", "__literal16");
    let ours_data = common::object_section_bytes(&paths.obj, "__DATA", "__data");
    let ref_data = common::object_section_bytes(&paths.ref_obj, "__DATA", "__data");
    let ours_thread_data = common::object_section_bytes(&paths.obj, "__DATA", "__thread_data");
    let ref_thread_data = common::object_section_bytes(&paths.ref_obj, "__DATA", "__thread_data");
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);
    let ours_symbols = common::object_symbols_verbose(&paths.obj);
    let ref_symbols = common::object_symbols_verbose(&paths.ref_obj);
    let ours_symbols_raw = common::object_symbols_preserve_order(&paths.obj);
    let ref_symbols_raw = common::object_symbols_preserve_order(&paths.ref_obj);

    for needle in [
        "sectname __cstring",
        "sectname __const",
        "sectname __literal16",
        "sectname __data",
        "sectname __bss",
        "sectname __thread_data",
        "sectname __thread_bss",
        "sectname __thread_vars",
    ] {
        assert!(
            normalize_tool_output(&ours_load).contains(needle),
            "missing {} in load commands\n{}",
            needle,
            ours_load
        );
    }

    for needle in [
        "BR26",
        "PAGE21",
        "PAGOF12",
        "GOTLDP",
        "SUB",
        "_puts",
        "_ext",
        "_tls_value$tlv$init",
        "_tls_counter$tlv$init",
        "__tlv_bootstrap",
    ] {
        assert!(
            normalize_tool_output(&ours_relocs).contains(needle),
            "missing {} in relocations\n{}",
            needle,
            ours_relocs
        );
    }

    for needle in [
        "_cross_section_reloc_stress",
        "_local_hidden",
        "msg",
        "const_ptr",
        "lit16",
        "data_ptr",
        "_scratch_buf",
        "_tls_value$tlv$init",
        "_tls_counter$tlv$init",
        "_tls_value",
        "_tls_counter",
        "_puts",
        "_ext",
        "_other",
        "__tlv_bootstrap",
    ] {
        assert!(
            normalize_tool_output(&ours_symbols).contains(needle),
            "missing {} in symbols\n{}",
            needle,
            ours_symbols
        );
    }

    assert_eq!(ours_text, ref_text);
    assert_eq!(ours_cstring, ref_cstring);
    assert_eq!(ours_const, ref_const);
    assert_eq!(ours_literal16, ref_literal16);
    assert_eq!(ours_data, ref_data);
    assert_eq!(ours_thread_data, ref_thread_data);
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
    assert_eq!(
        normalize_tool_output(&ours_symbols_raw),
        normalize_tool_output(&ref_symbols_raw)
    );
    assert_eq!(
        fs::read(&paths.obj).expect("read ours"),
        fs::read(&paths.ref_obj).expect("read ref")
    );
}

#[test]
fn cross_section_reloc_stress_links_relocatable_with_support() {
    let paths = assemble_fixture("cross_section_reloc_stress.s");
    let root = paths.asm.parent().expect("temp root");
    let support = root.join("link-support.o");
    let ours_linked = root.join("ours-linked.o");
    let ref_linked = root.join("ref-linked.o");

    common::assemble_link_support(&support);
    common::link_relocatable_with_system(&[&paths.obj, &support], &ours_linked);
    common::link_relocatable_with_system(&[&paths.ref_obj, &support], &ref_linked);

    let ours_undefined = common::object_undefined_symbols(&ours_linked);
    let ref_undefined = common::object_undefined_symbols(&ref_linked);

    assert!(
        normalize_tool_output(&ours_undefined).contains("__tlv_bootstrap"),
        "missing __tlv_bootstrap after ld -r\n{}",
        ours_undefined
    );
    assert_eq!(
        normalize_tool_output(&ours_undefined),
        normalize_tool_output(&ref_undefined)
    );
}
