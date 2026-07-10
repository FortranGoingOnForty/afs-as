//! x13: writer-vs-gas differential under the sprint's comparison
//! policy, on the checked-in corpus of armfortas backend output.
//!
//! afs-as has no x86 assembler until x14, so the logical object comes
//! from LIFTING gas's .o through our reader; the writer re-emits it
//! and both objects are compared in normalized form: per-section
//! content bytes and type/flags, relocation tuples with symbol names,
//! and the symbol set. Explicitly not compared: sh_offset, section
//! and symbol order, e_shnum, padding, .comment, .note.* (dropped by
//! the reader on both sides).
//!
//! The link gate goes further: a freestanding _start program is
//! assembled by gas, lifted, re-emitted, and BOTH objects are linked
//! by every available system linker and run — same exit code both
//! ways.

#[path = "common/elf.rs"]
mod celf;

use std::path::PathBuf;
use std::process::Command;

use afs_as::elf::{parse_elf, reloc::x86_64::*, write_elf};

#[test]
fn corpus_lift_reemit_matches_gas_under_policy() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "elf_differential",
            "corpus_lift_reemit_matches_gas_under_policy",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_elf_diff");
    for src in celf::corpus_files() {
        let stem = src.file_stem().unwrap().to_string_lossy().to_string();
        let obj_path = tmp.path(&format!("_{}.o", stem));
        celf::assemble_with_gas(&gas, &src, &obj_path);
        let gas_bytes = std::fs::read(&obj_path).expect("read gas object");

        let lifted = parse_elf(&gas_bytes)
            .unwrap_or_else(|e| panic!("{}: reader rejects gas object: {}", src.display(), e));
        let ours = write_elf(&lifted)
            .unwrap_or_else(|e| panic!("{}: writer rejects lifted object: {}", src.display(), e));
        let reparsed = parse_elf(&ours)
            .unwrap_or_else(|e| panic!("{}: reader rejects our re-emission: {}", src.display(), e));

        let a = celf::normalize(&lifted);
        let b = celf::normalize(&reparsed);
        assert_eq!(
            a,
            b,
            "{}: normalized objects diverge between gas and re-emission",
            src.display()
        );
    }
}

#[test]
fn corpus_covers_every_emit_set_reloc_type() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "elf_differential",
            "corpus_covers_every_emit_set_reloc_type",
            "no GNU assembler on this host",
        );
        return;
    };
    // The x13 DoD: at least one corpus fixture per relocation type in
    // the emit set (PC32, PLT32, 64). Adding an emit type without a
    // fixture fails here by checklist.
    let tmp = celf::TempArtifacts::new("afs_elf_cover");
    let mut seen: Vec<u32> = Vec::new();
    for src in celf::corpus_files() {
        let obj_path = tmp.path(&format!(
            "_{}.o",
            src.file_stem().unwrap().to_string_lossy()
        ));
        celf::assemble_with_gas(&gas, &src, &obj_path);
        let obj = parse_elf(&std::fs::read(&obj_path).unwrap()).expect("lift");
        for sec in &obj.sections {
            for r in &sec.relas {
                if !seen.contains(&r.r_type) {
                    seen.push(r.r_type);
                }
            }
        }
    }
    for (needed, name) in [
        (R_X86_64_PC32, "R_X86_64_PC32"),
        (R_X86_64_PLT32, "R_X86_64_PLT32"),
        (R_X86_64_64, "R_X86_64_64"),
    ] {
        assert!(
            seen.contains(&needed),
            "corpus lacks a fixture producing {} (saw {:?})",
            name,
            seen
        );
    }
}

fn linkers() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut candidates: Vec<&str> = vec!["ld"];
    if cfg!(target_os = "freebsd") {
        // Base ld is LLD; the binutils package adds GNU ld.
        candidates.push("/usr/local/bin/ld");
    }
    for cand in candidates {
        if let Ok(out) = Command::new(cand).arg("--version").output() {
            if out.status.success() {
                found.push(PathBuf::from(cand));
            }
        }
    }
    found
}

#[test]
fn freestanding_program_links_and_runs_identically() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "elf_differential",
            "freestanding_program_links_and_runs_identically",
            "no GNU assembler on this host",
        );
        return;
    };
    let links = linkers();
    if links.is_empty() {
        celf::skip(
            "elf_differential",
            "freestanding_program_links_and_runs_identically",
            "no system linker on PATH",
        );
        return;
    }

    // exit(42) via raw syscall so no runtime is needed. The exit
    // syscall number differs per OS; the register convention does not.
    let exit_nr = if cfg!(target_os = "freebsd") { 1 } else { 60 };
    let asm = format!(
        ".text\n.globl _start\n.type _start,@function\n_start:\n    movl $42, %edi\n    movl ${}, %eax\n    syscall\n.size _start,.-_start\n",
        exit_nr
    );

    let tmp = celf::TempArtifacts::new("afs_elf_run");
    let src_path = tmp.path(".s");
    std::fs::write(&src_path, &asm).expect("write asm");
    let gas_obj = tmp.path("_gas.o");
    celf::assemble_with_gas(&gas, &src_path, &gas_obj);

    let lifted = parse_elf(&std::fs::read(&gas_obj).unwrap()).expect("lift");
    let ours_obj = tmp.path("_ours.o");
    std::fs::write(&ours_obj, write_elf(&lifted).expect("re-emit")).expect("write ours");

    for ld in &links {
        for (label, obj) in [("gas", &gas_obj), ("ours", &ours_obj)] {
            let bin = tmp.path(&format!(
                "_{}_{}",
                label,
                ld.file_name().unwrap().to_string_lossy()
            ));
            let out = Command::new(ld)
                .arg("-o")
                .arg(&bin)
                .arg(obj)
                .output()
                .expect("run linker");
            assert!(
                out.status.success(),
                "{} rejected {} object:\n{}",
                ld.display(),
                label,
                String::from_utf8_lossy(&out.stderr)
            );
            let run = Command::new(&bin).output().expect("run binary");
            assert_eq!(
                run.status.code(),
                Some(42),
                "{} binary linked by {} exited {:?}",
                label,
                ld.display(),
                run.status.code()
            );
        }
    }
}
