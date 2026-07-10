#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct MixedPaths {
    root: PathBuf,
    main_asm: PathBuf,
    support_asm: PathBuf,
    ours_main: PathBuf,
    ref_main: PathBuf,
    ours_support: PathBuf,
    ref_support: PathBuf,
    ours_linked: PathBuf,
    ref_linked: PathBuf,
    ours_bin: PathBuf,
    ref_bin: PathBuf,
}

impl MixedPaths {
    fn new(prefix: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), id));
        fs::create_dir_all(&root).expect("create temp root");
        Self {
            main_asm: root.join("main.s"),
            support_asm: root.join("support.s"),
            ours_main: root.join("main-ours.o"),
            ref_main: root.join("main-ref.o"),
            ours_support: root.join("support-ours.o"),
            ref_support: root.join("support-ref.o"),
            ours_linked: root.join("ours-linked.o"),
            ref_linked: root.join("ref-linked.o"),
            ours_bin: root.join("ours-bin"),
            ref_bin: root.join("ref-bin"),
            root,
        }
    }
}

fn normalize_tool_output(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_end().ends_with(".o:") && !line.trim_end().ends_with(":"))
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn link_objects_with_system(obj_paths: &[&Path], bin_path: &Path, entry: &str) {
    let sdk = Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
        .expect("run xcrun");
    assert!(sdk.status.success(), "xcrun failed");
    let sdk_path = String::from_utf8_lossy(&sdk.stdout).trim().to_string();

    let mut cmd = Command::new("ld");
    for obj in obj_paths {
        cmd.arg(obj);
    }
    let output = cmd
        .args([
            "-o",
            bin_path.to_str().expect("binary path"),
            "-lSystem",
            "-syslibroot",
            &sdk_path,
            "-e",
            entry,
        ])
        .output()
        .expect("run ld");

    assert!(
        output.status.success(),
        "ld failed for {}: {}",
        bin_path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assemble_fixtures() -> MixedPaths {
    let paths = MixedPaths::new("afs_mixed_link_runtime");
    let main_src = common::read_fixture("mixed_link_runtime_main.s");
    let support_src = common::read_fixture("mixed_link_runtime_support.s");

    fs::write(&paths.main_asm, &main_src).expect("write main fixture");
    fs::write(&paths.support_asm, &support_src).expect("write support fixture");

    common::assemble_with_ours(&main_src, &paths.ours_main);
    common::assemble_with_system(&paths.main_asm, &paths.ref_main);
    common::assemble_with_ours(&support_src, &paths.ours_support);
    common::assemble_with_system(&paths.support_asm, &paths.ref_support);

    paths
}

#[test]
fn mixed_link_runtime_inputs_match_system_as() {
    if !common::native_macho_host(
        "mixed_link_runtime",
        "mixed_link_runtime_inputs_match_system_as",
    ) {
        return;
    }
    let paths = assemble_fixtures();

    assert_eq!(
        fs::read(&paths.ours_main).expect("read ours main"),
        fs::read(&paths.ref_main).expect("read ref main")
    );
    assert_eq!(
        fs::read(&paths.ours_support).expect("read ours support"),
        fs::read(&paths.ref_support).expect("read ref support")
    );
}

#[test]
fn mixed_link_runtime_relocatable_matches_system_as() {
    if !common::native_macho_host(
        "mixed_link_runtime",
        "mixed_link_runtime_relocatable_matches_system_as",
    ) {
        return;
    }
    let paths = assemble_fixtures();

    common::link_relocatable_with_system(
        &[&paths.ours_main, &paths.ours_support],
        &paths.ours_linked,
    );
    common::link_relocatable_with_system(&[&paths.ref_main, &paths.ref_support], &paths.ref_linked);

    let ours_undef = common::object_undefined_symbols(&paths.ours_linked);
    let ref_undef = common::object_undefined_symbols(&paths.ref_linked);
    let ours_symbols = common::object_symbols_verbose(&paths.ours_linked);
    let ref_symbols = common::object_symbols_verbose(&paths.ref_linked);
    let ours_load = common::object_load_commands(&paths.ours_linked);
    let ref_load = common::object_load_commands(&paths.ref_linked);

    for needle in [
        "_common_counter",
        "_scratch_buf",
        "_tls_value",
        "_tls_counter",
        "_helper_step",
        "_shared_value",
        "_local_optional",
    ] {
        assert!(
            normalize_tool_output(&ours_symbols).contains(needle),
            "missing {} in linked symbols\n{}",
            needle,
            ours_symbols
        );
    }
    for needle in ["_puts", "__tlv_bootstrap"] {
        assert!(
            normalize_tool_output(&ours_undef).contains(needle),
            "missing {} in undefined symbols\n{}",
            needle,
            ours_undef
        );
    }

    assert_eq!(
        normalize_tool_output(&ours_undef),
        normalize_tool_output(&ref_undef)
    );
    assert_eq!(
        normalize_tool_output(&ours_symbols),
        normalize_tool_output(&ref_symbols)
    );
    assert_eq!(
        normalize_tool_output(&ours_load),
        normalize_tool_output(&ref_load)
    );
}

#[test]
fn mixed_link_runtime_binary_matches_reference() {
    if !common::native_macho_host(
        "mixed_link_runtime",
        "mixed_link_runtime_binary_matches_reference",
    ) {
        return;
    }
    let paths = assemble_fixtures();

    link_objects_with_system(
        &[&paths.ours_main, &paths.ours_support],
        &paths.ours_bin,
        "_main",
    );
    link_objects_with_system(
        &[&paths.ref_main, &paths.ref_support],
        &paths.ref_bin,
        "_main",
    );

    let ours = common::run_binary(&paths.ours_bin);
    let reference = common::run_binary(&paths.ref_bin);

    assert_eq!(ours, reference);
    assert_eq!(
        ours.0,
        67,
        "unexpected exit code; temp root: {}",
        paths.root.display()
    );
    assert_eq!(ours.1, "", "unexpected stdout; stderr:\n{}", ours.2);
}
