//! x14: whole-file assembler differential vs gas.
//!
//! Our full pipeline (parse → encode → relax → elf writer) runs on
//! each checked-in corpus fixture (real armfortas backend output) and
//! must produce a .text byte-identical to gas plus policy-equal
//! relocations/symbols/sections. Branch-relaxation parity gets its
//! own targeted cases: short/long forward/backward jumps around
//! padding, at both sides of the rel8 boundary.

#[path = "common/elf.rs"]
mod celf;

use afs_as::elf::{
    parse_elf, SymbolPlace, ELFOSABI_FREEBSD, ELFOSABI_NONE, SHF_EXECINSTR, STB_GLOBAL, STB_WEAK,
    STT_FUNC, STT_NOTYPE, STT_OBJECT,
};
use afs_as::x86::assemble::assemble_x86;

fn host_osabi() -> u8 {
    if cfg!(target_os = "freebsd") {
        ELFOSABI_FREEBSD
    } else {
        ELFOSABI_NONE
    }
}

fn diff_one(
    name: &str,
    src: &str,
    gas: &std::path::Path,
    tmp: &celf::TempArtifacts,
) -> Option<String> {
    let src_path = tmp.path(&format!("_{}.s", name));
    let obj_path = tmp.path(&format!("_{}.o", name));
    std::fs::write(&src_path, src).unwrap();
    celf::assemble_with_gas(gas, &src_path, &obj_path);
    let gas_obj = parse_elf(&std::fs::read(&obj_path).unwrap()).expect("lift gas");

    let ours = match assemble_x86(src, host_osabi()) {
        Ok(o) => o,
        Err(e) => return Some(format!("{}: our assembler failed: {}", name, e)),
    };

    // .text byte identity (the strong check), modulo NOP-fill split
    // order which differs across binutils versions.
    let gas_text = gas_obj
        .section_by_name(".text")
        .map(|s| celf::canonicalize_nop_fill(&s.data));
    let our_text = ours
        .section_by_name(".text")
        .map(|s| celf::canonicalize_nop_fill(&s.data));
    if gas_text != our_text {
        let (g, o) = (gas_text.unwrap_or_default(), our_text.unwrap_or_default());
        let first_diff = g
            .iter()
            .zip(o.iter())
            .position(|(a, b)| a != b)
            .unwrap_or(g.len().min(o.len()));
        return Some(format!(
            "{}: .text diverges at byte {} (gas len {}, ours {})\n  gas:  {:02x?}\n  ours: {:02x?}",
            name,
            first_diff,
            g.len(),
            o.len(),
            &g[first_diff.saturating_sub(8)..(first_diff + 8).min(g.len())],
            &o[first_diff.saturating_sub(8)..(first_diff + 8).min(o.len())],
        ));
    }

    // Policy comparison for everything else.
    let a = celf::normalize(&gas_obj);
    let b = celf::normalize(&ours);
    if a.sections != b.sections {
        return Some(format!(
            "{}: sections diverge\n  gas:  {:?}\n  ours: {:?}",
            name,
            a.sections.keys().collect::<Vec<_>>(),
            b.sections.keys().collect::<Vec<_>>()
        ));
    }
    if a.relocs != b.relocs {
        return Some(format!(
            "{}: relocs diverge\n  gas:  {:?}\n  ours: {:?}",
            name, a.relocs, b.relocs
        ));
    }
    if a.symbols != b.symbols {
        return Some(format!(
            "{}: symbols diverge\n  gas:  {:?}\n  ours: {:?}",
            name, a.symbols, b.symbols
        ));
    }
    None
}

