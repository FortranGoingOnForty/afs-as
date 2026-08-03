use afs_as::elf::{SHF_ALLOC, SHT_NOTE};
use afs_as::x86::assemble::assemble_x86;

#[test]
fn emits_allocated_elf_note_section() {
    let object = assemble_x86(
        ".section .note.cgf.safe,\"a\",@note\n.long 4, 4, 1, 0x00464743, 1\n",
        0,
    )
    .expect("assemble note section");
    let note = object
        .sections
        .iter()
        .find(|section| section.name == ".note.cgf.safe")
        .expect("note section exists");

    assert_eq!(note.sh_type, SHT_NOTE);
    assert_eq!(note.sh_flags, SHF_ALLOC);
}
