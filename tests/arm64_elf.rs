//! arm64 GNU-syntax source assembled to ELF64 relocatable objects.
//!
//! The byte-for-byte differential against `aarch64-linux-gnu-as` lives in the
//! consuming compiler's test lane, where a real corpus of emitted assembly is
//! available. What is pinned HERE is the model: which relocation a given
//! instruction earns, how a symbol's binding decides between a named and a
//! section-relative reference, and the mapping symbols the AArch64 ABI
//! requires. Those are the choices that silently mis-link when they are
//! wrong, and none of them is visible in a disassembly.

use afs_as::assemble::assemble_source_elf;
use afs_as::elf::{
    reloc::aarch64::*, ObjectFile, EM_AARCH64, SHF_ALLOC, SHF_EXECINSTR, SHF_WRITE, SHT_NOBITS,
    SHT_PROGBITS, STB_GLOBAL, STB_LOCAL, STB_WEAK, STT_FUNC, STT_OBJECT, STT_SECTION, STV_HIDDEN,
};
use afs_as::elf::{Rela, SymbolPlace};

fn assemble(src: &str) -> ObjectFile {
    assemble_source_elf(src).unwrap_or_else(|err| panic!("assembling failed: {}\n{}", err, src))
}

fn text_relocs(obj: &ObjectFile) -> &[Rela] {
    &obj.sections
        .iter()
        .find(|s| s.name == ".text")
        .expect(".text is always present")
        .relas
}

fn reloc_symbol_name(obj: &ObjectFile, rela: &Rela) -> String {
    let sym = &obj.symbols[rela.symbol];
    if sym.typ == STT_SECTION {
        match sym.place {
            SymbolPlace::Section(idx) => obj.sections[idx].name.clone(),
            _ => unreachable!("a section symbol lives in a section"),
        }
    } else {
        sym.name.clone()
    }
}

fn symbol(obj: &ObjectFile, name: &str) -> afs_as::elf::Symbol {
    obj.symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("no symbol '{}' in {:?}", name, symbol_names(obj)))
        .clone()
}

fn symbol_names(obj: &ObjectFile) -> Vec<&str> {
    obj.symbols.iter().map(|s| s.name.as_str()).collect()
}

// ---------------------------------------------------------------------
// Object shape
// ---------------------------------------------------------------------

#[test]
fn machine_is_aarch64_and_the_standard_sections_are_always_present() {
    let obj = assemble(".text\nf:\n    ret\n");
    assert_eq!(obj.machine, EM_AARCH64);
    // gas emits all three whether or not the source used them, and a
    // relocation's symbol index depends on that ordering.
    let names: Vec<_> = obj.sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(&names[..3], &[".text", ".data", ".bss"]);
    assert_eq!(obj.sections[0].sh_flags, SHF_ALLOC | SHF_EXECINSTR);
    assert_eq!(obj.sections[1].sh_flags, SHF_ALLOC | SHF_WRITE);
    assert_eq!(obj.sections[2].sh_type, SHT_NOBITS);
}

#[test]
fn ret_encodes_to_its_architectural_word() {
    let obj = assemble(".text\nf:\n    ret\n");
    assert_eq!(obj.sections[0].data, 0xD65F_03C0u32.to_le_bytes());
}

#[test]
fn a_generic_section_takes_its_flags_from_the_directive() {
    let obj = assemble(".section .rodata,\"a\",@progbits\n    .quad 7\n");
    let sec = obj
        .sections
        .iter()
        .find(|s| s.name == ".rodata")
        .expect(".rodata");
    assert_eq!(sec.sh_type, SHT_PROGBITS);
    assert_eq!(sec.sh_flags, SHF_ALLOC);
    assert_eq!(sec.data, 7u64.to_le_bytes());
}

#[test]
fn an_unknown_section_without_flags_is_an_error_rather_than_a_guess() {
    let err = assemble_source_elf(".section .mystery\n").unwrap_err();
    assert!(
        err.to_string().contains("explicit flag string"),
        "unexpected: {}",
        err
    );
}

