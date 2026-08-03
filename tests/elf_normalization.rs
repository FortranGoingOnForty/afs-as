//! Regression coverage for the ELF differential oracle itself.

#[path = "common/elf.rs"]
mod celf;

use afs_as::elf::{
    ObjectFile, Section, Symbol, SymbolPlace, ELFOSABI_NONE, EM_X86_64, STB_GLOBAL, STT_FUNC,
    STV_DEFAULT, STV_HIDDEN,
};

fn sample_object() -> ObjectFile {
    let mut object = ObjectFile::new(EM_X86_64, ELFOSABI_NONE);
    let mut text = Section::text();
    text.data.push(0xc3);
    object.sections.push(text);
    object.symbols.push(Symbol {
        name: "function".into(),
        bind: STB_GLOBAL,
        typ: STT_FUNC,
        vis: STV_DEFAULT,
        place: SymbolPlace::Section(0),
        value: 0,
        size: 1,
    });
    object
}

#[test]
fn normalization_distinguishes_section_alignment() {
    let object = sample_object();
    let mut over_aligned = object.clone();
    over_aligned.sections[0].sh_addralign = 4096;

    assert_ne!(object, over_aligned, "fixture must change the object model");
    assert_ne!(celf::normalize(&object), celf::normalize(&over_aligned));
}

#[test]
fn normalization_distinguishes_symbol_visibility() {
    let object = sample_object();
    let mut hidden = object.clone();
    hidden.symbols[0].vis = STV_HIDDEN;

    assert_ne!(object, hidden, "fixture must change the object model");
    assert_ne!(celf::normalize(&object), celf::normalize(&hidden));
}

#[test]
fn normalization_preserves_duplicate_section_multiplicity() {
    let object = sample_object();
    let mut duplicate_section = object.clone();
    duplicate_section.sections.push(object.sections[0].clone());

    assert_ne!(
        object, duplicate_section,
        "fixture must add a second section"
    );
    assert_ne!(
        celf::normalize(&object),
        celf::normalize(&duplicate_section)
    );
}

#[test]
fn normalization_still_ignores_section_order() {
    let mut object = sample_object();
    object.sections.push(Section::data());

    let mut reordered = object.clone();
    reordered.sections.swap(0, 1);
    reordered.symbols[0].place = SymbolPlace::Section(1);

    assert_ne!(object, reordered, "fixture must reorder the object model");
    assert_eq!(celf::normalize(&object), celf::normalize(&reordered));
}
