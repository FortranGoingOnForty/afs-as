use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempPaths {
    pub asm: PathBuf,
    pub obj: PathBuf,
    pub ref_obj: PathBuf,
    pub bin: PathBuf,
}

impl TempPaths {
    pub fn new(prefix: &str) -> Self {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), id));
        fs::create_dir_all(&root).expect("create temp root");
        Self {
            asm: root.join("input.s"),
            obj: root.join("ours.o"),
            ref_obj: root.join("ref.o"),
            bin: root.join("out"),
        }
    }
}

pub fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join(name)
}

pub fn read_fixture(name: &str) -> String {
    fs::read_to_string(fixture_path(name)).expect("read fixture")
}

pub fn assemble_with_ours(src: &str, output: &Path) {
    let obj = afs_as::assemble::assemble_source(src).expect("assemble with afs-as");
    let mut file = fs::File::create(output).expect("create afs-as object");
    afs_as::macho::write_macho(&obj, &mut file).expect("write Mach-O object");
}

pub fn assemble_with_system(src_path: &Path, output: &Path) {
    let status = Command::new("as")
        .args([
            "-o",
            output.to_str().expect("object path"),
            src_path.to_str().expect("source path"),
        ])
        .status()
        .expect("run system as");
    assert!(
        status.success(),
        "system as failed for {}",
        src_path.display()
    );
}

pub fn object_text_bytes(path: &Path) -> Vec<u8> {
    let output = Command::new("otool")
        .args(["-t", path.to_str().expect("object path")])
        .output()
        .expect("run otool -t");
    assert!(
        output.status.success(),
        "otool -t failed for {}",
        path.display()
    );
    parse_text_bytes(&String::from_utf8_lossy(&output.stdout))
}

pub fn object_section_bytes(path: &Path, segment: &str, section: &str) -> Vec<u8> {
    let output = Command::new("otool")
        .args(["-s", segment, section, path.to_str().expect("object path")])
        .output()
        .expect("run otool -s");
    assert!(
        output.status.success(),
        "otool -s failed for {}",
        path.display()
    );
    parse_section_bytes(&String::from_utf8_lossy(&output.stdout))
}

pub fn object_relocations(path: &Path) -> String {
    tool_output("otool", &["-rv", path.to_str().expect("object path")])
}

pub fn object_load_commands(path: &Path) -> String {
    tool_output("otool", &["-l", path.to_str().expect("object path")])
}

pub fn object_header(path: &Path) -> String {
    tool_output("otool", &["-hv", path.to_str().expect("object path")])
}

pub fn object_symbols(path: &Path) -> String {
    tool_output("nm", &["-a", path.to_str().expect("object path")])
}

pub fn object_symbols_preserve_order(path: &Path) -> String {
    tool_output("nm", &["-ap", path.to_str().expect("object path")])
}

pub fn object_symbols_verbose(path: &Path) -> String {
    tool_output("nm", &["-m", path.to_str().expect("object path")])
}

pub fn object_string_table(path: &Path) -> Vec<u8> {
    let data = fs::read(path).expect("read object");
    let symtab = parse_symtab(&data);
    data[symtab.stroff..symtab.stroff + symtab.strsize].to_vec()
}

pub fn object_symbol_string_offsets(path: &Path) -> Vec<(String, u32)> {
    let data = fs::read(path).expect("read object");
    let symtab = parse_symtab(&data);
    let mut out = Vec::with_capacity(symtab.nsyms);
    for index in 0..symtab.nsyms {
        let base = symtab.symoff + index * 16;
        let n_strx = u32::from_le_bytes(data[base..base + 4].try_into().expect("n_strx bytes"));
        let name = symbol_name_at(&data, symtab.stroff, n_strx);
        out.push((name, n_strx));
    }
    out
}