#[test]
fn corpus_fixtures_assemble_byte_identical_to_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "corpus_fixtures_assemble_byte_identical_to_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_asmdiff");
    let mut failures = Vec::new();
    for path in celf::corpus_files() {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&path).unwrap();
        if let Some(f) = diff_one(&name, &src, &gas, &tmp) {
            failures.push(f);
        }
    }
    assert!(
        failures.is_empty(),
        "{} fixtures diverge:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn branch_relaxation_matches_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "branch_relaxation_matches_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let pad = |n: usize| "    nop\n".repeat(n);
    let cases: Vec<(&str, String)> = vec![
        (
            "short_fwd",
            format!(".text\nf:\n    jmp .Lx\n{}.Lx:\n    ret\n", pad(10)),
        ),
        (
            "short_back",
            format!(".text\nf:\n.Lx:\n{}    jne .Lx\n    ret\n", pad(10)),
        ),
        // 130 nops force rel32 forward.
        (
            "long_fwd",
            format!(".text\nf:\n    jmp .Lx\n{}.Lx:\n    ret\n", pad(130)),
        ),
        (
            "long_back",
            format!(".text\nf:\n.Lx:\n{}    jg .Lx\n    ret\n", pad(130)),
        ),
        // Exactly at the rel8 boundary either side (127 / 128 bytes).
        (
            "edge_127",
            format!(".text\nf:\n    jmp .Lx\n{}.Lx:\n    ret\n", pad(127)),
        ),
        (
            "edge_128",
            format!(".text\nf:\n    jmp .Lx\n{}.Lx:\n    ret\n", pad(128)),
        ),
        // Chained: an early branch whose target moves when a later
        // branch grows — the fixed-point case.
        (
            "cascade",
            format!(
                ".text\nf:\n    jmp .La\n{}\n    jmp .Lb\n{}.La:\n    nop\n.Lb:\n    ret\n",
                pad(120),
                pad(120)
            ),
        ),
        // Conditional variants.
        (
            "jcc_mix",
            format!(
                ".text\nf:\n    je .Lx\n    jne .Ly\n{}.Lx:\n    nop\n{}.Ly:\n    ret\n",
                pad(100),
                pad(100)
            ),
        ),
        (
            "strong_global_jmp",
            ".text\n.globl foo\nfoo:\n    ret\n.globl caller\ncaller:\n    jmp foo\n".into(),
        ),
        (
            "weak_jmp",
            ".text\n.weak foo\nfoo:\n    ret\n.globl caller\ncaller:\n    jmp foo\n".into(),
        ),
        (
            "weak_jcc",
            ".text\n.weak foo\nfoo:\n    ret\n.globl caller\ncaller:\n    je foo\n".into(),
        ),
    ];
    let tmp = celf::TempArtifacts::new("afs_x86_relax");
    let mut failures = Vec::new();
    for (name, src) in &cases {
        if let Some(f) = diff_one(name, src, &gas, &tmp) {
            failures.push(f);
        }
    }
    assert!(
        failures.is_empty(),
        "{} relaxation cases diverge:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn large_bss_space_matches_gas_without_materializing() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "large_bss_space_matches_gas_without_materializing",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_bss_virtual");
    let src = ".bss\nscratch:\n.space 4294967299\n.p2align 4\ntail:\n.byte 0\n";
    if let Some(f) = diff_one("large_bss_space", src, &gas, &tmp) {
        panic!("{f}");
    }
}

#[test]
fn explicit_fill_bytes_match_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "explicit_fill_bytes_match_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_space_fill");
    let src = ".text\n.globl f\nf:\n.byte 0\n.p2align 2,,3\n.byte 0\n.p2align 2,0xcc\n.space 4,0x90\n.skip 3,0xab\nret\n.data\nd:\n.byte 0\n.p2align 2,0x5a\n.space 4,0x7f\n";
    if let Some(f) = diff_one("explicit_fill", src, &gas, &tmp) {
        panic!("{f}");
    }
}

#[test]
fn subsection_layout_matches_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "subsection_layout_matches_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_subsections");
    let src = ".text 7\n\
               text7a: .byte 0x70\n\
               .text 0\n\
               text0: jmp text7a\n\
               .text 3\n\
               .p2align 2\n\
               text3: .byte 0x30\n\
               .text 7\n\
               text7b: ret\n\
               .data 5\n\
               data5: .quad text3\n\
               .data 0\n\
               data0: .byte 0x10\n\
               .data 2\n\
               data2: .byte 0x20\n\
               .bss 4\n\
               bss4: .zero 1\n\
               .bss 0\n\
               bss0: .zero 2\n\
               .bss 2\n\
               bss2: .zero 3\n";
    if let Some(failure) = diff_one("subsection_layout", src, &gas, &tmp) {
        panic!("{failure}");
    }
}

#[test]
fn zero_fill_bytes_match_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "zero_fill_bytes_match_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_zero_fill");
    let src = ".data\n.zero 4,0xa5\n.zero 2\n.zero 3,-1\n.zero 1,\n";
    if let Some(failure) = diff_one("zero_fill", src, &gas, &tmp) {
        panic!("{failure}");
    }
}

#[test]
fn string_operand_groups_match_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "string_operand_groups_match_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_string_operands");
    let src = ".data\n\
               .ascii \"A\",\"B,C\"\n\
               .ascii ,\"D\" \"E\",,\"F\",\n\
               .asciz \"G\",\"H\"\n\
               .asciz ,\"I\" \"J\",,\"\",\n\
               .string \"K\" \"L\",\"M\"\n";
    if let Some(failure) = diff_one("string_operands", src, &gas, &tmp) {
        panic!("{failure}");
    }
}

#[test]
fn default_common_alignment_matches_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "default_common_alignment_matches_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_common_alignment");
    let src = ".text\nret\n.comm c0,0\n.comm c1,1\n.comm c2,2\n.comm c3,3\n.comm c5,5\n.comm c9,9\n.comm c16,16\n.comm c32,32\n";
    if let Some(f) = diff_one("default_common_alignment", src, &gas, &tmp) {
        panic!("{f}");
    }
}

#[test]
fn repeated_global_commons_match_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "repeated_global_commons_match_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_repeated_global_common");
    let src = ".comm duplicate,8,8\n\
               .comm duplicate,3,16\n\
               .comm from_zero,0,1\n\
               .comm from_zero,5,32\n\
               .comm equal_size,4,2\n\
               .comm equal_size,4,8\n\
               .data\n\
               .quad duplicate,from_zero,equal_size\n";
    if let Some(failure) = diff_one("repeated_global_common", src, &gas, &tmp) {
        panic!("{failure}");
    }
}

