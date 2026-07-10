#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;

fn assemble_fixture(name: &str) -> common::TempPaths {
    let paths = common::TempPaths::new("afs_metadata_stress_mix");
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
fn metadata_stress_mix_matches_system_as() {
    if !common::native_macho_host(
        "metadata_stress_mix_corpus",
        "metadata_stress_mix_matches_system_as",
    ) {
        return;
    }
    let paths = assemble_fixture("metadata_stress_mix.s");

    let ours_header = common::object_header(&paths.obj);
    let ref_header = common::object_header(&paths.ref_obj);
    let ours_load = common::object_load_commands(&paths.obj);
    let ref_load = common::object_load_commands(&paths.ref_obj);
    let ours_relocs = common::object_relocations(&paths.obj);
    let ref_relocs = common::object_relocations(&paths.ref_obj);
    let ours_symbols = common::object_symbols_verbose(&paths.obj);
    let ref_symbols = common::object_symbols_verbose(&paths.ref_obj);

    for needle in [
        "SUBSECTIONS_VIA_SYMBOLS",
        "sectname __bss",
        "sectname __thread_data",
        "sectname __thread_bss",
        "sectname __thread_vars",
    ] {
        let haystack = if needle == "SUBSECTIONS_VIA_SYMBOLS" {
            &ours_header
        } else {
            &ours_load
        };
        assert!(
            normalize_tool_output(haystack).contains(needle),
            "missing {} in object metadata\nheader:\n{}\nload:\n{}",
            needle,
            ours_header,
            ours_load
        );
    }

    for needle in [
        "_common_buf",
        "_scratch_buf",
        "_tls_value$tlv$init",
        "_tls_counter$tlv$init",
        "_tls_value",
        "_tls_counter",
        "_hidden",
        "_puts",
    ] {
        assert!(
            normalize_tool_output(&ours_symbols).contains(needle),
            "missing {} in verbose symbols\n{}",
            needle,
            ours_symbols
        );
    }

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
