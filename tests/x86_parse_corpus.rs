//! x14: the AT&T parser must accept every checked-in corpus fixture
//! (real armfortas backend output) completely — every line becomes a
//! label, instruction, or directive; nothing is skipped silently.

use afs_as::x86::parse::{parse, Stmt};

#[test]
fn every_corpus_fixture_parses_completely() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus_elf");
    let mut checked = 0usize;
    let mut insns = 0usize;
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "s"))
        .collect();
    files.sort();
    for path in files {
        let src = std::fs::read_to_string(&path).expect("read fixture");
        let stmts = parse(&src).unwrap_or_else(|e| {
            panic!("{}: {}\n{}", path.display(), e, e.render_with_source(&src))
        });
        assert!(!stmts.is_empty(), "{}: parsed to nothing", path.display());
        insns += stmts
            .iter()
            .filter(|s| matches!(s.stmt, Stmt::Insn { .. }))
            .count();
        checked += 1;
    }
    assert!(checked >= 6, "corpus shrank? {} fixtures", checked);
    assert!(insns > 100, "suspiciously few instructions: {}", insns);
}

#[test]
fn diagnostics_carry_line_and_caret() {
    let src = ".text\nmovq %rax, %nosuchreg\n";
    let err = parse(src).unwrap_err();
    assert_eq!(err.line, 2);
    let rendered = err.render_with_source(src);
    assert!(rendered.contains("nosuchreg"), "{}", rendered);
    assert!(rendered.contains('^'), "{}", rendered);
}

#[test]
fn unsupported_directive_fails_loudly() {
    let err = parse(".intel_syntax noprefix\n").unwrap_err();
    assert!(
        err.msg.contains("unsupported directive"),
        "wrong error: {}",
        err.msg
    );
}