#[test]
fn exported_dot_l_symbols_match_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "exported_dot_l_symbols_match_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let cases = [
        (
            "global_dot_l_call",
            ".text\n.globl .Lfoo\ncaller: call .Lfoo\n.Lfoo: ret\n",
        ),
        (
            "global_dot_l_jmp",
            ".text\n.globl .Lfoo\ncaller: jmp .Lfoo\n.Lfoo: ret\n",
        ),
        (
            "weak_dot_l_call",
            ".text\n.weak .Lfoo\ncaller: call .Lfoo\n.Lfoo: ret\n",
        ),
        (
            "weak_dot_l_jmp",
            ".text\n.weak .Lfoo\ncaller: jmp .Lfoo\n.Lfoo: ret\n",
        ),
    ];
    let tmp = celf::TempArtifacts::new("afs_x86_exported_dot_l");
    let mut failures = Vec::new();
    for (name, src) in cases {
        if let Some(failure) = diff_one(name, src, &gas, &tmp) {
            failures.push(failure);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

const UNDEFINED_SYMBOL_METADATA: &str = ".text
.globl globl_only
.globl ext_func
.type ext_func,@function
call ext_func
.extern extern_used
.extern extern_unused
call extern_used
.type typed_only,@function
.size sized_only,7
base:
.byte 1
.globl dotted
.type dotted,@function
.size dotted,.-base
.globl wrapped
.size wrapped,.-later
.byte 1
later:
.data
.weak ext_obj
.type ext_obj,@object
.size ext_obj,16
.quad ext_obj
.weak weak_unused
.type weak_unused,@object
.size weak_unused,8
";

#[test]
fn undefined_symbol_metadata_is_preserved() {
    let obj = assemble_x86(UNDEFINED_SYMBOL_METADATA, host_osabi()).expect("assemble metadata");
    let symbol = |name: &str| {
        obj.symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap_or_else(|| panic!("missing symbol '{name}'"))
    };

    for (name, bind, typ, size) in [
        ("globl_only", STB_GLOBAL, STT_NOTYPE, 0),
        ("ext_func", STB_GLOBAL, STT_FUNC, 0),
        ("extern_used", STB_GLOBAL, STT_NOTYPE, 0),
        ("typed_only", STB_GLOBAL, STT_FUNC, 0),
        ("sized_only", STB_GLOBAL, STT_NOTYPE, 7),
        ("dotted", STB_GLOBAL, STT_FUNC, 1),
        ("wrapped", STB_GLOBAL, STT_NOTYPE, u64::MAX),
        ("ext_obj", STB_WEAK, STT_OBJECT, 16),
    ] {
        let actual = symbol(name);
        assert_eq!(actual.bind, bind, "binding for {name}");
        assert_eq!(actual.typ, typ, "type for {name}");
        assert_eq!(actual.place, SymbolPlace::Undef, "placement for {name}");
        assert_eq!(actual.value, 0, "value for {name}");
        assert_eq!(actual.size, size, "size for {name}");
    }

    for name in ["extern_unused", "weak_unused"] {
        assert!(
            obj.symbols.iter().all(|symbol| symbol.name != name),
            "GNU as omits unreferenced {name} declarations"
        );
    }
}

#[test]
fn undefined_symbol_metadata_matches_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "undefined_symbol_metadata_matches_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_undefined_metadata");
    if let Some(failure) = diff_one(
        "undefined_symbol_metadata",
        UNDEFINED_SYMBOL_METADATA,
        &gas,
        &tmp,
    ) {
        panic!("{failure}");
    }
}

#[test]
fn gnu_stack_intent_matches_gas() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_assemble_differential",
            "gnu_stack_intent_matches_gas",
            "no GNU assembler on this host",
        );
        return;
    };
    let cases = [
        ("absent", ".text\nret\n", None),
        (
            "non_executable",
            ".text\nret\n.section .note.GNU-stack,\"\",@progbits\n",
            Some(0),
        ),
        (
            "executable",
            ".text\nret\n.section .note.GNU-stack,\"x\",@progbits\n",
            Some(SHF_EXECINSTR),
        ),
        (
            "first_non_executable",
            ".section .note.GNU-stack,\"\",@progbits\n\
             .section .note.GNU-stack,\"x\",@progbits\n",
            Some(0),
        ),
        (
            "first_executable",
            ".section .note.GNU-stack,\"x\",@progbits\n\
             .section .note.GNU-stack,\"\",@progbits\n",
            Some(SHF_EXECINSTR),
        ),
    ];
    let tmp = celf::TempArtifacts::new("afs_x86_gnu_stack");
    let mut failures = Vec::new();
    for (name, src, expected) in cases {
        let ours = assemble_x86(src, host_osabi()).expect("assemble GNU-stack case");
        assert_eq!(ours.gnu_stack_flags, expected, "stack intent for {name}");
        if let Some(failure) = diff_one(name, src, &gas, &tmp) {
            failures.push(failure);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