#[test]
fn a_nobits_section_may_not_carry_initialized_bytes() {
    let err = assemble_source_elf(".section .bss\n    .quad 1\n").unwrap_err();
    assert!(
        err.to_string().contains("zero-fill") || err.to_string().contains("NOBITS"),
        "unexpected: {}",
        err
    );
}

#[test]
fn macho_only_directives_are_refused_in_elf_mode() {
    for src in [
        ".text\n.subsections_via_symbols\n",
        ".text\n.build_version macos, 11, 0\n",
    ] {
        let err = assemble_source_elf(src).unwrap_err();
        assert!(
            err.to_string().contains("Mach-O"),
            "expected a Mach-O rejection for {:?}, got {}",
            src,
            err
        );
    }
}

// ---------------------------------------------------------------------
// Relocations
// ---------------------------------------------------------------------

#[test]
fn adrp_plus_add_lo12_is_the_page_pair() {
    let obj = assemble(".text\nf:\n    adrp x0, g\n    add x0, x0, #:lo12:g\n    ret\n");
    let relocs = text_relocs(&obj);
    assert_eq!(relocs.len(), 2);
    assert_eq!(relocs[0].r_type, R_AARCH64_ADR_PREL_PG_HI21);
    assert_eq!(relocs[0].offset, 0);
    assert_eq!(relocs[1].r_type, R_AARCH64_ADD_ABS_LO12_NC);
    assert_eq!(relocs[1].offset, 4);
    for r in relocs {
        assert_eq!(reloc_symbol_name(&obj, r), "g");
    }
}

#[test]
fn the_lo12_relocation_follows_the_access_width_of_the_instruction_it_patches() {
    // The lo12 immediate is SCALED by the access size, so the linker has to
    // be told which scale to use. Mach-O spells all of these PAGEOFF12; ELF
    // does not, and picking the wrong row shifts the address.
    for (insn, expected) in [
        ("add x0, x0, #:lo12:g", R_AARCH64_ADD_ABS_LO12_NC),
        ("ldr w1, [x0, #:lo12:g]", R_AARCH64_LDST32_ABS_LO12_NC),
        ("ldr x1, [x0, #:lo12:g]", R_AARCH64_LDST64_ABS_LO12_NC),
        ("str x1, [x0, #:lo12:g]", R_AARCH64_LDST64_ABS_LO12_NC),
        ("ldr q1, [x0, #:lo12:g]", R_AARCH64_LDST128_ABS_LO12_NC),
    ] {
        let obj = assemble(&format!(
            ".text\nf:\n    adrp x0, g\n    {}\n    ret\n",
            insn
        ));
        let relocs = text_relocs(&obj);
        assert_eq!(
            relocs[1].r_type, expected,
            "wrong lo12 relocation for `{}`",
            insn
        );
    }
}

#[test]
fn the_narrow_lo12_forms_are_refused_by_name() {
    // LDST8/LDST16 lo12 folding is not implemented: the byte/halfword parser
    // yields a bare instruction and has nowhere to hang a relocation. It must
    // say so plainly rather than fail as a malformed expression.
    for insn in ["ldrb w1, [x0, #:lo12:g]", "strh w1, [x0, #:lo12:g]"] {
        let err = assemble_source_elf(&format!(".text\nf:\n    {}\n", insn)).unwrap_err();
        assert!(
            err.to_string().contains(":lo12:"),
            "expected a named refusal for `{}`, got {}",
            insn,
            err
        );
    }
}

#[test]
fn bl_is_call26_and_a_tail_b_is_jump26() {
    let call = assemble(".text\nf:\n    bl target\n");
    assert_eq!(text_relocs(&call)[0].r_type, R_AARCH64_CALL26);

    let jump = assemble(".text\nf:\n    b target\n");
    assert_eq!(text_relocs(&jump)[0].r_type, R_AARCH64_JUMP26);
}

