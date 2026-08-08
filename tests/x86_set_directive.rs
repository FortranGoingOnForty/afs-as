//! `.set NAME, TARGET` — symbol aliases, compared against GNU as.
//!
//! Filed as corpus evidence by Cgfried's `alias` attribute, which is musl's
//! `weak_alias` idiom. The alias takes its target's section, offset and type;
//! the BINDING comes from the alias's own `.globl`/`.weak`, which is what lets
//! a weak alias name a strong target.
//!
//! Two of these earn their keep. The LOCAL-alias call pins relocation folding: Getting the symbol right
//! while leaving a relocation gas folds away produces an object that links and
//! runs correctly and STILL diverges, because the alias has to be a known
//! local label before relocations are resolved. Bytes alone would not have
//! caught it — the relocation list is where it showed.
//!
//! The SIZED-object case pins the other one, and it is deliberately a CONSTANT
//! `.size` because that is NOT what a compiler emits: the positional form
//! `.size f, .-f` cannot simply be copied onto the alias, which has no
//! directive of its own for `.` to measure from. This suite passed while real
//! compiler output failed, so the row is here to keep both spellings covered.

#[path = "common/elf.rs"]
mod celf;

use afs_as::elf::parse_elf;
use afs_as::x86::assemble::assemble_x86;

/// (assembly, what the row pins)
const CASES: &[(&str, &str)] = &[
    (
        ".text\n.globl real\n.type real,@function\nreal: ret\n.globl al\n.set al,real\n",
        "global alias of a global function: section, offset and STT_FUNC",
    ),
    (
        ".text\n.globl real\n.type real,@function\nreal: ret\n.weak wk\n.set wk,real\n",
        "WEAK alias of a strong target: the binding is the alias's own",
    ),
    (
        ".text\nloc: ret\n.set la,loc\n.globl caller\ncaller: call la\n",
        "LOCAL alias, CALLED: gas folds it into the section symbol",
    ),
    (
        ".data\n.globl obj\n.type obj,@object\n.size obj,4\nobj: .long 42\n.globl oal\n.set oal,obj\n",
        "alias of a data object: STT_OBJECT and the CONSTANT .size, in .data",
    ),
    (
        ".text\n.globl f\n.type f,@function\nf: ret\n.size f, .-f\n.globl fa\n.set fa,f\n",
        "alias of a POSITIONAL .size: measured from the target's directive",
    ),
    (
        ".text\n.globl fwd\n.set fwd,later\n.globl later\n.type later,@function\nlater: ret\n",
        "FORWARD reference: the target is defined after the .set",
    ),
];

#[test]
fn gas_matches_set_symbol_aliases() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_set_directive",
            "gas_matches_set_symbol_aliases",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_set_directive");

    for (index, (src, what)) in CASES.iter().enumerate() {
        let src_path = tmp.path(&format!("case_{index}.s"));
        let gas_obj = tmp.path(&format!("case_{index}_gas.o"));
        std::fs::write(&src_path, src).unwrap();
        celf::assemble_with_gas(&gas, &src_path, &gas_obj);

        let ours = assemble_x86(src, 0)
            .unwrap_or_else(|e| panic!("afs-as rejected case {index} ({what}): {e:?}"));
        let theirs = parse_elf(&std::fs::read(&gas_obj).unwrap())
            .unwrap_or_else(|e| panic!("parse gas object for case {index}: {e:?}"));

        assert_eq!(
            celf::normalize(&ours),
            celf::normalize(&theirs),
            "case {index} disagrees with gas: {what}",
        );
    }
}
