//! `.set NAME, TARGET` on the arm64 path — symbol aliases.
//!
//! Filed as corpus evidence by Cgfried's `alias` attribute, which is musl's
//! `weak_alias` idiom. PR #28 gave the x86 dialect the same thing; the arm64
//! and Mach-O paths are a different parser and assembler, and there `.set`
//! meant an ABSOLUTE assignment only, so `.set alias, real` was rejected with
//! "absolute symbol 'alias' must resolve to an absolute value".
//!
//! The alias takes its target's section, offset, type and SIZE; the BINDING is
//! its own, which is what lets a weak alias name a strong target.
//!
//! Absolute `.set` keeps working, and that is not a formality: the two forms
//! are told apart only by whether the right-hand side is a lone symbol naming
//! a defined LABEL, and the parser cannot make that call because it runs
//! before any label exists. `set_of_an_absolute_symbol_is_not_an_alias` is the
//! row that pins the boundary.
//!
//! The byte-for-byte differential against `aarch64-linux-gnu-as` lives in the
//! consuming compiler's lane, as it does for the rest of the arm64 surface.
//! What is pinned here is the model.

use afs_as::assemble::assemble_source_elf;
use afs_as::elf::{
    ObjectFile, SymbolPlace, STB_GLOBAL, STB_LOCAL, STB_WEAK, STT_FUNC, STT_NOTYPE, STT_OBJECT,
};

fn assemble(src: &str) -> ObjectFile {
    assemble_source_elf(src).unwrap_or_else(|err| panic!("assembling failed: {}\n{}", err, src))
}

fn symbol(obj: &ObjectFile, name: &str) -> afs_as::elf::Symbol {
    obj.symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| {
            panic!(
                "no symbol '{}' among {:?}",
                name,
                obj.symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
            )
        })
        .clone()
}

fn text_relocs(obj: &ObjectFile) -> &[afs_as::elf::Rela] {
    &obj.sections
        .iter()
        .find(|s| s.name == ".text")
        .expect(".text is always present")
        .relas
}

#[test]
fn global_alias_takes_the_targets_place_and_type() {
    let obj = assemble(
        ".text\n.globl real\n.type real,@function\nreal:\n\tret\n\
         .size real, .-real\n.globl al\n.set al, real\n",
    );
    let real = symbol(&obj, "real");
    let al = symbol(&obj, "al");

    assert_eq!(
        al.place, real.place,
        "alias must share the target's section"
    );
    assert_eq!(al.value, real.value, "alias must share the target's offset");
    assert_eq!(al.typ, STT_FUNC, "type follows the target");
    // The positional `.size real, .-real` is measured during emission, so an
    // alias resolved any earlier reports zero -- which looks right in a
    // disassembly and is wrong in the symbol table.
    assert_eq!(al.size, real.size, "size follows the target");
    assert_eq!(al.size, 4, "one instruction");
    assert_eq!(al.bind, STB_GLOBAL);
}

#[test]
fn weak_alias_may_name_a_strong_target() {
    let obj = assemble(
        ".text\n.globl real\n.type real,@function\nreal:\n\tret\n\
         .size real, .-real\n.weak wk\n.set wk, real\n",
    );
    let wk = symbol(&obj, "wk");

    // Binding is the ALIAS's own: this is musl's weak_alias, where a weak
    // name is layered over a strong definition.
    assert_eq!(wk.bind, STB_WEAK);
    assert_eq!(symbol(&obj, "real").bind, STB_GLOBAL);
    assert_eq!(wk.typ, STT_FUNC);
    assert_eq!(wk.size, 4);
}

#[test]
fn call_to_a_local_alias_folds_into_the_section_symbol() {
    let obj = assemble(".text\nloc:\n\tret\n.set la, loc\n.globl caller\ncaller:\n\tbl la\n");

    assert_eq!(symbol(&obj, "la").bind, STB_LOCAL);
    assert_eq!(symbol(&obj, "la").typ, STT_NOTYPE);
    // The alias has to be a known LOCAL LABEL before relocations resolve, or
    // the call keeps a named-symbol relocation where gas folds the section
    // symbol in. The object links and runs either way, so only the relocation
    // list shows it -- bytes alone would not.
    // Measured: gas emits NO relocation at all here -- a branch to a local
    // label in the same section is resolved outright. Leaving a named-symbol
    // relocation behind produces an object that links and runs correctly and
    // still diverges, and only the relocation list shows it.
    assert!(
        text_relocs(&obj).is_empty(),
        "a call to a same-section LOCAL alias is resolved, not relocated: {:?}",
        text_relocs(&obj)
    );
}

#[test]
fn alias_of_a_data_object_keeps_object_type_and_size() {
    let obj = assemble(
        ".data\n.globl o\n.type o,@object\n.size o,4\no:\n\t.word 42\n\
         .globl oa\n.set oa, o\n",
    );
    let oa = symbol(&obj, "oa");

    assert_eq!(oa.typ, STT_OBJECT);
    assert_eq!(oa.size, 4);
    assert_eq!(oa.place, symbol(&obj, "o").place);
}

#[test]
fn alias_resolves_a_target_defined_later() {
    // gas resolves a forward `.set`, so the alias cannot be applied where it
    // is written; it waits until every label is final.
    let obj = assemble(
        ".text\n.globl fwd\n.set fwd, later\n.globl later\n\
         .type later,@function\nlater:\n\tret\n.size later, .-later\n",
    );

    assert_eq!(symbol(&obj, "fwd").place, symbol(&obj, "later").place);
    assert_eq!(symbol(&obj, "fwd").value, symbol(&obj, "later").value);
    assert_eq!(symbol(&obj, "fwd").typ, STT_FUNC);
}

#[test]
fn absolute_set_is_still_an_absolute_assignment() {
    let obj = assemble(".text\nf:\n\tret\n.globl n\n.set n, 5\n");
    let n = symbol(&obj, "n");

    assert_eq!(n.place, SymbolPlace::Abs, "a value, not an address");
    assert_eq!(n.value, 5);
}

#[test]
fn set_of_an_absolute_symbol_is_not_an_alias() {
    // The boundary: `b` names a `.set` symbol, not a label, so `c` takes its
    // VALUE. Treating any lone symbol as an alias would break this.
    let obj = assemble(".text\nf:\n\tret\n.globl c\n.set b, 5\n.set c, b\n");
    let c = symbol(&obj, "c");

    assert_eq!(c.place, SymbolPlace::Abs);
    assert_eq!(c.value, 5);
}

#[test]
fn alias_of_an_undefined_symbol_is_rejected() {
    // A DELIBERATE divergence, measured: gas accepts this and emits only the
    // undefined `nowhere`, dropping `a` from the symbol table entirely. A
    // `.set` whose target this file never defines is far more likely a typo
    // than an intent, and silently producing a different symbol table than
    // the source asked for is the failure this assembler exists to avoid.
    // Nothing in the consuming compiler emits it: an `alias` attribute
    // requires its target defined in the same translation unit.
    let err = assemble_source_elf(".text\n.globl a\n.set a, nowhere\n")
        .expect_err("an alias must name something defined in this file");
    let text = format!("{}", err);
    assert!(
        text.contains("nowhere"),
        "the diagnostic must name the missing target, got: {}",
        text
    );
}
