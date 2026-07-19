use afs_as::elf::{parse_elf, write_elf, SymbolPlace};
use afs_as::x86::assemble::assemble_x86;

const HALF: u64 = 9_223_372_036_854_775_807;

fn overflow(src: &str) -> String {
    assemble_x86(src, 0)
        .expect_err("overflowing layout unexpectedly assembled")
        .msg
}

#[test]
fn section_size_may_reach_u64_max() {
    let src = format!(".bss\n.space {HALF}\n.space {HALF}\n.byte 0\n");
    let object = assemble_x86(&src, 0).expect("boundary layout should assemble");
    let bss = object.section_by_name(".bss").expect("bss section");

    assert!(bss.data.is_empty());
    assert_eq!(bss.nobits_size, u64::MAX);

    let encoded = write_elf(&object).expect("serialize boundary object");
    let reparsed = parse_elf(&encoded).expect("parse boundary object");
    assert_eq!(
        reparsed
            .section_by_name(".bss")
            .expect("reparsed bss section")
            .nobits_size,
        u64::MAX
    );
}

#[test]
fn section_size_above_u64_max_is_rejected() {
    let src = format!(".bss\n.space {HALF}\n.space {HALF}\n.zero 3\n");
    assert_eq!(overflow(&src), "section layout size overflows u64");
}

#[test]
fn alignment_past_u64_max_is_rejected() {
    let src = format!(".bss\n.space {HALF}\n.space {HALF}\n.p2align 2\n");
    assert_eq!(overflow(&src), "section layout size overflows u64");
}

#[test]
fn max_skip_can_suppress_alignment_past_u64_max() {
    let src = format!(".bss\n.space {HALF}\n.space {HALF}\n.p2align 2,,1\n");
    let object = assemble_x86(&src, 0).expect("max-skip should suppress padding");
    let bss = object.section_by_name(".bss").expect("bss section");

    assert_eq!(bss.nobits_size, u64::MAX - 1);
    assert_eq!(bss.sh_addralign, 4);
}

#[test]
fn local_common_size_overflow_is_rejected() {
    let src = format!(".bss\n.space {HALF}\n.space {HALF}\n.local item\n.comm item,2,1\n");
    assert_eq!(overflow(&src), "local COMMON size overflows u64");
}

#[test]
fn local_common_alignment_overflow_is_rejected() {
    let src = format!(".bss\n.space {HALF}\n.space {HALF}\n.local item\n.comm item,1,4\n");
    assert_eq!(overflow(&src), "local COMMON alignment overflows u64");
}

#[test]
fn local_common_may_end_at_u64_max() {
    let src = format!(
        ".bss\n.space {HALF}\n.space {}\n.local item\n.comm item,3,4\n",
        HALF - 2
    );
    let object = assemble_x86(&src, 0).expect("boundary COMMON should assemble");
    let bss = object.section_by_name(".bss").expect("bss section");
    let item = object
        .symbols
        .iter()
        .find(|symbol| symbol.name == "item")
        .expect("item symbol");

    assert_eq!(bss.nobits_size, u64::MAX);
    assert!(matches!(item.place, SymbolPlace::Section(_)));
    assert_eq!(item.value, u64::MAX - 3);
    assert_eq!(item.size, 3);
}

#[test]
fn unencodable_high_offset_branch_is_rejected() {
    let src = format!(".bss\njmp .Ltarget\n.space {HALF}\n.Ltarget:\n");
    let message = overflow(&src);

    assert!(
        message.contains("branch displacement") && message.contains("i32"),
        "unexpected diagnostic: {message}"
    );
}

#[test]
fn unencodable_same_section_pcrel_fixup_is_rejected() {
    let src = format!(".bss\ncall .Ltarget\n.space {HALF}\n.Ltarget:\n");
    let message = overflow(&src);

    assert!(
        message.contains("displacement") && message.contains("i32"),
        "unexpected diagnostic: {message}"
    );
}

#[test]
fn cross_section_relocation_addend_wrap_is_explicit() {
    let src = format!(".text\n.quad .Ltarget+8\n.bss\n.space {HALF}\n.Ltarget:\n");
    let object = assemble_x86(&src, 0).expect("high relocation addend should assemble");
    let text = object.section_by_name(".text").expect("text section");

    assert_eq!(text.relas.len(), 1);
    assert_eq!(text.relas[0].addend, i64::MIN + 7);
}

#[test]
fn cross_section_relocation_addend_may_reach_i64_max() {
    let src = format!(".text\n.quad .Ltarget-1\n.bss\n.space {HALF}\n.byte 0\n.Ltarget:\n");
    let object = assemble_x86(&src, 0).expect("boundary relocation should assemble");
    let text = object.section_by_name(".text").expect("text section");

    assert_eq!(text.relas.len(), 1);
    assert_eq!(text.relas[0].addend, i64::MAX);
}
