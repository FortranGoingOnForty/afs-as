#[path = "common/elf.rs"]
mod celf;

use std::process::Command;

use afs_as::x86::assemble::assemble_x86;

const COLLISIONS: &[&str] = &[
    ".text\n.globl foo\n.type foo,@function\nfoo: ret\n.comm foo,8,8\ncaller: call foo\n",
    ".comm foo,8,8\n.text\nfoo: ret\n",
];

#[test]
fn defined_labels_cannot_be_redeclared_as_common() {
    for (src, line) in COLLISIONS.iter().zip([5, 3]) {
        let err = assemble_x86(src, 0).expect_err("label/COMMON collision assembled");
        assert_eq!(err.line, Some(line));
        assert_eq!(err.col, Some(1));
        assert!(
            err.msg.contains("symbol 'foo' is already defined"),
            "unexpected diagnostic: {err}"
        );
    }
}

#[test]
fn gas_rejects_the_same_collisions() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_symbol_collisions",
            "gas_rejects_the_same_collisions",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_symbol_collision");
    for (index, src) in COLLISIONS.iter().enumerate() {
        let src_path = tmp.path(&format!("_{index}.s"));
        let obj_path = tmp.path(&format!("_{index}.o"));
        std::fs::write(&src_path, src).unwrap();
        let output = Command::new(&gas)
            .arg("--64")
            .arg("-o")
            .arg(&obj_path)
            .arg(&src_path)
            .output()
            .expect("run gas");
        assert!(
            !output.status.success(),
            "gas accepted label/COMMON collision {index}"
        );
    }
}
