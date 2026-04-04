#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct ProbeCase {
    name: &'static str,
    source: &'static str,
    driver: &'static str,
    support: Option<&'static str>,
}

#[derive(Clone, Copy, Default)]
struct StageStatus {
    parse: bool,
    assemble: bool,
    semantics: bool,
    raw: bool,
    link: bool,
    run: bool,
}

struct ProbeResult {
    case: &'static str,
    opt: &'static str,
    status: StageStatus,
    detail: Option<String>,
}

const CASES: &[ProbeCase] = &[
    ProbeCase {
        name: "simple_math",
        source: "simple_math.c",
        driver: "extern int square_plus_three(int);\nint main(void) { return square_plus_three(4) != 19; }\n",
        support: None,
    },
    ProbeCase {
        name: "globals",
        source: "globals.c",
        driver: "extern int read_global_plus_one(void);\nint main(void) { return read_global_plus_one() != 8; }\n",
        support: None,
    },
    ProbeCase {
        name: "struct_global",
        source: "struct_global.c",
        driver: "extern int sum_pair(void);\nint main(void) { return sum_pair() != 3; }\n",
        support: None,
    },
    ProbeCase {
        name: "switch_stmt",
        source: "switch_stmt.c",
        driver: "extern int classify(int);\nint main(void) {\n    return (classify(0) != 10) || (classify(1) != 20) || (classify(2) != 30) || (classify(9) != 40);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "stack_frame",
        source: "stack_frame.c",
        driver: "extern int frame_heavy(int, int, int, int);\nint main(void) { return frame_heavy(5, 6, 7, 8) != 9; }\n",
        support: None,
    },
    ProbeCase {
        name: "ext_global",
        source: "ext_global.c",
        driver: "extern int read_ext_plus_one(void);\nint main(void) { return read_ext_plus_one() != 42; }\n",
        support: Some("int ext_value = 41;\n"),
    },
    ProbeCase {
        name: "extern_puts",
        source: "extern_puts.c",
        driver: "extern int call_puts(void);\nint main(void) { return call_puts() < 0; }\n",
        support: None,
    },
    ProbeCase {
        name: "tls_global",
        source: "tls_global.c",
        driver: "extern int read_tls_plus_one(void);\nint main(void) { return read_tls_plus_one() != 6; }\n",
        support: None,
    },
];

fn probe_source_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("clang_probe")
        .join(name)
}

fn clang_generate_asm(source: &Path, opt: &str, output: &Path) -> Result<(), String> {
    let opt_flag = format!("-{}", opt);
    let result = Command::new("clang")
        .arg("-target")
        .arg("arm64-apple-macos11")
        .arg("-S")
        .arg(&opt_flag)
        .arg("-o")
        .arg(output)
        .arg(source)
        .output()
        .map_err(|err| format!("run clang -S for {}: {}", source.display(), err))?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "clang -S failed for {} {}:\n{}",
            source.display(),
            opt,
            String::from_utf8_lossy(&result.stderr)
        ))
    }
}

fn clang_compile_object(source: &Path, output: &Path) -> Result<(), String> {
    let result = Command::new("clang")
        .arg("-target")
        .arg("arm64-apple-macos11")
        .arg("-c")
        .arg(source)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|err| format!("run clang -c for {}: {}", source.display(), err))?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "clang -c failed for {}:\n{}",
            source.display(),
            String::from_utf8_lossy(&result.stderr)
        ))
    }
}

fn assemble_with_ours(src: &str, output: &Path) -> Result<(), String> {
    let obj = afs_as::assemble::assemble_source(src)
        .map_err(|err| format!("afs-as assemble failed:\n{}", err))?;
    let mut file =
        fs::File::create(output).map_err(|err| format!("create {}: {}", output.display(), err))?;
    afs_as::macho::write_macho(&obj, &mut file)
        .map_err(|err| format!("write {}: {}", output.display(), err))
}

fn assemble_with_system(src_path: &Path, output: &Path) -> Result<(), String> {
    let result = Command::new("as")
        .arg("-o")
        .arg(output)
        .arg(src_path)
        .output()
        .map_err(|err| format!("run system as for {}: {}", src_path.display(), err))?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "system as failed for {}:\n{}",
            src_path.display(),
            String::from_utf8_lossy(&result.stderr)
        ))
    }
}

