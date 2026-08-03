//! Ownership-boundary regressions for the ELF differential artifacts.

#[path = "common/elf.rs"]
mod celf;

use std::fs;

#[test]
fn cleanup_preserves_prefix_matching_neighbor() {
    let artifacts = celf::TempArtifacts::new("afs_elf_cleanup_ownership");
    let root = artifacts.root().to_path_buf();
    let root_name = root
        .file_name()
        .expect("temporary root has a filename")
        .to_string_lossy();
    let foreign = root.with_file_name(format!("{root_name}0_foreign"));
    fs::write(artifacts.path(".owned"), b"owned").expect("write owned artifact");
    fs::write(&foreign, b"not owned by TempArtifacts").expect("write neighboring artifact");

    drop(artifacts);

    let root_removed = !root.exists();
    let survived = foreign.exists();
    if survived {
        fs::remove_file(&foreign).expect("remove neighboring artifact");
    }
    assert!(root_removed, "owned artifact directory survived drop");
    assert!(
        survived,
        "TempArtifacts deleted a prefix-matching file it did not create"
    );
}
