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

use std::ops::Range;
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
/// section content bytes + type/flags/alignment (with multiplicity),
/// sorted relocation tuples with symbol NAMES, and the symbol set including
/// visibility. Deliberately not represented: sh_offset, section order,
/// symbol order, e_shnum, padding.
#[derive(Debug, PartialEq, Eq)]
pub struct Normalized {
    pub osabi: u8,
    pub machine: u16,
    pub gnu_stack_flags: Option<u64>,
    /// Sorted by all fields. A vector preserves duplicate-name sections.
    pub sections: Vec<NormalizedSection>,
    /// (section, offset, r_type, symbol name, addend), sorted.
    pub relocs: Vec<(String, u64, u32, String, i64)>,
    /// Sorted. SECTION symbols are excluded because they are bookkeeping the
    /// reader synthesizes.
    pub symbols: Vec<NormalizedSymbol>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NormalizedSection {
    pub name: String,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addralign: u64,
    pub data: Vec<u8>,
    pub nobits_size: u64,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct NormalizedSymbol {
    pub name: String,
    pub bind: u8,
    pub typ: u8,
    pub vis: u8,
    pub place: String,
    pub value: u64,
    pub size: u64,
}

const X86_NOP_FILL: [&[u8]; 11] = [
    &[
        0x66, 0x66, 0x2e, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00,
    ],
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

/// Canonicalize only byte ranges proven by the assembler's layout pass to be
/// implicit x86 text-alignment padding. Every byte inside each range must be a
/// recognized GNU NOP-fill sequence; arbitrary corruption is an error.
pub fn canonicalize_nop_padding(text: &[u8], padding: &[Range<usize>]) -> Result<Vec<u8>, String> {
    let mut out = text.to_vec();
    let mut previous_end = 0usize;
    for range in padding {
        if range.start > range.end {
            return Err(format!(
                "invalid text padding range {}..{}",
                range.start, range.end
            ));
        }
        if range.start < previous_end {
            return Err(format!(
                "text padding ranges overlap or are out of order at {}..{}",
                range.start, range.end
            ));
        }
        if range.end > text.len() {
            return Err(format!(
                "text padding range {}..{} exceeds section size {}",
                range.start,
                range.end,
                text.len()
            ));
        }

        let mut offset = range.start;
        while offset < range.end {
            let mut matched = None;
            for pattern in X86_NOP_FILL {
                if text[offset..range.end].starts_with(pattern) {
                    matched = Some(pattern);
                    break;
                }
            }
            let Some(pattern) = matched else {
                return Err(format!(
                    "unrecognized text padding byte 0x{:02x} at offset {} in range {}..{}",
                    text[offset], offset, range.start, range.end
                ));
            };
            out[offset..offset + pattern.len()].fill(0x90);
            offset += pattern.len();
        }
        previous_end = range.end;
    }
    Ok(out)
}

pub fn normalized_text_with_padding(
    obj: &ObjectFile,
    text_nop_padding: &[Range<usize>],
) -> Result<Option<Vec<u8>>, String> {
    obj.section_by_name(".text")
        .map(|section| canonicalize_nop_padding(&section.data, text_nop_padding))
        .transpose()
}

pub fn normalize(obj: &ObjectFile) -> Normalized {
    normalize_impl(obj, None).expect("raw object normalization cannot fail")
}

pub fn normalize_with_text_padding(
    obj: &ObjectFile,
    text_nop_padding: &[Range<usize>],
) -> Result<Normalized, String> {
    normalize_impl(obj, Some(text_nop_padding))
}

fn normalize_impl(
    obj: &ObjectFile,
    text_nop_padding: Option<&[Range<usize>]>,
) -> Result<Normalized, String> {
    let mut sections = Vec::new();
    let mut relocs = Vec::new();
    let mut saw_text = false;
    for sec in &obj.sections {
        let data = if sec.name == ".text" {
            saw_text = true;
            match text_nop_padding {
                Some(padding) => canonicalize_nop_padding(&sec.data, padding)?,
                None => sec.data.clone(),
            }
        } else {
            sec.data.clone()
        };
        sections.push(NormalizedSection {
            name: sec.name.clone(),
            sh_type: sec.sh_type,
            sh_flags: sec.sh_flags,
            sh_addralign: sec.sh_addralign,
            data,
            nobits_size: if sec.sh_type == SHT_NOBITS {
                sec.nobits_size
            } else {
                0
            },
        });
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
    if !saw_text && text_nop_padding.is_some_and(|padding| !padding.is_empty()) {
        return Err("text padding provenance supplied for an object without .text".into());
    }
    sections.sort();
    relocs.sort();
    let mut symbols: Vec<NormalizedSymbol> = obj
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
            NormalizedSymbol {
                name: s.name.clone(),
                bind: s.bind,
                typ: s.typ,
                vis: s.vis,
                place,
                value: s.value,
                size: s.size,
            }
        })
        .collect();
    symbols.sort();
    Ok(Normalized {
        osabi: obj.osabi,
        machine: obj.machine,
        gnu_stack_flags: obj.gnu_stack_flags,
        sections,
        relocs,
        symbols,
    })
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
