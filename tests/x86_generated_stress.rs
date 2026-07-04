//! x14: program-shaped seeded cases through the full pipeline —
//! assemble_x86 -> write_elf -> parse_elf -> write_elf — asserting
//! writer determinism, reader/writer agreement at the byte level,
//! and model validity. No system assembler needed, so this runs on
//! every host.

#[path = "common/x86_gen.rs"]
mod x86_gen;

use afs_as::elf::{parse_elf, write_elf, ELFOSABI_FREEBSD, ELFOSABI_NONE};
use afs_as::x86::assemble::assemble_x86;

const SEEDS: u64 = 128;

fn host_osabi() -> u8 {
    if cfg!(target_os = "freebsd") {
        ELFOSABI_FREEBSD
    } else {
        ELFOSABI_NONE
    }
}

#[test]
fn generated_cases_roundtrip_and_validate() {
    for seed in 0..SEEDS {
        let src = x86_gen::generate_supported_case(seed);
        let obj = assemble_x86(&src, host_osabi())
            .unwrap_or_else(|e| panic!("seed {}: assemble failed: {}\n{}", seed, e, src));
        afs_as::elf::validate(&obj)
            .unwrap_or_else(|e| panic!("seed {}: invalid model: {}", seed, e));

        let bytes1 = write_elf(&obj).unwrap_or_else(|e| panic!("seed {}: write: {}", seed, e));
        // Determinism: assembling and writing again is byte-identical.
        let obj2 = assemble_x86(&src, host_osabi()).unwrap();
        let bytes2 = write_elf(&obj2).unwrap();
        assert_eq!(bytes1, bytes2, "seed {}: writer nondeterminism", seed);

        // Reader/writer agreement: lift and re-emit reproduces the
        // record-level bytes (the x13 fidelity property).
        let lifted = parse_elf(&bytes1)
            .unwrap_or_else(|e| panic!("seed {}: our own object unreadable: {}", seed, e));
        let bytes3 = write_elf(&lifted).unwrap();
        assert_eq!(bytes1, bytes3, "seed {}: lift+re-emit drifts", seed);
    }
}

#[test]
fn generated_cases_are_nontrivial() {
    // Guard against the generator degenerating: cases must keep
    // producing text, data, relocations, and symbols.
    let mut text = 0usize;
    let mut relas = 0usize;
    let mut syms = 0usize;
    for seed in 0..SEEDS {
        let src = x86_gen::generate_supported_case(seed);
        let obj = assemble_x86(&src, host_osabi()).unwrap();
        for s in &obj.sections {
            if s.name == ".text" {
                text += s.data.len();
            }
            relas += s.relas.len();
        }
        syms += obj.symbols.len();
    }
    assert!(text > 20_000, "suspiciously little text: {}", text);
    assert!(relas > 300, "suspiciously few relocations: {}", relas);
    assert!(syms > 500, "suspiciously few symbols: {}", syms);
}
