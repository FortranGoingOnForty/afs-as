#[path = "common/owned_temp_dir.rs"]
mod owned_temp_dir;

use std::fs;

use owned_temp_dir::OwnedTempDir;

#[test]
fn owned_temp_dir_removes_its_artifact_tree_on_drop() {
    let root = {
        let temp = OwnedTempDir::new("afs_native_cleanup_regression");
        let nested = temp.path("nested");
        fs::create_dir(&nested).expect("create nested artifact directory");
        fs::write(temp.path("input.s"), b"ret\n").expect("write source artifact");
        fs::write(nested.join("output.o"), b"object").expect("write object artifact");
        temp.as_ref().to_path_buf()
    };

    assert!(
        !root.exists(),
        "owned temporary artifact tree survived drop: {}",
        root.display()
    );
}

#[test]
fn owned_temp_dir_removes_artifacts_during_unwind() {
    let mut root = None;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let temp = OwnedTempDir::new("afs_native_cleanup_unwind");
        root = Some(temp.as_ref().to_path_buf());
        fs::write(temp.path("input.s"), b"ret\n").expect("write source artifact");
        panic!("exercise temporary-artifact cleanup during unwind");
    }));

    assert!(outcome.is_err(), "cleanup witness did not unwind");
    let root = root.expect("record temporary artifact root");
    assert!(
        !root.exists(),
        "owned temporary artifact tree survived unwind: {}",
        root.display()
    );
}
