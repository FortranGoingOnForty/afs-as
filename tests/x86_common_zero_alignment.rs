#[path = "common/elf.rs"]
mod celf;

use afs_as::elf::{parse_elf, SymbolPlace, ELFOSABI_FREEBSD, ELFOSABI_NONE};
use afs_as::x86::assemble::assemble_x86;

const GLOBAL_ZERO: &str = ".comm c0,0,0\n\
.comm c1,1,0\n\
.comm c3,3,0\n\
.comm c8,8,0\n\
.comm c9,9,0\n\
.comm c16,16,0\n\
.comm c17,17,0\n\
.comm c32,32,0\n";

const GLOBAL_OMITTED: &str = ".comm c0,0\n\
.comm c1,1\n\
.comm c3,3\n\
.comm c8,8\n\
.comm c9,9\n\
.comm c16,16\n\
.comm c17,17\n\
.comm c32,32\n";

const LOCAL_ZERO: &str = ".bss\n\
.space 1\n\
.local local_zero\n\
.comm local_zero,8,0\n";

const LOCAL_OMITTED: &str = ".bss\n\
.space 1\n\
.local local_zero\n\
.comm local_zero,8\n";

fn host_osabi() -> u8 {
    if cfg!(target_os = "freebsd") {
        ELFOSABI_FREEBSD
    } else {
        ELFOSABI_NONE
    }
}

fn common_metadata(object: &afs_as::elf::ObjectFile) -> Vec<(&str, u64, u64)> {
    object
        .symbols
        .iter()
        .filter(|symbol| symbol.place == SymbolPlace::Common)
        .map(|symbol| (symbol.name.as_str(), symbol.value, symbol.size))
        .collect()
}

#[test]
fn explicit_zero_global_alignment_uses_the_size_default() {
    let explicit =
        assemble_x86(GLOBAL_ZERO, host_osabi()).expect("assemble explicit zero alignments");
    let omitted = assemble_x86(GLOBAL_OMITTED, host_osabi()).expect("assemble omitted alignments");

    assert_eq!(explicit, omitted);
    assert_eq!(
        common_metadata(&explicit),
        [
            ("c0", 1, 0),
            ("c1", 1, 1),
            ("c3", 4, 3),
            ("c8", 8, 8),
            ("c9", 16, 9),
            ("c16", 16, 16),
            ("c17", 16, 17),
            ("c32", 16, 32),
        ]
    );
}

#[test]
fn explicit_zero_local_alignment_uses_the_local_destination_default() {
    let explicit = assemble_x86(LOCAL_ZERO, host_osabi()).expect("assemble local zero alignment");
    let omitted =
        assemble_x86(LOCAL_OMITTED, host_osabi()).expect("assemble local default alignment");

    assert_eq!(explicit, omitted);
    let bss_index = explicit
        .sections
        .iter()
        .position(|section| section.name == ".bss")
        .expect("bss section");
    let bss = &explicit.sections[bss_index];
    assert_eq!((bss.nobits_size, bss.sh_addralign), (9, 1));
    let symbol = explicit
        .symbols
        .iter()
        .find(|symbol| symbol.name == "local_zero")
        .expect("local COMMON symbol");
    assert_eq!(
        (symbol.place, symbol.value, symbol.size),
        (SymbolPlace::Section(bss_index), 1, 8)
    );
}

#[test]
fn explicit_zero_common_alignment_matches_gnu_as() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_common_zero_alignment",
            "explicit_zero_common_alignment_matches_gnu_as",
            "no GNU assembler on this host",
        );
        return;
    };
    let artifacts = celf::TempArtifacts::new("afs_x86_common_zero_alignment");
    let source_path = artifacts.path(".s");
    let object_path = artifacts.path("_gas.o");
    let source = format!("{LOCAL_ZERO}{GLOBAL_ZERO}");
    std::fs::write(&source_path, &source).expect("write GNU as input");
    celf::assemble_with_gas(&gas, &source_path, &object_path);

    let gas_object = parse_elf(&std::fs::read(&object_path).expect("read GNU as object"))
        .expect("parse GNU as object");
    let ours = assemble_x86(&source, host_osabi()).expect("assemble explicit zero alignments");

    assert_eq!(celf::normalize(&ours), celf::normalize(&gas_object));
}

#[test]
fn negative_common_alignment_remains_rejected() {
    let error = assemble_x86(".comm item,8,-1\n", host_osabi())
        .expect_err("negative COMMON alignment unexpectedly assembled");

    assert_eq!(error.line, Some(1));
    assert_eq!(error.col, Some(1));
    assert_eq!(error.msg, "bad .comm align in 'item,8,-1'");
}
