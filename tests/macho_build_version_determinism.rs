#![cfg(unix)]

#[path = "common/owned_temp_dir.rs"]
mod owned_temp_dir;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use owned_temp_dir::OwnedTempDir;

fn write_fake_sw_vers(directory: &Path, product_version: &str) {
    fs::create_dir(directory).expect("create fake tool directory");
    let executable = directory.join("sw_vers");
    fs::write(
        &executable,
        format!("#!/bin/sh\nprintf '%s\\n' '{product_version}'\n"),
    )
    .expect("write fake sw_vers");

    let mut permissions = fs::metadata(&executable)
        .expect("read fake sw_vers metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).expect("make fake sw_vers executable");
}

fn assemble_with_path(source: &Path, output: &Path, path: &Path) -> Vec<u8> {
    let result = Command::new(env!("CARGO_BIN_EXE_afs-as"))
        .arg(source)
        .arg("-o")
        .arg(output)
        .env("PATH", path)
        .output()
        .expect("run afs-as");
    assert!(
        result.status.success(),
        "afs-as failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::read(output).expect("read assembled object")
}

#[test]
fn implicit_macho_build_version_is_path_independent() {
    let temp = OwnedTempDir::new("afs_macho_build_version_path");
    let source = temp.path("input.s");
    fs::write(&source, ".text\n.globl _entry\n_entry:\nret\n").expect("write source");

    let macos_13_path = temp.path("macos-13-bin");
    let macos_14_path = temp.path("macos-14-bin");
    write_fake_sw_vers(&macos_13_path, "13.6.1");
    write_fake_sw_vers(&macos_14_path, "14.4.0");

    let macos_13_object = assemble_with_path(&source, &temp.path("macos-13.o"), &macos_13_path);
    let macos_14_object = assemble_with_path(&source, &temp.path("macos-14.o"), &macos_14_path);

    assert_eq!(
        macos_13_object, macos_14_object,
        "implicit Mach-O deployment metadata must not depend on PATH"
    );
}