#[test]
fn a_branch_to_a_local_label_resolves_with_no_relocation() {
    let obj = assemble(".text\nf:\n.Lloop:\n    b .Lloop\n");
    assert!(text_relocs(&obj).is_empty());
    // ...and the temporary never reaches the symbol table.
    assert!(!symbol_names(&obj).contains(&".Lloop"));
}

#[test]
fn a_branch_to_a_globl_symbol_keeps_its_relocation() {
    // Resolving this at assembly time would defeat interposition: the linker
    // is entitled to bind `g` to a different definition.
    let obj = assemble(".text\n.globl g\ng:\n    ret\nf:\n    bl g\n");
    let relocs = text_relocs(&obj);
    assert_eq!(relocs.len(), 1);
    assert_eq!(relocs[0].r_type, R_AARCH64_CALL26);
    assert_eq!(reloc_symbol_name(&obj, &relocs[0]), "g");
}

#[test]
fn a_relocation_against_a_defined_local_names_its_section_plus_offset() {
    let obj = assemble(
        ".section .rodata,\"a\",@progbits\n\
         pad:\n    .quad 0\n\
         tab:\n    .quad 0\n\
         .text\nf:\n    adrp x0, tab\n    add x0, x0, #:lo12:tab\n",
    );
    let relocs = text_relocs(&obj);
    assert_eq!(relocs.len(), 2);
    for r in relocs {
        assert_eq!(reloc_symbol_name(&obj, r), ".rodata");
        assert_eq!(r.addend, 8, "the local's offset becomes the addend");
    }
}

#[test]
fn an_explicit_addend_rides_the_rela_entry() {
    let obj = assemble(".text\nf:\n    adrp x0, g + 16\n    add x0, x0, #:lo12:g + 16\n");
    for r in text_relocs(&obj) {
        assert_eq!(r.addend, 16);
        assert_eq!(reloc_symbol_name(&obj, r), "g");
    }
}

#[test]
fn got_and_gottprel_operands_parse_to_their_own_relocations() {
    let obj = assemble(".text\nf:\n    adrp x0, :got:g\n    ldr x0, [x0, #:got_lo12:g]\n");
    let relocs = text_relocs(&obj);
    assert_eq!(relocs[0].r_type, R_AARCH64_ADR_GOT_PAGE);
    assert_eq!(relocs[1].r_type, R_AARCH64_LD64_GOT_LO12_NC);

    let tls =
        assemble(".text\nf:\n    adrp x0, :gottprel:g\n    ldr x0, [x0, #:gottprel_lo12:g]\n");
    let relocs = text_relocs(&tls);
    assert_eq!(relocs[0].r_type, R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21);
    assert_eq!(relocs[1].r_type, R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC);
}

#[test]
fn a_data_word_relocation_takes_the_width_of_the_directive() {
    let obj = assemble(".data\nd:\n    .quad g\n");
    let data = obj
        .sections
        .iter()
        .find(|s| s.name == ".data")
        .expect(".data");
    assert_eq!(data.relas.len(), 1);
    assert_eq!(data.relas[0].r_type, R_AARCH64_ABS64);
}

#[test]
fn an_unrelocatable_instruction_cannot_take_a_lo12_operand() {
    // `mov` has no 12-bit immediate field to patch. Silently emitting the
    // relocation anyway would corrupt an unrelated field.
    let err = assemble_source_elf(".text\nf:\n    orr x0, x0, #:lo12:g\n").unwrap_err();
    assert!(!err.to_string().is_empty());
}

// ---------------------------------------------------------------------
// Symbols
// ---------------------------------------------------------------------

#[test]
fn type_and_size_directives_reach_the_symbol_table() {
    let obj = assemble(".text\n.globl f\n.type f, @function\nf:\n    ret\n    ret\n.size f, .-f\n");
    let f = symbol(&obj, "f");
    assert_eq!(f.typ, STT_FUNC);
    assert_eq!(f.bind, STB_GLOBAL);
    assert_eq!(f.size, 8, "two instructions");
}

