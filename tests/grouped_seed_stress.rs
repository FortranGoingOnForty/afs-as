#[allow(dead_code)]
#[path = "common/corpus.rs"]
mod common;

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct StressPaths {
    root: PathBuf,
    asm: PathBuf,
    ours_obj: PathBuf,
    ref_obj: PathBuf,
    support_obj: PathBuf,
    ours_linked: PathBuf,
    ref_linked: PathBuf,
}

impl StressPaths {
    fn new(prefix: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), id));
        fs::create_dir_all(&root).expect("create temp root");
        Self {
            asm: root.join("input.s"),
            ours_obj: root.join("ours.o"),
            ref_obj: root.join("ref.o"),
            support_obj: root.join("support.o"),
            ours_linked: root.join("ours-linked.o"),
            ref_linked: root.join("ref-linked.o"),
            root,
        }
    }
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0xD1B5_4A32_D192_ED03)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as u32
    }

    fn bounded(&mut self, upper: u32) -> u32 {
        self.next_u32() % upper
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

fn generate_metadata_heavy_case(seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let mut src = String::new();
    let common_value = 9 + rng.bounded(11) as u64;
    let hidden_value = 2 + rng.bounded(5) as u64;
    let local_value = 6 + rng.bounded(7) as u64;
    let tls_init = 3 + rng.bounded(9) as u64;
    let addend = 4 * (1 + rng.bounded(3) as i64);

    let _ = writeln!(src, ".build_version macos, 11, 0 sdk_version 15, 5");
    let _ = writeln!(src, ".subsections_via_symbols");
    let _ = writeln!(src, ".globl _meta_{}", seed);
    let _ = writeln!(src, ".private_extern _hidden_{}", seed);
    let _ = writeln!(src, ".weak_definition _local_{}", seed);
    let _ = writeln!(src, ".weak_reference _weak_ext");
    let _ = writeln!(src, ".comm _common_{}, 8, 3", seed);
    let _ = writeln!(src, ".section __TEXT,__text,regular,pure_instructions");
    let _ = writeln!(src, ".p2align 2");
    let _ = writeln!(src, "_meta_{}:", seed);
    let _ = writeln!(src, "LlohP0_{}:", seed);
    let _ = writeln!(src, "    adrp x0, msg_{}@PAGE", seed);
    let _ = writeln!(src, "LlohP1_{}:", seed);
    let _ = writeln!(src, "    add x0, x0, msg_{}@PAGEOFF", seed);
    let _ = writeln!(src, "LlohG0_{}:", seed);
    let _ = writeln!(src, "    adrp x8, _ext@GOTPAGE");
    let _ = writeln!(src, "LlohG1_{}:", seed);
    let _ = writeln!(src, "    ldr x8, [x8, _ext@GOTPAGEOFF]");
    let _ = writeln!(src, "    bl _puts");
    let _ = writeln!(src, "    bl _hidden_{}", seed);
    let _ = writeln!(src, "    mov x19, x0");
    let _ = writeln!(src, "    bl _local_{}", seed);
    let _ = writeln!(src, "    add x19, x19, x0");
    let _ = writeln!(src, "    bl _weak_ext");
    let _ = writeln!(src, "    adrp x9, _common_{}@PAGE", seed);
    let _ = writeln!(src, "    add x9, x9, _common_{}@PAGEOFF", seed);
    let _ = writeln!(src, "    mov x10, #{}", common_value);
    let _ = writeln!(src, "    str x10, [x9]");
    let _ = writeln!(src, "    ldr x10, [x9]");
    let _ = writeln!(src, "    add x19, x19, x10");
    let _ = writeln!(src, "    adrp x11, _tls_value_{}@TLVPPAGE", seed);
    let _ = writeln!(src, "    ldr x11, [x11, _tls_value_{}@TLVPPAGEOFF]", seed);
    let _ = writeln!(src, "    ldr x12, [x11]");
    let _ = writeln!(src, "    blr x12");
    let _ = writeln!(src, "    ldr x13, [x11]");
    let _ = writeln!(src, "    add x0, x19, x13");
    let _ = writeln!(src, "    ret");
    let _ = writeln!(src, ".loh AdrpAdd LlohP0_{}, LlohP1_{}", seed, seed);
    let _ = writeln!(src, ".loh AdrpLdrGot LlohG0_{}, LlohG1_{}", seed, seed);
    let _ = writeln!(src);
    let _ = writeln!(src, ".p2align 2");
    let _ = writeln!(src, "_hidden_{}:", seed);
    let _ = writeln!(src, "    mov x0, #{}", hidden_value);
    let _ = writeln!(src, "    ret");
    let _ = writeln!(src);
    let _ = writeln!(src, ".p2align 2");
    let _ = writeln!(src, "_local_{}:", seed);
    let _ = writeln!(src, "    mov x0, #{}", local_value);
    let _ = writeln!(src, "    ret");
    let _ = writeln!(src);
    let _ = writeln!(src, ".section __TEXT,__cstring,cstring_literals");
    let _ = writeln!(src, "msg_{}:", seed);
    let _ = writeln!(
        src,
        "    .asciz \"meta-stress-{}-{:08x}\"",
        seed,
        rng.next_u32()
    );
    let _ = writeln!(src);
    let _ = writeln!(src, ".section __TEXT,__const");
    let _ = writeln!(src, ".p2align 3");
    let _ = writeln!(src, "meta_const0_{}:", seed);
    let _ = writeln!(src, "    .quad msg_{}", seed);
    let _ = writeln!(src, "meta_const1_{}:", seed);
    let _ = writeln!(src, "    .quad _other - _ext + {}", addend);
    let _ = writeln!(src, "meta_const2_{}:", seed);
    let _ = writeln!(src, "    .quad _puts@GOT");
    let _ = writeln!(src);
    let _ = writeln!(src, ".data");
    let _ = writeln!(src, ".p2align 3");
    let _ = writeln!(src, "meta_data0_{}:", seed);
    let _ = writeln!(src, "    .quad meta_const0_{}", seed);
    let _ = writeln!(src, "meta_data1_{}:", seed);
    let _ = writeln!(src, "    .quad _common_{}", seed);
    let _ = writeln!(src, "meta_data2_{}:", seed);
    let _ = writeln!(src, "    .quad _weak_ext");
    let _ = writeln!(src);
    let _ = writeln!(src, ".zerofill __DATA,__bss,_scratch_{},16,4", seed);
    let _ = writeln!(src);
    let _ = writeln!(src, ".section __DATA,__thread_data,thread_local_regular");
    let _ = writeln!(src, "_tls_value_{}$tlv$init:", seed);
    let _ = writeln!(src, "    .quad {}", tls_init);
    let _ = writeln!(src);
    let _ = writeln!(
        src,
        ".zerofill __DATA,__thread_bss,_tls_bss_{}$tlv$init,8,3",
        seed
    );
    let _ = writeln!(src);
    let _ = writeln!(src, ".section __DATA,__thread_vars,thread_local_variables");
    let _ = writeln!(src, ".globl _tls_value_{}", seed);
    let _ = writeln!(src, "_tls_value_{}:", seed);
    let _ = writeln!(src, "    .quad __tlv_bootstrap");
    let _ = writeln!(src, "    .quad 0");
    let _ = writeln!(src, "    .quad _tls_value_{}$tlv$init", seed);
    let _ = writeln!(src, ".globl _tls_bss_{}", seed);
    let _ = writeln!(src, "_tls_bss_{}:", seed);
    let _ = writeln!(src, "    .quad __tlv_bootstrap");
    let _ = writeln!(src, "    .quad 0");
    let _ = writeln!(src, "    .quad _tls_bss_{}$tlv$init", seed);

    src
}

fn generate_section_switch_case(seed: u64) -> String {
    let mut rng = Rng::new(seed ^ 0xF00D_BAAD);
    let mut src = String::new();
    let addend = 4 * (1 + rng.bounded(4) as i64);
    let cstring_count = 2 + rng.bounded(2) as usize;

    let _ = writeln!(src, ".build_version macos, 11, 0 sdk_version 15, 5");
    let _ = writeln!(src, ".subsections_via_symbols");
    let _ = writeln!(src, ".section __TEXT,__text,regular,pure_instructions");
    let _ = writeln!(src, ".globl _switch_{}", seed);
    let _ = writeln!(src, ".p2align 2");
    let _ = writeln!(src, "_switch_{}:", seed);
    let _ = writeln!(src, "    adrp x0, cstr0_{}@PAGE", seed);
    let _ = writeln!(src, "    add x0, x0, cstr0_{}@PAGEOFF", seed);
    let _ = writeln!(src, "    adrp x1, switch_data0_{}@PAGE", seed);
    let _ = writeln!(src, "    add x1, x1, switch_data0_{}@PAGEOFF", seed);
    let _ = writeln!(src, "    adrp x2, switch_lit0_{}@PAGE", seed);
    let _ = writeln!(src, "    add x2, x2, switch_lit0_{}@PAGEOFF", seed);
    let _ = writeln!(src, "    ldr x3, [x1]");
    let _ = writeln!(src, "    ldr x4, [x2]");
    let _ = writeln!(src, "    add x0, x3, x4");
    let _ = writeln!(src, "    ret");
    let _ = writeln!(src);

    let _ = writeln!(src, ".section __TEXT,__cstring,cstring_literals");
    for index in 0..cstring_count {
        let _ = writeln!(src, "cstr{}_{}:", index, seed);
        let _ = writeln!(
            src,
            "    .asciz \"switch-{}-{}-{:08x}\"",
            seed,
            index,
            rng.next_u32()
        );
    }
    let _ = writeln!(src);

    let _ = writeln!(src, ".data");
    let _ = writeln!(src, ".p2align 3");
    let _ = writeln!(src, "switch_data0_{}:", seed);
    let _ = writeln!(src, "    .quad cstr0_{}", seed);
    let _ = writeln!(src, "switch_data1_{}:", seed);
    let _ = writeln!(src, "    .quad switch_const0_{}", seed);
    let _ = writeln!(src);

    let _ = writeln!(src, ".section __TEXT,__const");
    let _ = writeln!(src, ".p2align 3");
    let _ = writeln!(src, "switch_const0_{}:", seed);
    let _ = writeln!(src, "    .quad switch_data0_{}", seed);
    let _ = writeln!(src, "switch_const1_{}:", seed);
    let _ = writeln!(src, "    .quad _other - _ext + {}", addend);
    let _ = writeln!(src, "switch_const2_{}:", seed);
    let _ = writeln!(src, "    .quad _puts@GOT");
    let _ = writeln!(src);

    let _ = writeln!(src, ".section __TEXT,__literal16,16byte_literals");
    let _ = writeln!(src, ".p2align 4");
    let _ = writeln!(src, "switch_lit0_{}:", seed);
    let _ = writeln!(src, "    .quad switch_const0_{}", seed);
    let _ = writeln!(src, "    .quad switch_data1_{}", seed);
    let _ = writeln!(src);

    let _ = writeln!(src, ".zerofill __DATA,__bss,_switch_bss_{},24,4", seed);
    let _ = writeln!(src);

    let _ = writeln!(src, ".section __DATA,__thread_data,thread_local_regular");
    let _ = writeln!(src, "_switch_tls_{}$tlv$init:", seed);
    let _ = writeln!(src, "    .quad {}", 1 + rng.bounded(9));
    let _ = writeln!(src);

    let _ = writeln!(src, ".section __DATA,__thread_vars,thread_local_variables");
    let _ = writeln!(src, ".globl _switch_tls_{}", seed);
    let _ = writeln!(src, "_switch_tls_{}:", seed);
    let _ = writeln!(src, "    .quad __tlv_bootstrap");
    let _ = writeln!(src, "    .quad 0");
    let _ = writeln!(src, "    .quad _switch_tls_{}$tlv$init", seed);

    src
}

fn assert_raw_parity(prefix: &str, src: &str) -> StressPaths {
    let paths = StressPaths::new(prefix);
    fs::write(&paths.asm, src).expect("write stress source");
    let obj = afs_as::assemble::assemble_source(src)
        .unwrap_or_else(|err| panic!("afs-as failed for {}\n{}\n{}", prefix, src, err));
    let mut file = fs::File::create(&paths.ours_obj).expect("create afs-as object");
    afs_as::macho::write_macho(&obj, &mut file).expect("write Mach-O object");
    common::assemble_with_system(&paths.asm, &paths.ref_obj);

    let ours = fs::read(&paths.ours_obj).expect("read ours");
    let reference = fs::read(&paths.ref_obj).expect("read reference");
    if ours != reference {
        panic!(
            "raw object mismatch for {}\nroot: {}\n---source---\n{}\n---ours load---\n{}\n---ref load---\n{}\n---ours relocs---\n{}\n---ref relocs---\n{}\n---ours symbols---\n{}\n---ref symbols---\n{}",
            prefix,
            paths.root.display(),
            src,
            common::object_load_commands(&paths.ours_obj),
            common::object_load_commands(&paths.ref_obj),
            common::object_relocations(&paths.ours_obj),
            common::object_relocations(&paths.ref_obj),
            common::object_symbols_verbose(&paths.ours_obj),
            common::object_symbols_verbose(&paths.ref_obj),
        );
    }

    paths
}

fn assert_relocatable_parity(paths: &StressPaths, src: &str) {
    common::assemble_link_support(&paths.support_obj);
    common::link_relocatable_with_system(
        &[&paths.ours_obj, &paths.support_obj],
        &paths.ours_linked,
    );
    common::link_relocatable_with_system(&[&paths.ref_obj, &paths.support_obj], &paths.ref_linked);

    let ours = fs::read(&paths.ours_linked).expect("read ours linked");
    let reference = fs::read(&paths.ref_linked).expect("read ref linked");
    if ours != reference {
        panic!(
            "relocatable mismatch for {}\n---source---\n{}\n---ours undef---\n{}\n---ref undef---\n{}\n---ours load---\n{}\n---ref load---\n{}\n---ours symbols---\n{}\n---ref symbols---\n{}",
            paths.root.display(),
            src,
            normalize_tool_output(&common::object_undefined_symbols(&paths.ours_linked)),
            normalize_tool_output(&common::object_undefined_symbols(&paths.ref_linked)),
            normalize_tool_output(&common::object_load_commands(&paths.ours_linked)),
            normalize_tool_output(&common::object_load_commands(&paths.ref_linked)),
            normalize_tool_output(&common::object_symbols_verbose(&paths.ours_linked)),
            normalize_tool_output(&common::object_symbols_verbose(&paths.ref_linked)),
        );
    }
}

#[test]
fn grouped_metadata_heavy_cases_match_system_as_and_link_relocatable() {
    for seed in 1..=4u64 {
        let src = generate_metadata_heavy_case(seed);
        let paths = assert_raw_parity(&format!("afs_grouped_meta_{}", seed), &src);
        assert_relocatable_parity(&paths, &src);
    }
}

#[test]
fn grouped_section_switch_cases_match_system_as() {
    for seed in 1..=6u64 {
        let src = generate_section_switch_case(seed);
        let _paths = assert_raw_parity(&format!("afs_grouped_switch_{}", seed), &src);
    }
}
