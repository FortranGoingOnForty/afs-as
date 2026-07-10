//! x13 smoke: system tooling accepts writer output.
//!
//! Deeper coverage (system-corpus struct round-trip, differential
//! policy comparison) lives in elf_system_corpus.rs and
//! elf_differential.rs; this suite is the fast canary that readelf
//! parses a writer-produced object and the system linker accepts it.

use std::process::Command;

use afs_as::elf::{
    self, reloc::x86_64::*, ObjectFile, Rela, Section, Symbol, SymbolPlace, ELFOSABI_FREEBSD,
    ELFOSABI_NONE, EM_X86_64, STB_GLOBAL, STB_LOCAL, STT_FUNC, STT_OBJECT, STV_DEFAULT,
};

fn host_osabi() -> u8 {
    if cfg!(target_os = "freebsd") {
        ELFOSABI_FREEBSD
    } else {
        ELFOSABI_NONE
    }
}

fn skip_reason() -> Option<String> {
    if !cfg!(any(target_os = "freebsd", target_os = "linux")) {
        return Some("ELF smoke needs an ELF host".into());
    }
    if Command::new("readelf").arg("--version").output().is_err() {
        return Some("no readelf on PATH".into());
    }
    None
}

fn sample() -> ObjectFile {
    let mut obj = ObjectFile::new(EM_X86_64, host_osabi());
    let mut text = Section::text();
    // f: call ext; ret  — PLT32 with the gas addend bytes in place.
    text.data = vec![0xe8, 0xfc, 0xff, 0xff, 0xff, 0xc3];
    text.relas.push(Rela {
        offset: 1,
        symbol: 1,
        r_type: R_X86_64_PLT32,
        addend: -4,
    });
    let mut data = Section::data();
    data.data = vec![0u8; 8];
    data.relas.push(Rela {
        offset: 0,
        symbol: 0,
        r_type: R_X86_64_64,
        addend: 0,
    });
    obj.sections.push(text);
    obj.sections.push(data);
    obj.symbols.push(Symbol {
        name: "f".into(),
        bind: STB_GLOBAL,
        typ: STT_FUNC,
        vis: STV_DEFAULT,
        place: SymbolPlace::Section(0),
        value: 0,
        size: 6,
    });
    obj.symbols.push(Symbol {
        name: "ext".into(),
        bind: STB_GLOBAL,
        typ: elf::STT_NOTYPE,
        vis: STV_DEFAULT,
        place: SymbolPlace::Undef,
        value: 0,
        size: 0,
    });
    obj.symbols.push(Symbol {
        name: "lcl".into(),
        bind: STB_LOCAL,
        typ: STT_OBJECT,
        vis: STV_DEFAULT,
        place: SymbolPlace::Section(1),
        value: 0,
        size: 8,
    });
    obj
}

#[test]
fn readelf_parses_writer_output() {
    if let Some(reason) = skip_reason() {
        eprintln!("\nHARNESS_SKIP suite=elf_smoke test=readelf_parses_writer_output count=1 reason=\"{}\"", reason);
        return;
    }
    let bytes = elf::write_elf(&sample()).expect("write");
    let dir = std::env::temp_dir();
    let obj_path = dir.join(format!("afs_elf_smoke_{}.o", std::process::id()));
    std::fs::write(&obj_path, &bytes).expect("write obj");

    let out = Command::new("readelf")
        .arg("-a")
        .arg(&obj_path)
        .output()
        .expect("run readelf");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "readelf rejected object:\n{}", text);
    for needle in [
        "REL (Relocatable file)",
        ".text",
        ".rela.text",
        ".symtab",
        "R_X86_64_PLT32",
        "R_X86_64_64",
        ".note.GNU-stack",
    ] {
        assert!(
            text.contains(needle),
            "missing {:?} in readelf -a:\n{}",
            needle,
            text
        );
    }
    // No complaints about malformed structures.
    for bad in ["Error", "corrupt", "Warning"] {
        assert!(!text.contains(bad), "readelf flagged {:?}:\n{}", bad, text);
    }
    let _ = std::fs::remove_file(&obj_path);
}

#[test]
fn system_linker_accepts_relocatable_link() {
    if let Some(reason) = skip_reason() {
        eprintln!("\nHARNESS_SKIP suite=elf_smoke test=system_linker_accepts_relocatable_link count=1 reason=\"{}\"", reason);
        return;
    }
    if Command::new("ld").arg("--version").output().is_err() {
        eprintln!("\nHARNESS_SKIP suite=elf_smoke test=system_linker_accepts_relocatable_link count=1 reason=\"no ld on PATH\"");
        return;
    }
    let bytes = elf::write_elf(&sample()).expect("write");
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj_path = dir.join(format!("afs_elf_smoke_ld_{}.o", pid));
    let out_path = dir.join(format!("afs_elf_smoke_ld_{}_out.o", pid));
    std::fs::write(&obj_path, &bytes).expect("write obj");

    let out = Command::new("ld")
        .arg("-r")
        .arg("-o")
        .arg(&out_path)
        .arg(&obj_path)
        .output()
        .expect("run ld");
    assert!(
        out.status.success(),
        "ld -r rejected writer output:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_file(&obj_path);
    let _ = std::fs::remove_file(&out_path);
}
