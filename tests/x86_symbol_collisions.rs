#[path = "common/elf.rs"]
mod celf;

use std::process::Command;

use afs_as::elf::{SymbolPlace, STB_LOCAL};
use afs_as::x86::assemble::assemble_x86;

const COLLISIONS: &[&str] = &[
    ".text\n.globl foo\n.type foo,@function\nfoo: ret\n.comm foo,8,8\ncaller: call foo\n",
    ".comm foo,8,8\n.text\nfoo: ret\n",
];

const WEAK_COMMON_COLLISIONS: &[(&str, u32)] = &[
    (".weak wc\n.comm wc,8,8\n", 2),
    (".comm wc,8,8\n.weak wc\n", 2),
    (".comm wc,8,8\n.weak wc\n.comm wc,16,16\n", 2),
    (".comm wc,8,8\n.local wc\n.weak wc\n", 3),
];

const LOCALIZED_WEAK_COMMONS: &[&str] = &[
    ".weak wc\n.local wc\n.comm wc,8,8\n",
    ".local wc\n.weak wc\n.comm wc,8,8\n",
    ".local wc\n.comm wc,8,8\n.weak wc\n",
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
fn weak_global_common_is_rejected_in_every_directive_order() {
    for (src, common_line) in WEAK_COMMON_COLLISIONS {
        let err = assemble_x86(src, 0).expect_err("weak global COMMON assembled");
        assert_eq!(err.line, Some(*common_line));
        assert_eq!(err.col, Some(1));
        assert_eq!(err.msg, "symbol 'wc' can not be both weak and common");
    }
}

#[test]
fn local_common_binding_overrides_weak_metadata() {
    for src in LOCALIZED_WEAK_COMMONS {
        let object = assemble_x86(src, 0).expect("localized weak COMMON must assemble");
        let symbol = object
            .symbols
            .iter()
            .find(|symbol| symbol.name == "wc")
            .expect("localized COMMON symbol");
        assert_eq!(symbol.bind, STB_LOCAL);
        assert!(matches!(symbol.place, SymbolPlace::Section(_)));
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

#[test]
fn gas_matches_weak_common_binding_rules() {
    let Some(gas) = celf::gas_path() else {
        celf::skip(
            "x86_symbol_collisions",
            "gas_matches_weak_common_binding_rules",
            "no GNU assembler on this host",
        );
        return;
    };
    let tmp = celf::TempArtifacts::new("afs_x86_weak_common_collision");

    for (index, (src, _)) in WEAK_COMMON_COLLISIONS.iter().enumerate() {
        let src_path = tmp.path(&format!("_reject_{index}.s"));
        let obj_path = tmp.path(&format!("_reject_{index}.o"));
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
            "gas accepted weak global COMMON collision {index}"
        );
    }

    for (index, src) in LOCALIZED_WEAK_COMMONS.iter().enumerate() {
        let src_path = tmp.path(&format!("_accept_{index}.s"));
        let obj_path = tmp.path(&format!("_accept_{index}.o"));
        std::fs::write(&src_path, src).unwrap();
        let output = Command::new(&gas)
            .arg("--64")
            .arg("-o")
            .arg(&obj_path)
            .arg(&src_path)
            .output()
            .expect("run gas");
        assert!(
            output.status.success(),
            "gas rejected localized weak COMMON {index}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
