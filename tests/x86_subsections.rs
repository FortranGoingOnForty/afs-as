use afs_as::elf::SymbolPlace;
use afs_as::x86::assemble::assemble_x86;

fn symbol_value(object: &afs_as::elf::ObjectFile, name: &str) -> u64 {
    let symbol = object
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol '{name}'"));
    assert!(
        matches!(symbol.place, SymbolPlace::Section(_)),
        "symbol '{name}' is not section-defined"
    );
    symbol.value
}

#[test]
fn subsection_streams_merge_in_numeric_order() {
    let source = ".text 7\n\
                  text7a: .byte 0x70\n\
                  .text 0\n\
                  text0: .byte 0x00\n\
                  .text 3\n\
                  text3: .byte 0x30\n\
                  .text 7\n\
                  text7b: .byte 0x71\n\
                  .data 5\n\
                  data5: .byte 0x50\n\
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

    let object = assemble_x86(source, 0).expect("assemble subsection streams");
    assert_eq!(
        object.section_by_name(".text").expect(".text").data,
        [0x00, 0x30, 0x70, 0x71]
    );
    assert_eq!(
        object.section_by_name(".data").expect(".data").data,
        [0x10, 0x20, 0x50]
    );
    assert_eq!(object.section_by_name(".bss").expect(".bss").nobits_size, 6);

    for (name, expected) in [
        ("text0", 0),
        ("text3", 1),
        ("text7a", 2),
        ("text7b", 3),
        ("data0", 0),
        ("data2", 1),
        ("data5", 2),
        ("bss0", 0),
        ("bss2", 2),
        ("bss4", 5),
    ] {
        assert_eq!(symbol_value(&object, name), expected, "offset for {name}");
    }
}

#[test]
fn subsection_merge_precedes_alignment_relaxation_and_size_evaluation() {
    let source = ".text 1\n\
                  late:\n\
                  .p2align 2\n\
                  ret\n\
                  .size late,.-late\n\
                  .text 0\n\
                  start:\n\
                  jmp late\n";

    let object = assemble_x86(source, 0).expect("assemble cross-subsection branch");
    let text = object.section_by_name(".text").expect(".text");
    assert_eq!(text.data, [0xeb, 0x00, 0x66, 0x90, 0xc3]);
    assert!(
        text.relas.is_empty(),
        "local branch should be fully resolved"
    );
    assert_eq!(symbol_value(&object, "start"), 0);
    assert_eq!(symbol_value(&object, "late"), 2);
    assert_eq!(
        object
            .symbols
            .iter()
            .find(|symbol| symbol.name == "late")
            .expect("late symbol")
            .size,
        3
    );
}