fn clang_link_binary(objects: &[&Path], output: &Path) -> Result<(), String> {
    let mut cmd = Command::new("clang");
    cmd.arg("-target").arg("arm64-apple-macos11");
    for object in objects {
        cmd.arg(object);
    }
    cmd.arg("-o").arg(output);
    let result = cmd
        .output()
        .map_err(|err| format!("run clang link for {}: {}", output.display(), err))?;
    if result.status.success() {
        Ok(())
    } else {
        Err(format!(
            "clang link failed for {}:\n{}",
            output.display(),
            String::from_utf8_lossy(&result.stderr)
        ))
    }
}

fn compare_object_semantics(ours: &Path, reference: &Path) -> Result<(), String> {
    let ours_text = common::object_text_bytes(ours);
    let ref_text = common::object_text_bytes(reference);
    if ours_text != ref_text {
        return Err(format!(
            "text bytes differ\nours: {:02X?}\nref:  {:02X?}",
            ours_text, ref_text
        ));
    }

    let ours_load = normalize_tool_output(&common::object_load_commands(ours));
    let ref_load = normalize_tool_output(&common::object_load_commands(reference));
    if ours_load != ref_load {
        return Err(format!(
            "load commands differ\n--- ours ---\n{}\n--- ref ---\n{}",
            ours_load, ref_load
        ));
    }

    let ours_relocs = normalize_tool_output(&common::object_relocations(ours));
    let ref_relocs = normalize_tool_output(&common::object_relocations(reference));
    if ours_relocs != ref_relocs {
        return Err(format!(
            "relocations differ\n--- ours ---\n{}\n--- ref ---\n{}",
            ours_relocs, ref_relocs
        ));
    }

    let ours_symbols = normalize_tool_output(&common::object_symbols(ours));
    let ref_symbols = normalize_tool_output(&common::object_symbols(reference));
    if ours_symbols != ref_symbols {
        return Err(format!(
            "symbols differ\n--- ours ---\n{}\n--- ref ---\n{}",
            ours_symbols, ref_symbols
        ));
    }

    let ours_symbols_verbose = normalize_tool_output(&common::object_symbols_verbose(ours));
    let ref_symbols_verbose = normalize_tool_output(&common::object_symbols_verbose(reference));
    if ours_symbols_verbose != ref_symbols_verbose {
        return Err(format!(
            "verbose symbols differ\n--- ours ---\n{}\n--- ref ---\n{}",
            ours_symbols_verbose, ref_symbols_verbose
        ));
    }

    Ok(())
}

