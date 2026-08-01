#[path = "common/elf.rs"]
mod celf;

use std::io::Write;
use std::process::{Command, Stdio};

use afs_as::elf::{parse_elf, SymbolPlace, ELFOSABI_FREEBSD, ELFOSABI_NONE};
use afs_as::x86::assemble::assemble_x86;

const SOURCE: &str = ".text\n\
.globl entry\n\
.type entry,@function\n\
entry:\n\
  ret\n\
.size entry, .-entry\n\
.local alpha\n\
.local bravo\n\
.local charlie\n\
.local delta\n\
.local echo\n\
.local foxtrot\n\
.local golf\n\
.local hotel\n\
.local india\n\
.local juliet\n\
.local kilo\n\
.local lima\n\
.comm golf,8,8\n\
.comm alpha,8,8\n\
.comm lima,8,8\n\
.comm bravo,8,8\n\
.comm hotel,8,8\n\
.comm charlie,8,8\n\
.comm kilo,8,8\n\
.comm delta,8,8\n\
.comm india,8,8\n\
.comm echo,8,8\n\
.comm juliet,8,8\n\
.comm foxtrot,8,8\n";

const COMMON_NAMES: [&str; 12] = [
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet",
    "kilo", "lima",
];

const EXPECTED_METADATA: [(&str, u64, u64); 12] = [
    ("alpha", 8, 8),
    ("bravo", 24, 8),
    ("charlie", 40, 8),
    ("delta", 56, 8),
    ("echo", 72, 8),
    ("foxtrot", 88, 8),
    ("golf", 0, 8),
    ("hotel", 32, 8),
    ("india", 64, 8),
    ("juliet", 80, 8),
    ("kilo", 48, 8),
    ("lima", 16, 8),
];

const MIXED_DECLARATIONS: &str = ".type alpha,@object\n\
.size bravo,8\n\
.extern charlie\n\
.local delta\n\
.local alpha\n\
.local bravo\n\
.local charlie\n\
.comm delta,8,8\n\
.comm charlie,8,8\n\
.comm bravo,8,8\n\
.comm alpha,8,8\n";

const MIXED_EXPECTED_METADATA: [(&str, u64, u64); 4] = [
    ("alpha", 24, 8),
    ("bravo", 16, 8),
    ("delta", 0, 8),
    ("charlie", 8, 8),
];

fn host_osabi() -> u8 {
    if cfg!(target_os = "freebsd") {
        ELFOSABI_FREEBSD
    } else {
        ELFOSABI_NONE
    }
}

fn common_metadata(obj: &afs_as::elf::ObjectFile) -> Vec<(&str, u64, u64)> {
    obj.symbols
        .iter()
        .filter(|symbol| COMMON_NAMES.contains(&symbol.name.as_str()))
        .map(|symbol| (symbol.name.as_str(), symbol.value, symbol.size))
        .collect()
}

#[test]
fn local_common_order_matches_gas() {
    let ours = assemble_x86(SOURCE, host_osabi()).expect("assemble source");
    assert_eq!(common_metadata(&ours), EXPECTED_METADATA);

    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_common_determinism",
            "local_common_order_matches_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_common_order");
    let source_path = tmp.path(".s");
    let object_path = tmp.path(".o");
    std::fs::write(&source_path, SOURCE).expect("write source");
    celf::assemble_with_gas(&gas, &source_path, &object_path);

    let gas_obj = parse_elf(&std::fs::read(&object_path).expect("read gas object"))
        .expect("parse gas object");

    assert_eq!(common_metadata(&ours), common_metadata(&gas_obj));
}

#[test]
fn symbol_creating_directives_set_local_common_order() {
    let ours = assemble_x86(MIXED_DECLARATIONS, host_osabi()).expect("assemble source");
    assert_eq!(common_metadata(&ours), MIXED_EXPECTED_METADATA);

    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_common_determinism",
            "symbol_creating_directives_set_local_common_order",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_common_declaration_order");
    let source_path = tmp.path(".s");
    let object_path = tmp.path(".o");
    std::fs::write(&source_path, MIXED_DECLARATIONS).expect("write source");
    celf::assemble_with_gas(&gas, &source_path, &object_path);

    let gas_obj = parse_elf(&std::fs::read(&object_path).expect("read gas object"))
        .expect("parse gas object");
    assert_eq!(common_metadata(&ours), common_metadata(&gas_obj));
}

#[test]
fn duplicate_local_commons_keep_one_final_allocation_symbol() {
    let source = ".local duplicate\n.comm duplicate,8,8\n.comm duplicate,3,16\n";
    let obj = assemble_x86(source, host_osabi()).expect("assemble duplicate common");
    let bss = obj.section_by_name(".bss").expect("bss section");
    assert_eq!((bss.nobits_size, bss.sh_addralign), (19, 16));

    let symbols: Vec<_> = obj
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "duplicate")
        .collect();
    assert_eq!(symbols.len(), 1);
    assert_eq!((symbols[0].value, symbols[0].size), (16, 3));
}

