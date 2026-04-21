use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempPaths {
    pub asm: PathBuf,
    pub obj: PathBuf,
    pub ref_obj: PathBuf,
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
        }
    }
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

pub fn object_load_commands(path: &Path) -> String {
    tool_output("otool", &["-l", path.to_str().expect("object path")])
}

pub fn object_symbols(path: &Path) -> String {
    tool_output("nm", &["-a", path.to_str().expect("object path")])
}

fn tool_output(tool: &str, args: &[&str]) -> String {
    let output = Command::new(tool)
        .args(args)
        .output()
        .unwrap_or_else(|_| panic!("run {}", tool));
    assert!(output.status.success(), "{} failed", tool);
    String::from_utf8_lossy(&output.stdout).into_owned()
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
