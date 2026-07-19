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

use afs_as::elf::{parse_elf, ELFOSABI_FREEBSD, ELFOSABI_NONE};
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
    let src = ".text\n.globl f\nf:\n.byte 0\n.p2align 2,0xcc\n.space 4,0x90\n.skip 3,0xab\nret\n.data\nd:\n.byte 0\n.p2align 2,0x5a\n.space 4,0x7f\n";
    if let Some(f) = diff_one("explicit_fill", src, &gas, &tmp) {
        panic!("{f}");
    }
}