#[test]
fn local_common_alignment_must_be_a_power_of_two() {
    let src = ".local oddalign\n    .comm oddalign,8,3\n";
    let err = assemble_x86(src, host_osabi()).expect_err("odd local COMMON alignment assembled");

    assert_eq!(err.line, Some(2));
    assert_eq!(err.col, Some(5));
    assert_eq!(
        err.msg,
        "local COMMON 'oddalign' alignment 3 is not a power of two"
    );
}

#[test]
fn valid_local_and_non_power_of_two_global_common_alignments_are_preserved() {
    let src =
        ".bss\n.byte 0\n.local local_aligned\n.comm local_aligned,8,32\n.comm global_unusual,8,3\n";
    let obj = assemble_x86(src, host_osabi()).expect("assemble valid COMMON alignments");
    let bss = obj.section_by_name(".bss").expect("bss section");
    assert_eq!((bss.nobits_size, bss.sh_addralign), (40, 32));
    let bss_index = obj
        .sections
        .iter()
        .position(|section| section.name == ".bss")
        .expect("bss section index");

    let local = obj
        .symbols
        .iter()
        .find(|symbol| symbol.name == "local_aligned")
        .expect("local COMMON symbol");
    assert_eq!(
        (local.place, local.value, local.size),
        (SymbolPlace::Section(bss_index), 32, 8)
    );

    let global = obj
        .symbols
        .iter()
        .find(|symbol| symbol.name == "global_unusual")
        .expect("global COMMON symbol");
    assert_eq!(
        (global.place, global.value, global.size),
        (SymbolPlace::Common, 3, 8)
    );
}

#[test]
fn gas_matches_local_common_alignment_rules() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_common_determinism",
            "gas_matches_local_common_alignment_rules",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_common_alignment");

    for (index, (src, accepted)) in [
        (".local x\n.comm x,8,2\n", true),
        (".local x\n.comm x,8,3\n", false),
        (".local x\n.comm x,8,4\n", true),
        (".comm x,8,3\n", true),
    ]
    .iter()
    .enumerate()
    {
        let source_path = tmp.path(&format!("_{index}.s"));
        let object_path = tmp.path(&format!("_{index}.o"));
        std::fs::write(&source_path, src).expect("write source");
        let output = Command::new(&gas)
            .arg("--64")
            .arg("-o")
            .arg(&object_path)
            .arg(&source_path)
            .output()
            .expect("run gas");
        assert_eq!(
            output.status.success(),
            *accepted,
            "unexpected gas result for {src:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn repeated_global_commons_emit_one_symbol_and_relocation_target() {
    let source = ".comm duplicate,8,8\n\
                  .comm duplicate,3,16\n\
                  .data\n\
                  .quad duplicate\n";
    let obj = assemble_x86(source, host_osabi()).expect("assemble repeated global common");
    let symbols: Vec<_> = obj
        .symbols
        .iter()
        .enumerate()
        .filter(|(_, symbol)| symbol.name == "duplicate")
        .collect();

    assert_eq!(symbols.len(), 1, "COMMON must have one symbol-table entry");
    let (symbol_index, symbol) = symbols[0];
    assert_eq!((symbol.value, symbol.size), (16, 8));

    let data = obj.section_by_name(".data").expect("data section");
    assert_eq!(data.relas.len(), 1);
    assert_eq!(data.relas[0].symbol, symbol_index);
}

#[test]
fn repeated_global_commons_preserve_order_and_gnu_size_rules() {
    let source = ".comm alpha,0,1\n\
                  .comm bravo,7,4\n\
                  .comm alpha,6,32\n\
                  .comm bravo,0,16\n\
                  .comm charlie,4,2\n\
                  .comm charlie,4,8\n";
    let obj = assemble_x86(source, host_osabi()).expect("assemble repeated global commons");
    let metadata: Vec<_> = obj
        .symbols
        .iter()
        .filter(|symbol| matches!(symbol.place, afs_as::elf::SymbolPlace::Common))
        .map(|symbol| (symbol.name.as_str(), symbol.value, symbol.size))
        .collect();

    assert_eq!(
        metadata,
        [("alpha", 32, 6), ("bravo", 16, 7), ("charlie", 8, 4)]
    );
}

#[test]
fn local_common_objects_are_stable_across_fresh_processes() {
    let tmp = celf::TempArtifacts::new("afs_x86_common_determinism");
    let mut expected = None;

    for run in 0..30 {
        let object_path = tmp.path(&format!("_{run}.o"));
        let mut child = Command::new(env!("CARGO_BIN_EXE_afs-as"))
            .args(["--64", "-", "-o"])
            .arg(&object_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn afs-as");
        child
            .stdin
            .as_mut()
            .expect("stdin pipe")
            .write_all(SOURCE.as_bytes())
            .expect("write assembly source");
        let output = child.wait_with_output().expect("wait for afs-as");
        assert!(
            output.status.success(),
            "run {run} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let bytes = std::fs::read(&object_path).expect("read assembled object");
        if let Some(expected) = &expected {
            assert_eq!(&bytes, expected, "object bytes changed on run {run}");
        } else {
            expected = Some(bytes);
        }
    }
}
