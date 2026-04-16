#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::panic::{self, AssertUnwindSafe};

use afs_as::assemble;
use afs_as::macho::{self, ObjectFile, SectionKind};

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as u32
    }

    fn bounded(&mut self, upper: u32) -> u32 {
        self.next_u32() % upper
    }
}

fn generate_case(seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let cstring_count = 2 + rng.bounded(3) as usize;
    let const_count = 4 + rng.bounded(4) as usize;
    let data_count = 3 + rng.bounded(4) as usize;
    let bss_size = 16 * (1 + rng.bounded(4) as u64);

    let mut src = String::new();
    let _ = writeln!(src, ".build_version macos, 11, 0 sdk_version 15, 5");
    let _ = writeln!(src, ".subsections_via_symbols");
    let _ = writeln!(src, ".globl _stress_{}", seed);
    let _ = writeln!(src, ".text");
    let _ = writeln!(src, ".p2align 2");
    let _ = writeln!(src, "_stress_{}:", seed);
    let _ = writeln!(src, "    and w8, w0, #0x7");
    let _ = writeln!(src, "    ubfiz w9, w8, #5, #3");
    let _ = writeln!(src, "    msub w10, w9, w8, w0");
    let _ = writeln!(src, "    cbz w10, Lcall_{}", seed);
    let _ = writeln!(src, "    tbz x0, #5, Llocal_{}", seed);
    let _ = writeln!(src, "    b Ldone_{}", seed);
    let _ = writeln!(src, "Lcall_{}:", seed);
    let _ = writeln!(src, "    adrp x11, cstr0_{}@PAGE", seed);
    let _ = writeln!(src, "    add x11, x11, cstr0_{}@PAGEOFF", seed);
    let _ = writeln!(src, "    adrp x12, _ext_{}_0@GOTPAGE", seed);
    let _ = writeln!(src, "    ldr x12, [x12, _ext_{}_0@GOTPAGEOFF]", seed);
    let _ = writeln!(src, "    bl _puts");
    let _ = writeln!(src, "Llocal_{}:", seed);
    let _ = writeln!(src, "    adrp x13, data0_{}@PAGE", seed);
    let _ = writeln!(src, "    add x13, x13, data0_{}@PAGEOFF", seed);
    let _ = writeln!(src, "Ldone_{}:", seed);
    let _ = writeln!(src, "    ret");

    let _ = writeln!(src, ".section __TEXT,__cstring,cstring_literals");
    for index in 0..cstring_count {
        let _ = writeln!(src, "cstr{}_{}:", index, seed);
        let _ = writeln!(
            src,
            "    .asciz \"stress-{}-{}-{:08x}\"",
            seed,
            index,
            rng.next_u32()
        );
    }

    let _ = writeln!(src, ".section __TEXT,__const");
    for index in 0..const_count {
        let _ = writeln!(src, "const{}_{}:", index, seed);
        match index % 4 {
            0 => {
                let _ = writeln!(src, "    .quad data0_{}", seed);
            }
            1 => {
                let _ = writeln!(src, "    .quad _ext_{}_{}", seed, index);
            }
            2 => {
                let _ = writeln!(
                    src,
                    "    .quad _other_{}_{} - _ext_{}_{} + {}",
                    seed,
                    index,
                    seed,
                    index - 1,
                    4 * (1 + (index as i64 % 3))
                );
            }
            _ => {
                let _ = writeln!(src, "    .quad _puts@GOT");
            }
        }
    }

    let _ = writeln!(src, ".data");
    let _ = writeln!(src, ".p2align 3");
    for index in 0..data_count {
        let _ = writeln!(src, "data{}_{}:", index, seed);
        match index % 3 {
            0 => {
                let cstr_index = index % cstring_count;
                let _ = writeln!(src, "    .quad cstr{}_{}", cstr_index, seed);
            }
            1 => {
                let _ = writeln!(src, "    .quad _ext_{}_{}", seed, index);
            }
            _ => {
                let _ = writeln!(
                    src,
                    "    .quad _other_{}_{} - _ext_{}_{} + {}",
                    seed,
                    index,
                    seed,
                    index,
                    4 * (1 + (rng.bounded(4) as i64))
                );
            }
        }
    }

    let _ = writeln!(
        src,
        ".zerofill __DATA,__bss,_scratch_{},{},4",
        seed, bss_size
    );
    src
}

