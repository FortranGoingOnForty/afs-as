#[path = "common/elf.rs"]
mod celf;

use afs_as::elf::{
    parse_elf, write_elf, ObjectFile, SymbolPlace, ELFOSABI_FREEBSD, ELFOSABI_NONE, STB_LOCAL,
    STT_FILE, STT_NOTYPE, STT_OBJECT, STV_DEFAULT,
};
use afs_as::x86::assemble::assemble_x86;

const SOURCE: &str = ".local pre_file_common\n\
.comm pre_file_common,8,8\n\
.file \"first.f90\"\n\
.local first_file_common\n\
.comm first_file_common,8,8\n\
.text\n\
local_before:\n\
ret\n\
.file \"path/second file.f90\"\n\
.local second_file_common\n\
.comm second_file_common,8,8\n\
.file \"\"\n\
.globl entry\n\
entry:\n\
ret\n";

fn host_osabi() -> u8 {
    if cfg!(target_os = "freebsd") {
        ELFOSABI_FREEBSD
    } else {
        ELFOSABI_NONE
    }
}

fn file_metadata(object: &ObjectFile) -> Vec<(String, u8, u8, u8, SymbolPlace, u64, u64)> {
    object
        .symbols
        .iter()
        .filter(|symbol| symbol.typ == STT_FILE)
        .map(|symbol| {
            (
                symbol.name.clone(),
                symbol.bind,
                symbol.typ,
                symbol.vis,
                symbol.place,
                symbol.value,
                symbol.size,
            )
        })
        .collect()
}

fn expected_file_metadata() -> Vec<(String, u8, u8, u8, SymbolPlace, u64, u64)> {
    ["first.f90", "path/second file.f90", ""]
        .into_iter()
        .map(|name| {
            (
                name.into(),
                STB_LOCAL,
                STT_FILE,
                STV_DEFAULT,
                SymbolPlace::Abs,
                0,
                0,
            )
        })
        .collect()
}

fn local_symbol_order(object: &ObjectFile) -> Vec<(String, u8)> {
    object
        .symbols
        .iter()
        .filter(|symbol| symbol.bind == STB_LOCAL)
        .map(|symbol| (symbol.name.clone(), symbol.typ))
        .collect()
}

fn expected_local_symbol_order() -> Vec<(String, u8)> {
    [
        ("first.f90", STT_FILE),
        ("pre_file_common", STT_OBJECT),
        ("first_file_common", STT_OBJECT),
        ("local_before", STT_NOTYPE),
        ("path/second file.f90", STT_FILE),
        ("second_file_common", STT_OBJECT),
        ("", STT_FILE),
    ]
    .into_iter()
    .map(|(name, typ)| (name.into(), typ))
    .collect()
}

#[test]
fn file_directives_partition_local_symbols_in_gnu_order() {
    let object = assemble_x86(SOURCE, host_osabi()).expect("assemble .file directives");
    assert_eq!(file_metadata(&object), expected_file_metadata());
    assert_eq!(local_symbol_order(&object), expected_local_symbol_order());

    let emitted = write_elf(&object).expect("serialize ELF object");
    let reparsed = parse_elf(&emitted).expect("parse serialized ELF object");
    assert_eq!(file_metadata(&reparsed), expected_file_metadata());
    assert_eq!(local_symbol_order(&reparsed), expected_local_symbol_order());
}

#[test]
fn file_directive_rejects_a_missing_quoted_name() {
    let error = assemble_x86(".file\n", host_osabi())
        .expect_err("operand-less .file directive unexpectedly assembled");

    assert_eq!(error.line, Some(1));
    assert_eq!(error.col, Some(1));
    assert_eq!(error.msg, ".file requires one quoted file name");
}

#[test]
fn file_directive_rejects_unquoted_or_extra_name_tokens() {
    for source in [
        ".file source.f90\n",
        ".file 1 \"source.f90\"\n",
        ".file \"first.f90\" \"second.f90\"\n",
    ] {
        let error = assemble_x86(source, host_osabi())
            .expect_err("malformed .file directive unexpectedly assembled");
        assert_eq!(error.line, Some(1), "source: {source:?}");
        assert_eq!(error.col, Some(1), "source: {source:?}");
        assert_eq!(
            error.msg, ".file requires one quoted file name",
            "source: {source:?}"
        );
    }
}

#[test]
fn file_directive_metadata_matches_gnu_as() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_file_directive",
            "file_directive_metadata_matches_gnu_as",
            "no GNU assembler on this host",
        );
        return;
    };
    let artifacts = celf::TempArtifacts::new("afs_x86_file_directive");
    let source_path = artifacts.path(".s");
    let gas_path = artifacts.path("_gas.o");
    std::fs::write(&source_path, SOURCE).expect("write GNU as input");
    celf::assemble_with_gas(&gas, &source_path, &gas_path);

    let gas_object = parse_elf(&std::fs::read(&gas_path).expect("read GNU as object"))
        .expect("parse GNU as object");
    let ours = assemble_x86(SOURCE, host_osabi()).expect("assemble .file directives");
    let ours = parse_elf(&write_elf(&ours).expect("serialize afs-as object"))
        .expect("parse afs-as object");

    assert_eq!(file_metadata(&ours), file_metadata(&gas_object));
    assert_eq!(local_symbol_order(&ours), local_symbol_order(&gas_object));
    assert_eq!(celf::normalize(&ours), celf::normalize(&gas_object));
}