#[test]
fn local_overrides_globl_regardless_of_directive_order() {
    for src in [
        ".data\n.globl v\n.local v\n.type v, @object\nv:\n    .quad 0\n",
        ".data\n.local v\n.globl v\n.type v, @object\nv:\n    .quad 0\n",
    ] {
        let obj = assemble(src);
        let v = symbol(&obj, "v");
        assert_eq!(v.bind, STB_LOCAL, "for {:?}", src);
        assert_eq!(v.typ, STT_OBJECT);
    }
}

#[test]
fn weak_and_hidden_are_carried_through() {
    let obj = assemble(".text\n.weak w\n.hidden h\n.globl h\nh:\n    ret\n    bl w\n");
    assert_eq!(symbol(&obj, "w").bind, STB_WEAK);
    assert_eq!(symbol(&obj, "h").vis, STV_HIDDEN);
}

#[test]
fn an_undefined_reference_becomes_an_undefined_global() {
    let obj = assemble(".text\nf:\n    bl elsewhere\n");
    let sym = symbol(&obj, "elsewhere");
    assert_eq!(sym.bind, STB_GLOBAL);
    assert_eq!(sym.place, SymbolPlace::Undef);
}

#[test]
fn defined_symbols_come_out_in_address_order() {
    let obj =
        assemble(".text\n.globl a\na:\n    ret\n.globl b\nb:\n    ret\n.globl c\nc:\n    ret\n");
    let order: Vec<_> = obj
        .symbols
        .iter()
        .filter(|s| matches!(s.name.as_str(), "a" | "b" | "c"))
        .map(|s| (s.name.as_str(), s.value))
        .collect();
    assert_eq!(order, vec![("a", 0), ("b", 4), ("c", 8)]);
}

// ---------------------------------------------------------------------
// AArch64 mapping symbols
// ---------------------------------------------------------------------

#[test]
fn code_and_data_regions_get_their_mapping_symbols() {
    let obj = assemble(".text\nf:\n    ret\n.data\nd:\n    .quad 0\n");
    let mapping: Vec<_> = obj
        .symbols
        .iter()
        .filter(|s| s.name == "$x" || s.name == "$d")
        .map(|s| (s.name.as_str(), s.place, s.value))
        .collect();
    assert_eq!(
        mapping,
        vec![
            ("$x", SymbolPlace::Section(0), 0),
            ("$d", SymbolPlace::Section(1), 0),
        ]
    );
}

#[test]
fn a_data_island_inside_text_flips_the_mapping_back_and_forth() {
    let obj = assemble(".text\nf:\n    ret\n    .quad 0\n    ret\n");
    let mapping: Vec<_> = obj
        .symbols
        .iter()
        .filter(|s| s.name == "$x" || s.name == "$d")
        .map(|s| (s.name.as_str(), s.value))
        .collect();
    assert_eq!(mapping, vec![("$x", 0), ("$d", 4), ("$x", 12)]);
}

#[test]
fn alignment_padding_does_not_break_a_code_region() {
    // gas keeps one `$x` across `.p2align` fill; an extra mapping symbol
    // there would make every function boundary disassemble as data.
    let obj = assemble(".text\nf:\n    ret\n    .p2align 4\ng:\n    ret\n");
    let mapping = obj
        .symbols
        .iter()
        .filter(|s| s.name == "$x" || s.name == "$d")
        .count();
    assert_eq!(mapping, 1);
}

// ---------------------------------------------------------------------
// Determinism
// ---------------------------------------------------------------------

#[test]
fn assembling_twice_produces_identical_bytes() {
    let src = ".text\n.globl f\n.type f, @function\nf:\n    adrp x0, g\n\
               add x0, x0, #:lo12:g\n    bl h\n    ret\n.size f, .-f\n\
               .section .bss\n.local g\ng:\n    .zero 16\n";
    let first = afs_as::elf::write_elf(&assemble(src)).unwrap();
    let second = afs_as::elf::write_elf(&assemble(src)).unwrap();
    assert_eq!(first, second);
}
