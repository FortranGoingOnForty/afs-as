//! x13: struct-level round-trip against system-gas objects.
//!
//! For every corpus fixture (harvested armfortas x86_64 backend
//! output): assemble with gas, then assert `write(parse(bytes)) ==
//! bytes` for each wire record — ehdr, every shdr, every symtab
//! entry, every rela entry. This pins our field layout to what gas
//! actually writes, record by record.

#[path = "common/elf.rs"]
mod celf;

use afs_as::elf::{
    Elf64Ehdr, Elf64Rela, Elf64Shdr, Elf64Sym, EHDR_SIZE, RELA_SIZE, SHDR_SIZE, SHT_RELA,
    SHT_SYMTAB, SYM_SIZE,
};

#[test]
fn every_gas_record_roundtrips_bytewise() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "elf_system_corpus",
            "every_gas_record_roundtrips_bytewise",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_elf_syscorpus");
    for src in celf::corpus_files() {
        let obj_path = tmp.path(&format!(
            "_{}.o",
            src.file_stem().unwrap().to_string_lossy()
        ));
        celf::assemble_with_gas(&gas, &src, &obj_path);
        let bytes = std::fs::read(&obj_path).expect("read gas object");

        // ehdr
        let ehdr = Elf64Ehdr::parse(&bytes).expect("parse ehdr");
        let mut rewritten = Vec::new();
        ehdr.write(&mut rewritten);
        assert_eq!(
            rewritten,
            &bytes[..EHDR_SIZE],
            "{}: ehdr record does not round-trip",
            src.display()
        );

        // every shdr
        let shoff = ehdr.e_shoff as usize;
        for i in 0..ehdr.e_shnum as usize {
            let off = shoff + i * SHDR_SIZE;
            let sh = Elf64Shdr::parse(&bytes, off).expect("parse shdr");
            let mut w = Vec::new();
            sh.write(&mut w);
            assert_eq!(
                w,
                &bytes[off..off + SHDR_SIZE],
                "{}: shdr {} does not round-trip",
                src.display(),
                i
            );

            // symtab / rela records inside this section
            let body_start = sh.sh_offset as usize;
            match sh.sh_type {
                SHT_SYMTAB => {
                    assert_eq!(sh.sh_size as usize % SYM_SIZE, 0, "{}: symtab size", src.display());
                    for k in 0..sh.sh_size as usize / SYM_SIZE {
                        let so = body_start + k * SYM_SIZE;
                        let sym = Elf64Sym::parse(&bytes, so).expect("parse sym");
                        let mut sw = Vec::new();
                        sym.write(&mut sw);
                        assert_eq!(
                            sw,
                            &bytes[so..so + SYM_SIZE],
                            "{}: symbol {} does not round-trip",
                            src.display(),
                            k
                        );
                    }
                }
                SHT_RELA => {
                    assert_eq!(sh.sh_size as usize % RELA_SIZE, 0, "{}: rela size", src.display());
                    for k in 0..sh.sh_size as usize / RELA_SIZE {
                        let ro = body_start + k * RELA_SIZE;
                        let rela = Elf64Rela::parse(&bytes, ro).expect("parse rela");
                        let mut rw = Vec::new();
                        rela.write(&mut rw);
                        assert_eq!(
                            rw,
                            &bytes[ro..ro + RELA_SIZE],
                            "{}: rela {} does not round-trip",
                            src.display(),
                            k
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn gas_brands_osabi_as_expected() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "elf_system_corpus",
            "gas_brands_osabi_as_expected",
            "no GNU assembler on this host",
        );
        return;
    };
    // The x13 pitfall list: gas 2.44 brands relocatables
    // ELFOSABI_FREEBSD on FreeBSD; Linux stays ELFOSABI_NONE. The
    // writer must match per target, so pin what gas does here.
    let expected = if cfg!(target_os = "freebsd") {
        afs_as::elf::ELFOSABI_FREEBSD
    } else {
        afs_as::elf::ELFOSABI_NONE
    };
    let tmp = celf::TempArtifacts::new("afs_elf_osabi");
    let src = &celf::corpus_files()[0];
    let obj_path = tmp.path(".o");
    celf::assemble_with_gas(&gas, src, &obj_path);
    let bytes = std::fs::read(&obj_path).expect("read");
    let ehdr = Elf64Ehdr::parse(&bytes).expect("ehdr");
    assert_eq!(
        ehdr.osabi,
        expected,
        "gas EI_OSABI {} != expected {} on this OS — update the writer's per-target branding",
        ehdr.osabi,
        expected
    );
}
