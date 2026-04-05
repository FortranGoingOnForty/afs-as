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
        name: "counted_loop",
        source: "counted_loop.c",
        driver: "extern int sum_to_n(int);\nint main(void) { return sum_to_n(10) != 45; }\n",
        support: None,
    },
    ProbeCase {
        name: "div_mod",
        source: "div_mod.c",
        driver: "extern int div_mod_mix(int, int);\nint main(void) { return div_mod_mix(17, 5) != 5; }\n",
        support: None,
    },
    ProbeCase {
        name: "ext_global",
        source: "ext_global.c",
        driver: "extern int read_ext_plus_one(void);\nint main(void) { return read_ext_plus_one() != 42; }\n",
        support: Some("int ext_value = 41;\n"),
    },
    ProbeCase {
        name: "ext_array",
        source: "ext_array.c",
        driver: "extern int read_ext_array_slot(int);\nint main(void) {\n    return (read_ext_array_slot(0) != 11)\n        || (read_ext_array_slot(1) != 22)\n        || (read_ext_array_slot(2) != 33)\n        || (read_ext_array_slot(7) != 44);\n}\n",
        support: Some("int ext_array[4] = {11, 22, 33, 44};\n"),
    },
    ProbeCase {
        name: "ext_struct_field",
        source: "ext_struct_field.c",
        driver: "extern int read_ext_pair_b(void);\nint main(void) { return read_ext_pair_b() != 17; }\n",
        support: Some("struct Pair { int a; int b; };\nstruct Pair ext_pair = {3, 17};\n"),
    },
    ProbeCase {
        name: "ext_ptr_deref",
        source: "ext_ptr_deref.c",
        driver: "extern int read_ext_ptr(void);\nint main(void) { return read_ext_ptr() != 29; }\n",
        support: Some("int ext_value = 29;\nint *ext_ptr = &ext_value;\n"),
    },
    ProbeCase {
        name: "ext_byte_ptr",
        source: "ext_byte_ptr.c",
        driver: "extern int read_ext_byte3(void);\nint main(void) { return read_ext_byte3() != 77; }\n",
        support: Some("unsigned char ext_storage[] = {1, 2, 3, 77, 5};\nunsigned char *ext_bytes = ext_storage;\n"),
    },
    ProbeCase {
        name: "ext_signed_byte_ptr",
        source: "ext_signed_byte_ptr.c",
        driver: "extern int read_ext_sbyte3(void);\nint main(void) { return read_ext_sbyte3() != -11; }\n",
        support: Some("signed char ext_storage[] = {1, 2, 3, -11, 5};\nsigned char *ext_sbytes = ext_storage;\n"),
    },
    ProbeCase {
        name: "ext_short_ptr",
        source: "ext_short_ptr.c",
        driver: "extern int read_ext_short2(void);\nint main(void) { return read_ext_short2() != 321; }\n",
        support: Some("unsigned short ext_storage[] = {7, 9, 321, 11};\nunsigned short *ext_shorts = ext_storage;\n"),
    },
    ProbeCase {
        name: "ext_signed_short_ptr",
        source: "ext_signed_short_ptr.c",
        driver: "extern int read_ext_short_signed2(void);\nint main(void) { return read_ext_short_signed2() != -321; }\n",
        support: Some("short ext_storage[] = {7, 9, -321, 11};\nshort *ext_shorts_signed = ext_storage;\n"),
    },
    ProbeCase {
        name: "ext_str_index",
        source: "ext_str_index.c",
        driver: "extern int second_ext_char(void);\nint main(void) { return second_ext_char() != 'Q'; }\n",
        support: Some("const char *ext_str = \"zQ\";\n"),
    },
    ProbeCase {
        name: "ext_ptr_to_struct",
        source: "ext_ptr_to_struct.c",
        driver: "extern int read_ext_pair_ptr_b(void);\nint main(void) { return read_ext_pair_ptr_b() != 19; }\n",
        support: Some("struct Pair { int a; int b; };\nstatic struct Pair pair = {8, 19};\nstruct Pair *ext_pair_ptr = &pair;\n"),
    },
    ProbeCase {
        name: "func_ptr",
        source: "func_ptr.c",
        driver: "extern int call_helper_ptr(int);\nint main(void) {\n    return (call_helper_ptr(4) != 15) || (call_helper_ptr(-1) != 0);\n}\n",
        support: Some("int helper(int x) { return x * 3; }\n"),
    },
    ProbeCase {
        name: "func_slot_array",
        source: "func_slot_array.c",
        driver: "extern int call_helper_slot1(int);\nint main(void) {\n    return (call_helper_slot1(2) != 35) || (call_helper_slot1(-3) != 0);\n}\n",
        support: Some("int helper_a(int x) { return x + 1; }\nint helper_b(int x) { return x * 7; }\nint (*helper_slots[2])(int) = {helper_a, helper_b};\n"),
    },
    ProbeCase {
        name: "func_slot",
        source: "func_slot.c",
        driver: "extern int call_helper_slot(int);\nint main(void) {\n    return (call_helper_slot(4) != 30) || (call_helper_slot(-2) != 0);\n}\n",
        support: Some("int helper_impl(int x) { return x * 5; }\nint (*helper_slot)(int) = helper_impl;\n"),
    },
    ProbeCase {
        name: "float_branch",
        source: "float_branch.c",
        driver: "extern double clampish(double, double);\nint main(void) { double got = clampish(2.0, 3.0); return (got < 9.49) || (got > 9.51); }\n",
        support: None,
    },
    ProbeCase {
        name: "vector_add",
        source: "vector_add.c",
        driver: "typedef float v4f __attribute__((vector_size(16)));\nextern v4f add4(v4f, v4f);\nextern void add4_store(float *, const float *, const float *);\nint main(void) {\n    v4f a = {1.0f, 2.0f, 3.0f, 4.0f};\n    v4f b = {10.0f, 20.0f, 30.0f, 40.0f};\n    v4f c = add4(a, b);\n    float out[4] = {0.0f, 0.0f, 0.0f, 0.0f};\n    add4_store(out, (const float *)&a, (const float *)&b);\n    return (c[0] != 11.0f) || (c[1] != 22.0f) || (c[2] != 33.0f) || (c[3] != 44.0f)\n        || (out[0] != 11.0f) || (out[1] != 22.0f) || (out[2] != 33.0f) || (out[3] != 44.0f);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "bitops",
        source: "bitops.c",
        driver: "extern unsigned bit_mix(unsigned);\nint main(void) { return bit_mix(0xABu) != 117u; }\n",
        support: None,
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
    ProbeCase {
        name: "tls_addr",
        source: "tls_addr.c",
        driver: "extern int *tls_value_addr(void);\nint main(void) {\n    int *ptr = tls_value_addr();\n    *ptr = 7;\n    return (*ptr != 7) || (*tls_value_addr() != 7);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "tls_bss_global",
        source: "tls_bss_global.c",
        driver: "extern int bump_tls_counter(int);\nint main(void) {\n    return (bump_tls_counter(4) != 4) || (bump_tls_counter(3) != 7);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "tls_ptr_pass",
        source: "tls_ptr_pass.c",
        driver: "extern int bump_tls_via_ptr(void);\nint main(void) {\n    return (bump_tls_via_ptr() != 2) || (bump_tls_via_ptr() != 4);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomics",
        source: "atomics.c",
        driver: "extern int add_and_fetch(int);\nextern int load_then_store(int);\nint main(void) {\n    return (add_and_fetch(4) != 4) || (load_then_store(7) != 4) || (add_and_fetch(1) != 12);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_fences",
        source: "atomic_fences.c",
        driver: "extern int fence_acquire(int *);\nextern void fence_release(int *, int);\nextern int fence_acq_rel(int *);\nextern int fence_seq_cst(int *);\nint main(void) {\n    int x = 5;\n    if (fence_acquire(&x) != 5) return 1;\n    fence_release(&x, 9);\n    if (x != 9) return 1;\n    if (fence_acq_rel(&x) != 9) return 1;\n    if (fence_seq_cst(&x) != 9) return 1;\n    return 0;\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomics8",
        source: "atomics8.c",
        driver: "extern unsigned char load_then_store8(unsigned char);\nextern unsigned char swap8(unsigned char);\nint main(void) {\n    return (load_then_store8(4) != 0u)\n        || (load_then_store8(3) != 4u)\n        || (swap8(9) != 7u)\n        || (swap8(2) != 9u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomics16",
        source: "atomics16.c",
        driver: "extern unsigned short load_then_store16(unsigned short);\nextern unsigned short swap16(unsigned short);\nint main(void) {\n    return (load_then_store16(400u) != 0u)\n        || (load_then_store16(300u) != 400u)\n        || (swap16(900u) != 700u)\n        || (swap16(200u) != 900u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_fetchadd_narrow",
        source: "atomic_fetchadd_narrow.c",
        driver: "extern unsigned char add8(unsigned char);\nextern unsigned short add16(unsigned short);\nint main(void) {\n    return (add8(4u) != 0u)\n        || (add8(3u) != 4u)\n        || (add16(400u) != 0u)\n        || (add16(300u) != 400u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_cas_narrow",
        source: "atomic_cas_narrow.c",
        driver: "extern unsigned char cas8(unsigned char, unsigned char);\nextern unsigned short cas16(unsigned short, unsigned short);\nint main(void) {\n    return (cas8(0u, 7u) != 0u)\n        || (cas8(0u, 11u) != 7u)\n        || (cas16(0u, 700u) != 0u)\n        || (cas16(0u, 300u) != 700u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_exchange_cas",
        source: "atomic_exchange_cas.c",
        driver: "extern int swap_acqrel(int);\nextern long swap64_acqrel(long);\nextern int cas_acqrel(int, int);\nextern long cas64_acqrel(long, long);\nint main(void) {\n    return (swap_acqrel(4) != 0)\n        || (swap_acqrel(7) != 4)\n        || (cas_acqrel(7, 9) != 7)\n        || (cas_acqrel(0, 11) != 9)\n        || (swap64_acqrel(10) != 0)\n        || (swap64_acqrel(15) != 10)\n        || (cas64_acqrel(15, 21) != 15)\n        || (cas64_acqrel(0, 31) != 21);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_bitops",
        source: "atomic_bitops.c",
        driver: "extern unsigned fetch_or_mask(unsigned);\nextern unsigned fetch_xor_mask(unsigned);\nextern unsigned fetch_and_mask(unsigned);\nint main(void) {\n    return (fetch_or_mask(3u) != 0u)\n        || (fetch_or_mask(4u) != 3u)\n        || (fetch_xor_mask(1u) != 7u)\n        || (fetch_and_mask(5u) != 6u)\n        || (fetch_and_mask(1u) != 4u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_bitops_narrow",
        source: "atomic_bitops_narrow.c",
        driver: "extern unsigned char or8(unsigned char);\nextern unsigned char xor8(unsigned char);\nextern unsigned char and8(unsigned char);\nextern unsigned short or16(unsigned short);\nextern unsigned short xor16(unsigned short);\nextern unsigned short and16(unsigned short);\nint main(void) {\n    return (or8(3u) != 0u)\n        || (xor8(1u) != 3u)\n        || (and8(1u) != 2u)\n        || (or16(0x30u) != 0u)\n        || (xor16(0x10u) != 0x30u)\n        || (and16(0x10u) != 0x20u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_maxmin",
        source: "atomic_maxmin.c",
        driver: "extern int fetch_max_builtin(int);\nextern int fetch_min_builtin(int);\nint main(void) {\n    return (fetch_max_builtin(4) != 0)\n        || (fetch_max_builtin(2) != 4)\n        || (fetch_min_builtin(3) != 4)\n        || (fetch_min_builtin(5) != 3);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "atomic_maxmin_narrow",
        source: "atomic_maxmin_narrow.c",
        driver: "extern unsigned char max8(unsigned char);\nextern unsigned char min8(unsigned char);\nextern unsigned short max16(unsigned short);\nextern unsigned short min16(unsigned short);\nint main(void) {\n    return (max8(7u) != 0u)\n        || (min8(3u) != 7u)\n        || (max16(700u) != 0u)\n        || (min16(300u) != 700u);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "compare_chain",
        source: "compare_chain.c",
        driver: "extern int both_small(int, int);\nint main(void) {\n    return (both_small(1, 2) != 2) || (both_small(3, 9) != 1) || (both_small(30, 40) != 0);\n}\n",
        support: None,
    },
    ProbeCase {
        name: "copy_until_zero",
        source: "copy_until_zero.c",
        driver: "extern int copy_until_zero(char *, const char *);\nint main(void) {\n    char buf[8] = {0};\n    int n = copy_until_zero(buf, \"cat\");\n    return (n != 3) || (buf[0] != 'c') || (buf[1] != 'a') || (buf[2] != 't') || (buf[3] != 0);\n}\n",
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