fn assert_object_semantics(obj: &ObjectFile, src: &str) {
    let mut section_names = BTreeSet::new();
    for (index, section) in obj.sections.iter().enumerate() {
        assert!(
            section_names.insert((section.segment.clone(), section.name.clone())),
            "duplicate section {},{}\n---source---\n{}",
            section.segment,
            section.name,
            src
        );
        match section.kind {
            SectionKind::ZeroFill | SectionKind::ThreadLocalZeroFill => assert!(
                section.data.is_empty(),
                "zerofill section {},{} should not carry file-backed bytes\n---source---\n{}",
                section.segment,
                section.name,
                src
            ),
            _ => assert_eq!(
                section.data.len() as u64,
                section.size,
                "section {},{} size/data mismatch\n---source---\n{}",
                section.segment,
                section.name,
                src
            ),
        }

        for reloc in &section.relocations {
            let width = 1u64 << reloc.length;
            assert!(
                reloc.offset as u64 + width <= section.size,
                "relocation at offset {} overflows section {},{}\n---source---\n{}",
                reloc.offset,
                section.segment,
                section.name,
                src
            );
            if reloc.extern_ {
                let symbol = obj
                    .symbols
                    .get(reloc.symbol_idx as usize)
                    .unwrap_or_else(|| {
                        panic!(
                            "missing relocation symbol index {}\n---source---\n{}",
                            reloc.symbol_idx, src
                        )
                    });
                assert!(
                    !symbol.name.is_empty(),
                    "empty relocation symbol name in section {},{}\n---source---\n{}",
                    section.segment,
                    section.name,
                    src
                );
            } else if reloc.reloc_type != macho::ARM64_RELOC_ADDEND {
                assert!(
                    reloc.symbol_idx >= 1 && reloc.symbol_idx <= obj.sections.len() as u32,
                    "non-external relocation section index {} is out of range\n---source---\n{}",
                    reloc.symbol_idx,
                    src
                );
            }
        }

        let _ = index;
    }

    let mut seen_symbol_names = BTreeSet::new();
    let mut previous_class = 0u8;
    for symbol in &obj.symbols {
        assert!(
            seen_symbol_names.insert(symbol.name.clone()),
            "duplicate symbol '{}'\n---source---\n{}",
            symbol.name,
            src
        );
        if symbol.undefined || symbol.common || symbol.absolute {
            assert_eq!(
                symbol.section, 0,
                "symbol '{}' should have section 0\n---source---\n{}",
                symbol.name, src
            );
        } else {
            assert!(
                symbol.section >= 1 && symbol.section as usize <= obj.sections.len(),
                "symbol '{}' section {} out of range\n---source---\n{}",
                symbol.name,
                symbol.section,
                src
            );
        }
        if symbol.undefined {
            assert!(
                symbol.global || symbol.common,
                "undefined local symbol '{}'\n---source---\n{}",
                symbol.name,
                src
            );
        }

        let class = if !symbol.global && !symbol.undefined {
            0
        } else if symbol.global && !symbol.undefined {
            1
        } else {
            2
        };
        assert!(
            class >= previous_class,
            "symbol classes out of LC_DYSYMTAB order at '{}'\n---source---\n{}",
            symbol.name,
            src
        );
        previous_class = class;
    }
}

#[test]
fn generated_stress_objects_have_valid_internal_semantics() {
    for seed in 1..=24u64 {
        let src = generate_case(seed);
        let obj = panic::catch_unwind(AssertUnwindSafe(|| assemble::assemble_source(&src)))
            .unwrap_or_else(|_| panic!("assemble stress seed {} panicked\n{}", seed, src))
            .unwrap_or_else(|err| panic!("assemble stress seed {} failed: {}\n{}", seed, err, src));
        assert_object_semantics(&obj, &src);
        let mut bytes = Vec::new();
        macho::write_macho(&obj, &mut bytes)
            .unwrap_or_else(|err| panic!("write stress seed {} failed: {}\n{}", seed, err, src));
        assert!(
            !bytes.is_empty(),
            "empty object for stress seed {}\n---source---\n{}",
            seed,
            src
        );
    }
}

#[test]
fn generated_stress_objects_match_system_as() {
    for seed in 1..=8u64 {
        let src = generate_case(seed);
        let paths = common::TempPaths::new(&format!("afs_stress_{}", seed));
        fs::write(&paths.asm, &src).expect("write generated stress assembly");
        panic::catch_unwind(AssertUnwindSafe(|| {
            common::assemble_with_ours(&src, &paths.obj)
        }))
        .unwrap_or_else(|_| panic!("assemble_with_ours stress seed {} panicked\n{}", seed, src));
        common::assemble_with_system(&paths.asm, &paths.ref_obj);
        let ours = fs::read(&paths.obj).expect("read afs-as object");
        let reference = fs::read(&paths.ref_obj).expect("read system object");
        assert_eq!(
            ours, reference,
            "raw object mismatch for generated stress seed {}\n---source---\n{}",
            seed, src
        );
    }
}
