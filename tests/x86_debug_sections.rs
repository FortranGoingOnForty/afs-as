#[path = "common/elf.rs"]
mod celf;

use afs_as::elf::{
    parse_elf, reloc::x86_64::*, ELFOSABI_FREEBSD, ELFOSABI_NONE, SHF_ALLOC, SHT_NOBITS,
    SHT_PROGBITS,
};
use afs_as::x86::assemble::assemble_x86;

const SOURCE: &str = r#"
.text
.globl main
main:
.Lfunc:
    ret
.Lfe0_0:

.section .debug_abbrev,"",@progbits
.byte 1, 2, 0

.section .debug_info,"",@progbits
.Linfo_begin:
.long .Lfe0_0-main
.long external32
.quad external64
.quad external64-.
.Lspan_begin:
.byte 0xaa, 0xbb, 0xcc
.Lspan_end:
.long .Lspan_end-.Lspan_begin
.long .Lfunc-.
.Linfo_end:

.section .debug_line,"",@progbits
.Lline_begin:
.long .Lline_end-.Lline_begin
.Lline_end:

.section .eh_frame,"a",@progbits
.Lframe_begin:
.long .Lfe0_0-main
.long .Lframe_end-.Lframe_begin
.long .Lfunc-.
.Lframe_end:

.section .debug_scratch,"",@nobits
.zero 16
"#;

fn host_osabi() -> u8 {
    if cfg!(target_os = "freebsd") {
        ELFOSABI_FREEBSD
    } else {
        ELFOSABI_NONE
    }
}

#[test]
fn preserves_debug_and_unwind_section_metadata_and_relocations() {
    let obj = assemble_x86(SOURCE, host_osabi()).expect("assemble debug sections");

    for name in [".debug_abbrev", ".debug_info", ".debug_line"] {
        let section = obj.section_by_name(name).expect("debug section");
        assert_eq!(section.sh_type, SHT_PROGBITS, "type for {name}");
        assert_eq!(section.sh_flags, 0, "flags for {name}");
    }
    let eh_frame = obj.section_by_name(".eh_frame").expect("eh_frame");
    assert_eq!(eh_frame.sh_type, SHT_PROGBITS);
    assert_eq!(eh_frame.sh_flags, SHF_ALLOC);

    let scratch = obj
        .section_by_name(".debug_scratch")
        .expect("nobits debug section");
    assert_eq!(scratch.sh_type, SHT_NOBITS);
    assert_eq!(scratch.sh_flags, 0);
    assert_eq!(scratch.nobits_size, 16);
    assert!(scratch.data.is_empty());

    let info = obj.section_by_name(".debug_info").unwrap();
    assert_eq!(u32::from_le_bytes(info.data[0..4].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(info.data[27..31].try_into().unwrap()), 3);
    assert_eq!(info.relas.len(), 4);
    assert_eq!(info.relas[0].r_type, R_X86_64_32);
    assert_eq!(info.relas[1].r_type, R_X86_64_64);
    assert_eq!(info.relas[2].r_type, R_X86_64_PC64);
    assert_eq!(info.relas[3].r_type, R_X86_64_PC32);

    let frame = obj.section_by_name(".eh_frame").unwrap();
    assert_eq!(u32::from_le_bytes(frame.data[0..4].try_into().unwrap()), 1);
    assert_eq!(u32::from_le_bytes(frame.data[4..8].try_into().unwrap()), 12);
    assert_eq!(frame.relas.len(), 1);
    assert_eq!(frame.relas[0].r_type, R_X86_64_PC32);
}

#[test]
fn debug_section_surface_matches_gnu_as() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_debug_sections",
            "debug_section_surface_matches_gnu_as",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_debug_sections");
    let src_path = tmp.path(".s");
    let obj_path = tmp.path(".o");
    std::fs::write(&src_path, SOURCE).unwrap();
    celf::assemble_with_gas(&gas, &src_path, &obj_path);
    let gas_obj = parse_elf(&std::fs::read(&obj_path).unwrap()).expect("parse gas object");
    let ours = assemble_x86(SOURCE, host_osabi()).expect("assemble our object");

    let gas = celf::normalize(&gas_obj);
    let ours = celf::normalize(&ours);
    for name in [
        ".debug_abbrev",
        ".debug_info",
        ".debug_line",
        ".eh_frame",
        ".debug_scratch",
    ] {
        assert_eq!(
            ours.sections.iter().find(|section| section.name == name),
            gas.sections.iter().find(|section| section.name == name),
            "section {name}"
        );
    }
    let relevant = |relocs: &[(String, u64, u32, String, i64)]| {
        relocs
            .iter()
            .filter(|r| matches!(r.0.as_str(), ".debug_info" | ".eh_frame"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(relevant(&ours.relocs), relevant(&gas.relocs));
}
