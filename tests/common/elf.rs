//! Shared helpers for the x13 ELF suites: gas discovery, corpus
//! iteration, and the differential comparison policy's normalized
//! object form.
//!
//! gas (GNU as) is the single comparison baseline on both ELF hosts:
//! FreeBSD base `as` does not exist and base ld/readelf are LLVM, so
//! we look for the binutils package's /usr/local/bin/as there; on
//! Linux plain `as` is gas. Suites skip cleanly when no gas is found
//! (macOS, minimal containers).

// Each test binary compiles this module independently and none uses
// every helper; silence per-binary dead-code noise.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use afs_as::elf::{ObjectFile, SymbolPlace, SHT_NOBITS, STT_SECTION};

pub fn gas_path() -> Option<PathBuf> {
    let candidates: &[&str] = if cfg!(target_os = "freebsd") {
        &["/usr/local/bin/as"]
    } else if cfg!(target_os = "linux") {
        &["as"]
    } else {
        &[]
    };
    for cand in candidates {
        if let Ok(out) = Command::new(cand).arg("--version").output() {
            let banner = String::from_utf8_lossy(&out.stdout).to_string();
            if out.status.success() && banner.contains("GNU assembler") {
                return Some(PathBuf::from(cand));
            }
        }
    }
    None
}

pub fn skip(suite: &str, test: &str, reason: &str) {
    eprintln!(
        "\nHARNESS_SKIP suite={} test={} count=1 reason=\"{}\"",
        suite, test, reason
    );
}

pub fn assemble_with_gas(gas: &Path, src: &Path, obj: &Path) {
    let out = Command::new(gas)
        .arg("--64")
        .arg("-o")
        .arg(obj)
        .arg(src)
        .output()
        .expect("run gas");
    assert!(
        out.status.success(),
        "gas failed on {}:\n{}",
        src.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

pub fn corpus_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus_elf");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {}", dir.display(), e))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "s"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "empty ELF corpus at {}", dir.display());
    files
}

/// The differential policy's normalized view of an object. Compared
/// between gas output (lifted through our reader) and our re-emission:
/// section content bytes + type/flags, sorted relocation tuples with
/// symbol NAMES, and the symbol set. Deliberately not represented:
/// sh_offset, section order, symbol order, e_shnum, padding.
#[derive(Debug, PartialEq, Eq)]
pub struct Normalized {
    pub osabi: u8,
    pub machine: u16,
    /// name -> (sh_type, sh_flags, content bytes, nobits size)
    pub sections: BTreeMap<String, (u32, u64, Vec<u8>, u64)>,
    /// (section, offset, r_type, symbol name, addend), sorted.
    pub relocs: Vec<(String, u64, u32, String, i64)>,
    /// (name, bind, typ, place-name, value, size), sorted. SECTION
    /// symbols excluded — they are bookkeeping the reader synthesizes.
    pub symbols: Vec<(String, u8, u8, String, u64, u64)>,
}

/// Rewrite every maximal run of x86 NOP-filler patterns (the gas
/// 1..=11-byte forms) as repeated 0x90. binutils changed the split
/// order for large fills between 2.44 (longest-first) and 2.46
/// (remainder-first); the padding is not architectural output, so
/// the differential compares it modulo that choice. Both sides pass
/// through the same rewrite and pattern lengths are preserved, so
/// real code — including any bytes that happen to look like NOPs —
/// still has to match exactly.
pub fn canonicalize_nop_fill(text: &[u8]) -> Vec<u8> {
    const NOPS: [&[u8]; 11] = [
        &[0x66, 0x66, 0x2e, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
        &[0x66, 0x2e, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
        &[0x66, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
        &[0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
        &[0x0f, 0x1f, 0x80, 0x00, 0x00, 0x00, 0x00],
        &[0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00],
        &[0x0f, 0x1f, 0x44, 0x00, 0x00],
        &[0x0f, 0x1f, 0x40, 0x00],
        &[0x0f, 0x1f, 0x00],
        &[0x66, 0x90],
        &[0x90],
    ];
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    'outer: while i < text.len() {
        for pat in NOPS {
            if text[i..].starts_with(pat) {
                out.resize(out.len() + pat.len(), 0x90);
                i += pat.len();
                continue 'outer;
            }
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

pub fn normalize(obj: &ObjectFile) -> Normalized {
    let mut sections = BTreeMap::new();
    let mut relocs = Vec::new();
    for sec in &obj.sections {
        sections.insert(
            sec.name.clone(),
            (
                sec.sh_type,
                sec.sh_flags,
                if sec.name == ".text" {
                    canonicalize_nop_fill(&sec.data)
                } else {
                    sec.data.clone()
                },
                if sec.sh_type == SHT_NOBITS {
                    sec.nobits_size
                } else {
                    0
                },
            ),
        );
        for r in &sec.relas {
            relocs.push((
                sec.name.clone(),
                r.offset,
                r.r_type,
                obj.symbols[r.symbol].name.clone(),
                r.addend,
            ));
        }
    }
    relocs.sort();
    let mut symbols: Vec<(String, u8, u8, String, u64, u64)> = obj
        .symbols
        .iter()
        .filter(|s| s.typ != STT_SECTION)
        .map(|s| {
            let place = match s.place {
                SymbolPlace::Undef => "<undef>".to_string(),
                SymbolPlace::Abs => "<abs>".to_string(),
                SymbolPlace::Common => "<common>".to_string(),
                SymbolPlace::Section(idx) => obj.sections[idx].name.clone(),
            };
            (s.name.clone(), s.bind, s.typ, place, s.value, s.size)
        })
        .collect();
    symbols.sort();
    Normalized {
        osabi: obj.osabi,
        machine: obj.machine,
        sections,
        relocs,
        symbols,
    }
}

pub struct TempArtifacts {
    pub dir: PathBuf,
    pub stem: String,
}

impl TempArtifacts {
    pub fn new(stem: &str) -> Self {
        Self {
            dir: std::env::temp_dir(),
            stem: format!("{}_{}", stem, std::process::id()),
        }
    }
    pub fn path(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!("{}{}", self.stem, suffix))
    }
}

impl Drop for TempArtifacts {
    fn drop(&mut self) {
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().starts_with(&self.stem) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
}