pub fn link_with_system(obj_path: &Path, bin_path: &Path, entry: &str) {
    let sdk = Command::new("xcrun")
        .args(["--show-sdk-path"])
        .output()
        .expect("run xcrun");
    assert!(sdk.status.success(), "xcrun failed");
    let sdk_path = String::from_utf8_lossy(&sdk.stdout).trim().to_string();

    let output = Command::new("ld")
        .args([
            obj_path.to_str().expect("object path"),
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
        obj_path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn assemble_link_support(output: &Path) {
    let support_src = "\
.text
.build_version macos, 11, 0 sdk_version 15, 5
.p2align 2
.globl _puts
_puts:
    ret
.p2align 2
.globl _ext
_ext:
    ret
.p2align 2
.globl _other
_other:
    ret
.p2align 2
.globl _exit
_exit:
    ret
";
    let asm_path = output.with_extension("s");
    fs::write(&asm_path, support_src).expect("write link support source");
    assemble_with_system(&asm_path, output);
}

pub fn link_relocatable_with_system(obj_paths: &[&Path], out_path: &Path) {
    let mut cmd = Command::new("ld");
    cmd.arg("-r");
    for path in obj_paths {
        cmd.arg(path);
    }
    cmd.arg("-o").arg(out_path);

    let output = cmd.output().expect("run ld -r");
    assert!(
        output.status.success(),
        "ld -r failed for {}: {}",
        out_path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn object_undefined_symbols(path: &Path) -> String {
    tool_output("nm", &["-u", path.to_str().expect("object path")])
}

pub fn run_binary(bin_path: &Path) -> (i32, String, String) {
    let output = Command::new(bin_path).output().expect("run binary");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn tool_output(tool: &str, args: &[&str]) -> String {
    let output = Command::new(tool)
        .args(args)
        .output()
        .unwrap_or_else(|_| panic!("run {}", tool));
    assert!(output.status.success(), "{} failed", tool);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

struct SymtabInfo {
    symoff: usize,
    nsyms: usize,
    stroff: usize,
    strsize: usize,
}

fn parse_symtab(data: &[u8]) -> SymtabInfo {
    let ncmds = u32::from_le_bytes(data[16..20].try_into().expect("ncmds bytes")) as usize;
    let mut offset = 32usize;
    for _ in 0..ncmds {
        let cmd = u32::from_le_bytes(data[offset..offset + 4].try_into().expect("cmd bytes"));
        let cmdsize = u32::from_le_bytes(
            data[offset + 4..offset + 8]
                .try_into()
                .expect("cmdsize bytes"),
        ) as usize;
        if cmd == 0x02 {
            return SymtabInfo {
                symoff: u32::from_le_bytes(
                    data[offset + 8..offset + 12]
                        .try_into()
                        .expect("symoff bytes"),
                ) as usize,
                nsyms: u32::from_le_bytes(
                    data[offset + 12..offset + 16]
                        .try_into()
                        .expect("nsyms bytes"),
                ) as usize,
                stroff: u32::from_le_bytes(
                    data[offset + 16..offset + 20]
                        .try_into()
                        .expect("stroff bytes"),
                ) as usize,
                strsize: u32::from_le_bytes(
                    data[offset + 20..offset + 24]
                        .try_into()
                        .expect("strsize bytes"),
                ) as usize,
            };
        }
        offset += cmdsize;
    }
    panic!("missing LC_SYMTAB in {}", Path::new("object").display());
}

fn symbol_name_at(data: &[u8], stroff: usize, n_strx: u32) -> String {
    if n_strx == 0 {
        return String::new();
    }
    let start = stroff + n_strx as usize;
    let len = data[start..]
        .iter()
        .position(|&byte| byte == 0)
        .expect("null-terminated symbol");
    String::from_utf8_lossy(&data[start..start + len]).into_owned()
}

fn parse_text_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in text.lines().filter(|line| line.starts_with('0')) {
        for hex in line.split_whitespace().skip(1) {
            if hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
                let word = u32::from_str_radix(hex, 16).expect("parse hex word");
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
    }
    bytes
}

fn parse_section_bytes(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for line in text.lines().filter(|line| line.starts_with('0')) {
        for hex in line.split_whitespace().skip(1) {
            if !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
                continue;
            }
            match hex.len() {
                2 => bytes.push(u8::from_str_radix(hex, 16).expect("parse byte")),
                4 => bytes.extend_from_slice(
                    &u16::from_str_radix(hex, 16)
                        .expect("parse halfword")
                        .to_le_bytes(),
                ),
                8 => bytes.extend_from_slice(
                    &u32::from_str_radix(hex, 16)
                        .expect("parse word")
                        .to_le_bytes(),
                ),
                16 => bytes.extend_from_slice(
                    &u64::from_str_radix(hex, 16)
                        .expect("parse quad")
                        .to_le_bytes(),
                ),
                other => panic!("unexpected hex chunk length {} in otool output", other),
            }
        }
    }
    bytes
}

/// The Mach-O corpus suites assemble ARM64 Apple-dialect fixtures with
/// the SYSTEM assembler and compare/link/run — only a macOS arm64 host
/// can do that. Off-host, print the standard skip notice and have the
/// caller return (same policy clang_probe_dashboard adopted in
/// a02dc89; on FreeBSD/Linux the system `as` is a GNU/x86 assembler
/// and every fixture dies on `.build_version`).
pub fn native_macho_host(suite: &str, test: &str) -> bool {
    if cfg!(target_os = "macos") && cfg!(target_arch = "aarch64") {
        return true;
    }
    eprintln!(
        "\nHARNESS_SKIP suite={} test={} count=1 reason=\"needs a macOS arm64 host toolchain\"",
        suite, test
    );
    false
}