fn run_probe(case: &ProbeCase, opt: &'static str) -> ProbeResult {
    let mut result = ProbeResult {
        case: case.name,
        opt,
        status: StageStatus::default(),
        detail: None,
    };

    let paths = common::TempPaths::new("afs_clang_probe");
    let root = paths.asm.parent().expect("temp root").to_path_buf();
    let source = probe_source_path(case.source);
    if let Err(err) = clang_generate_asm(&source, opt, &paths.asm) {
        result.detail = Some(err);
        return result;
    }

    let asm = match fs::read_to_string(&paths.asm) {
        Ok(asm) => asm,
        Err(err) => {
            result.detail = Some(format!("read {}: {}", paths.asm.display(), err));
            return result;
        }
    };

    match afs_as::parse::parse(&asm) {
        Ok(_) => result.status.parse = true,
        Err(err) => {
            result.detail = Some(format!("parse failed:\n{}", err));
            return result;
        }
    }

    if let Err(err) = assemble_with_ours(&asm, &paths.obj) {
        result.detail = Some(err);
        return result;
    }
    if let Err(err) = assemble_with_system(&paths.asm, &paths.ref_obj) {
        result.detail = Some(err);
        return result;
    }
    result.status.assemble = true;

    if let Err(err) = compare_object_semantics(&paths.obj, &paths.ref_obj) {
        result.detail = Some(err);
        return result;
    }
    result.status.semantics = true;

    let ours_obj = match fs::read(&paths.obj) {
        Ok(data) => data,
        Err(err) => {
            result.detail = Some(format!("read {}: {}", paths.obj.display(), err));
            return result;
        }
    };
    let ref_obj = match fs::read(&paths.ref_obj) {
        Ok(data) => data,
        Err(err) => {
            result.detail = Some(format!("read {}: {}", paths.ref_obj.display(), err));
            return result;
        }
    };
    if ours_obj != ref_obj {
        result.detail = Some("raw object bytes differ".into());
        return result;
    }
    result.status.raw = true;

    let driver_c = root.join("driver.c");
    let driver_o = root.join("driver.o");
    if let Err(err) = fs::write(&driver_c, case.driver) {
        result.detail = Some(format!("write {}: {}", driver_c.display(), err));
        return result;
    }
    if let Err(err) = clang_compile_object(&driver_c, &driver_o) {
        result.detail = Some(err);
        return result;
    }

    let mut link_inputs = vec![paths.obj.as_path(), driver_o.as_path()];
    let mut ref_link_inputs = vec![paths.ref_obj.as_path(), driver_o.as_path()];
    let support_o = root.join("support.o");
    if let Some(support_src) = case.support {
        let support_c = root.join("support.c");
        if let Err(err) = fs::write(&support_c, support_src) {
            result.detail = Some(format!("write {}: {}", support_c.display(), err));
            return result;
        }
        if let Err(err) = clang_compile_object(&support_c, &support_o) {
            result.detail = Some(err);
            return result;
        }
        link_inputs.push(support_o.as_path());
        ref_link_inputs.push(support_o.as_path());
    }

    let ref_bin = root.join("ref-out");
    if let Err(err) = clang_link_binary(&link_inputs, &paths.bin) {
        result.detail = Some(err);
        return result;
    }
    if let Err(err) = clang_link_binary(&ref_link_inputs, &ref_bin) {
        result.detail = Some(err);
        return result;
    }
    result.status.link = true;

    let (our_code, our_stdout, our_stderr) = common::run_binary(&paths.bin);
    let (ref_code, ref_stdout, ref_stderr) = common::run_binary(&ref_bin);
    if our_code != 0 || ref_code != 0 {
        result.detail = Some(format!(
            "unexpected exit codes ours={} ref={}\nours stderr:\n{}\nref stderr:\n{}",
            our_code, ref_code, our_stderr, ref_stderr
        ));
        return result;
    }
    if our_stdout != ref_stdout || our_stderr != ref_stderr {
        result.detail = Some(format!(
            "runtime output differs\n--- ours stdout ---\n{}\n--- ref stdout ---\n{}\n--- ours stderr ---\n{}\n--- ref stderr ---\n{}",
            our_stdout, ref_stdout, our_stderr, ref_stderr
        ));
        return result;
    }
    result.status.run = true;

    result
}

fn normalize_tool_output(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim_end().ends_with(".o:") && !line.trim_end().ends_with(":"))
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn mark(value: bool) -> &'static str {
    if value {
        "ok"
    } else {
        "--"
    }
}

fn render_dashboard(results: &[ProbeResult]) -> String {
    let mut out = String::from("case           opt parse asm sem raw link run\n");
    for result in results {
        out.push_str(&format!(
            "{:<14} {:<2} {:<5} {:<3} {:<3} {:<3} {:<4} {:<3}\n",
            result.case,
            result.opt,
            mark(result.status.parse),
            mark(result.status.assemble),
            mark(result.status.semantics),
            mark(result.status.raw),
            mark(result.status.link),
            mark(result.status.run),
        ));
    }

    let failures: Vec<_> = results.iter().filter(|result| !result.status.run).collect();
    if !failures.is_empty() {
        out.push_str("\nFailures:\n");
        for failure in failures {
            out.push_str(&format!(
                "\n[{} {}]\n{}\n",
                failure.case,
                failure.opt,
                failure.detail.as_deref().unwrap_or("missing detail")
            ));
        }
    }

    out
}

#[test]
fn clang_probe_dashboard() {
    let mut results = Vec::new();
    for case in CASES {
        for opt in ["O0", "O2"] {
            results.push(run_probe(case, opt));
        }
    }

    let failures: Vec<_> = results.iter().filter(|result| !result.status.run).collect();
    assert!(
        failures.is_empty(),
        "clang probe dashboard failed:\n{}",
        render_dashboard(&results)
    );
}
