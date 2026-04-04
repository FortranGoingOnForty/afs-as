//! Assembler pipeline: source text → ObjectFile.
//!
//! Two-pass assembly:
//! 1. First pass: parse, collect labels and section sizes
//! 2. Second pass: encode instructions with resolved label offsets, emit relocations
//!
//! Also provides the library API for the compiler to call directly.

use std::collections::BTreeMap;
use std::fs;
use std::io::BufWriter;
use std::path::Path;

use crate::encode::Inst;
use crate::macho::{self, ObjectFile, Symbol, Relocation};
use crate::parse::{self, Stmt, Directive, RelocKind};

/// Assemble a source file to a Mach-O object file.
pub fn assemble_file(input: &Path, output: &Path) -> Result<(), AsmError> {
    let src = fs::read_to_string(input)
        .map_err(|e| AsmError(format!("{}: {}", input.display(), e)))?;

    let obj = assemble_source(&src)?;

    let file = fs::File::create(output)
        .map_err(|e| AsmError(format!("{}: {}", output.display(), e)))?;
    let mut w = BufWriter::new(file);
    macho::write_macho(&obj, &mut w)
        .map_err(|e| AsmError(format!("writing {}: {}", output.display(), e)))?;

    Ok(())
}

/// Assemble source text into an ObjectFile (library API).
pub fn assemble_source(src: &str) -> Result<ObjectFile, AsmError> {
    let stmts = parse::parse(src).map_err(|e| AsmError(e.to_string()))?;
    assemble_stmts(&stmts)
}

/// Assemble pre-parsed statements into an ObjectFile.
pub fn assemble_stmts(stmts: &[Stmt]) -> Result<ObjectFile, AsmError> {
    let mut asm = Assembler::new();
    asm.process(stmts)?;
    asm.finish()
}

/// Assemble a list of pre-encoded instructions into an ObjectFile (compiler API).
/// No parsing needed — the compiler builds Inst values directly.
pub fn assemble_instructions(insts: &[Inst], globals: &[&str]) -> ObjectFile {
    let mut obj = ObjectFile::new();
    for inst in insts {
        let word = inst.encode();
        obj.text.extend_from_slice(&word.to_le_bytes());
    }
    // Add a section-start local symbol.
    obj.symbols.push(Symbol {
        name: "ltmp0".into(),
        section: 1,
        value: 0,
        global: false,
        undefined: false,
    });
    for name in globals {
        obj.symbols.push(Symbol {
            name: name.to_string(),
            section: 1,
            value: 0,
            global: true,
            undefined: false,
        });
    }
    obj
}

#[derive(Debug)]
pub struct AsmError(pub String);

impl std::fmt::Display for AsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AsmError {}

/// Internal assembler state.
struct Assembler {
    /// Current section: 0 = text, 1 = data.
    section: usize,

    /// Code bytes for __text.
    text: Vec<u8>,
    /// Data bytes for __data.
    data: Vec<u8>,

    /// Labels → (section, offset).
    labels: BTreeMap<String, (u8, u64)>,
    /// Global symbols.
    globals: Vec<String>,

    /// Pending relocations for the text section.
    text_relocs: Vec<PendingReloc>,
    /// All symbols we'll emit (built during finish()).

    /// Text alignment (power of 2).
    text_align: u32,
}

struct PendingReloc {
    offset: u32,
    symbol: String,
    reloc_type: u32,
    pcrel: bool,
}

impl Assembler {
    fn new() -> Self {
        Self {
            section: 0,
            text: Vec::new(),
            data: Vec::new(),
            labels: BTreeMap::new(),
            globals: Vec::new(),
            text_relocs: Vec::new(),
            text_align: 0,
        }
    }

    fn current_offset(&self) -> u64 {
        match self.section {
            0 => self.text.len() as u64,
            1 => self.data.len() as u64,
            _ => 0,
        }
    }

    fn current_section_num(&self) -> u8 {
        // Mach-O sections are 1-based.
        (self.section + 1) as u8
    }

    fn emit_bytes(&mut self, bytes: &[u8]) {
        match self.section {
            0 => self.text.extend_from_slice(bytes),
            1 => self.data.extend_from_slice(bytes),
            _ => {}
        }
    }

    fn process(&mut self, stmts: &[Stmt]) -> Result<(), AsmError> {
        for stmt in stmts {
            match stmt {
                Stmt::Label(name) => {
                    let section = self.current_section_num();
                    let offset = self.current_offset();
                    self.labels.insert(name.clone(), (section, offset));
                }
                Stmt::Directive(dir) => self.process_directive(dir)?,
                Stmt::Instruction(inst) => {
                    let word = inst.encode();
                    self.emit_bytes(&word.to_le_bytes());
                }
                Stmt::InstructionWithReloc(inst, label_ref) => {
                    let offset = self.current_offset() as u32;
                    let word = inst.encode();
                    self.emit_bytes(&word.to_le_bytes());

                    let reloc_type = match label_ref.kind {
                        RelocKind::Page21 => crate::macho::ARM64_RELOC_PAGE21,
                        RelocKind::PageOff12 => crate::macho::ARM64_RELOC_PAGEOFF12,
                        RelocKind::Branch26 => crate::macho::ARM64_RELOC_BRANCH26,
                    };
                    let pcrel = matches!(label_ref.kind, RelocKind::Page21 | RelocKind::Branch26);
                    self.text_relocs.push(PendingReloc {
                        offset,
                        symbol: label_ref.symbol.clone(),
                        reloc_type,
                        pcrel,
                    });
                }
            }
        }
        Ok(())
    }

    fn process_directive(&mut self, dir: &Directive) -> Result<(), AsmError> {
        match dir {
            Directive::Text => self.section = 0,
            Directive::Data => self.section = 1,
            Directive::Global(name) => self.globals.push(name.clone()),
            Directive::Align(n) | Directive::P2Align(n) => {
                if *n > 30 {
                    return Err(AsmError(format!("alignment power {} too large (max 30)", n)));
                }
                if self.section == 0 {
                    self.text_align = self.text_align.max(*n);
                }
                let alignment = 1u64 << *n;
                let current = self.current_offset();
                let aligned = (current + alignment - 1) & !(alignment - 1);
                let pad = (aligned - current) as usize;
                let zeros = vec![0u8; pad];
                self.emit_bytes(&zeros);
            }
            Directive::Byte(vals) => self.emit_bytes(vals),
            Directive::Word(vals) => {
                for v in vals {
                    self.emit_bytes(&v.to_le_bytes());
                }
            }
            Directive::Quad(vals) => {
                for v in vals {
                    self.emit_bytes(&v.to_le_bytes());
                }
            }
            Directive::Ascii(bytes) => self.emit_bytes(bytes),
            Directive::Asciz(bytes) => self.emit_bytes(bytes),
            Directive::Space(n) => {
                if *n > 1024 * 1024 * 64 {
                    return Err(AsmError(format!(".space size {} too large (max 64MB)", n)));
                }
                let zeros = vec![0u8; *n as usize];
                self.emit_bytes(&zeros);
            }
            Directive::Section(seg, sect) => {
                let seg_lower = seg.to_lowercase();
                let sect_lower = sect.to_lowercase();
                if seg_lower == "__text" && sect_lower == "__text" {
                    self.section = 0;
                } else if seg_lower == "__data" && sect_lower == "__data" {
                    self.section = 1;
                } else {
                    return Err(AsmError(format!(
                        "unsupported section {},{} (only __TEXT,__text and __DATA,__data are currently supported)",
                        seg, sect
                    )));
                }
            }
            Directive::Ignored(_) | Directive::SubsectionsViaSymbols | Directive::BuildVersion { .. } => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<ObjectFile, AsmError> {
        let text_size = self.text.len() as u64;
        let mut symbols: Vec<Symbol> = Vec::new();

        // Local symbols first (required by LC_DYSYMTAB ordering).
        // ltmp0: text section start
        symbols.push(Symbol {
            name: "ltmp0".into(),
            section: 1,
            value: 0,
            global: false,
            undefined: false,
        });

        // ltmp1: data section start (value = text_size, i.e., segment-relative addr)
        if !self.data.is_empty() {
            symbols.push(Symbol {
                name: "ltmp1".into(),
                section: 2,
                value: text_size,
                global: false,
                undefined: false,
            });
        }

        // Local labels that aren't global.
        for (name, (section, offset)) in &self.labels {
            if !self.globals.contains(name) && !name.starts_with("ltmp") {
                // Data section labels need segment-relative addresses.
                let value = if *section == 2 { text_size + offset } else { *offset };
                symbols.push(Symbol {
                    name: name.clone(),
                    section: *section,
                    value,
                    global: false,
                    undefined: false,
                });
            }
        }

        // External (global) symbols.
        for name in &self.globals {
            if let Some((section, offset)) = self.labels.get(name) {
                let value = if *section == 2 { text_size + offset } else { *offset };
                symbols.push(Symbol {
                    name: name.clone(),
                    section: *section,
                    value,
                    global: true,
                    undefined: false,
                });
            } else if !symbols.iter().any(|s| s.name == *name) {
                symbols.push(Symbol {
                    name: name.clone(),
                    section: 0,
                    value: 0,
                    global: true,
                    undefined: true,
                });
            }
        }

        for reloc in &self.text_relocs {
            if !self.labels.contains_key(&reloc.symbol) && !symbols.iter().any(|s| s.name == reloc.symbol) {
                symbols.push(Symbol {
                    name: reloc.symbol.clone(),
                    section: 0,
                    value: 0,
                    global: true,
                    undefined: true,
                });
            }
        }

        // Convert pending relocations.
        let text_relocs = self.text_relocs.into_iter().map(|pr| {
            let sym_idx = symbols
                .iter()
                .position(|s| s.name == pr.symbol)
                .ok_or_else(|| AsmError(format!("missing relocation symbol '{}'", pr.symbol)))?;
            Ok(Relocation {
                offset: pr.offset,
                symbol_idx: sym_idx as u32,
                pcrel: pr.pcrel,
                length: 2,
                extern_: true,
                reloc_type: pr.reloc_type,
            })
        }).collect::<Result<Vec<_>, AsmError>>()?;

        Ok(ObjectFile {
            text: self.text,
            data: self.data,
            symbols,
            text_relocs,
            text_align: self.text_align,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reg::*;

    #[test]
    fn assemble_nop() {
        let obj = assemble_source(".text\nnop\n").unwrap();
        assert_eq!(obj.text, vec![0x1F, 0x20, 0x03, 0xD5]);
    }

    #[test]
    fn assemble_ret() {
        let obj = assemble_source(".text\nret\n").unwrap();
        assert_eq!(obj.text, vec![0xC0, 0x03, 0x5F, 0xD6]);
    }

    #[test]
    fn assemble_multiple_instructions() {
        let obj = assemble_source(".text\nadd x0, x1, x2\nsub x3, x4, x5\nret\n").unwrap();
        assert_eq!(obj.text.len(), 12); // 3 instructions × 4 bytes
    }

    #[test]
    fn assemble_with_data() {
        let obj = assemble_source(".text\nnop\n.data\n.asciz \"hi\"\n").unwrap();
        assert_eq!(obj.text.len(), 4);
        assert_eq!(obj.data, b"hi\0");
    }

    #[test]
    fn assemble_global_symbol() {
        let obj = assemble_source(".global _main\n.text\n_main:\nnop\nret\n").unwrap();
        let main = obj.symbols.iter().find(|s| s.name == "_main").unwrap();
        assert!(main.global);
        assert_eq!(main.section, 1);
        assert_eq!(main.value, 0);
    }

    #[test]
    fn assemble_label_offset() {
        let obj = assemble_source(".text\n.global _start\n_start:\nnop\nfoo:\nadd x0, x1, x2\nret\n").unwrap();
        let foo = obj.symbols.iter().find(|s| s.name == "foo").unwrap();
        assert_eq!(foo.value, 4); // after the nop
        assert!(!foo.global);
    }

    #[test]
    fn assemble_instructions_api() {
        let insts = vec![
            Inst::Nop,
            Inst::Ret { rn: X30 },
        ];
        let obj = assemble_instructions(&insts, &["_main"]);
        assert_eq!(obj.text.len(), 8);
        assert!(obj.symbols.iter().any(|s| s.name == "_main" && s.global));
    }

    #[test]
    fn assemble_with_alignment() {
        let obj = assemble_source(".text\n.p2align 2\nnop\n").unwrap();
        assert_eq!(obj.text_align, 2);
    }

    #[test]
    fn assemble_data_directive_byte() {
        let obj = assemble_source(".data\n.byte 0x41, 0x42, 0x43\n").unwrap();
        assert_eq!(obj.data, vec![0x41, 0x42, 0x43]);
    }

    #[test]
    fn assemble_data_directive_word() {
        let obj = assemble_source(".data\n.word 42\n").unwrap();
        assert_eq!(obj.data, 42u32.to_le_bytes().to_vec());
    }

    #[test]
    fn assemble_data_directive_quad() {
        let obj = assemble_source(".data\n.quad 0xDEADBEEF\n").unwrap();
        assert_eq!(obj.data, 0xDEADBEEFu64.to_le_bytes().to_vec());
    }

    #[test]
    fn assemble_space() {
        let obj = assemble_source(".data\n.space 16\n").unwrap();
        assert_eq!(obj.data.len(), 16);
        assert!(obj.data.iter().all(|&b| b == 0));
    }

    #[test]
    fn assemble_section_switching() {
        let src = ".text\nnop\n.data\n.byte 1\n.text\nret\n.data\n.byte 2\n";
        let obj = assemble_source(src).unwrap();
        assert_eq!(obj.text.len(), 8); // nop + ret
        assert_eq!(obj.data, vec![1, 2]);
    }

    #[test]
    fn assemble_ignored_directive_does_not_switch_sections() {
        let obj = assemble_source(".data\n.byte 1\n.cfi_startproc\n.byte 2\n").unwrap();
        assert_eq!(obj.text, Vec::<u8>::new());
        assert_eq!(obj.data, vec![1, 2]);
    }

    #[test]
    fn assemble_rejects_unsupported_section() {
        let err = assemble_source(".section __DATA,__bss\n.space 16\n").unwrap_err();
        assert!(err.0.contains("unsupported section"), "got: {}", err.0);
    }

    #[test]
    fn assemble_page_reloc_uses_target_label_symbol() {
        let obj = assemble_source(
            ".global _main\n.text\n_main:\nadrp x0, second@PAGE\nadd x0, x0, second@PAGEOFF\nret\n.data\nfirst: .byte 1\nsecond: .byte 2\n"
        ).unwrap();

        let reloc_syms: Vec<_> = obj.text_relocs.iter()
            .map(|rel| obj.symbols[rel.symbol_idx as usize].name.as_str())
            .collect();
        assert_eq!(reloc_syms, vec!["second", "second"]);
    }

    #[test]
    fn assemble_page_reloc_creates_undefined_external_symbol() {
        let obj = assemble_source(
            ".global _main\n.text\n_main:\nadrp x0, _foo@PAGE\nadd x0, x0, _foo@PAGEOFF\nret\n"
        ).unwrap();

        let foo = obj.symbols.iter().find(|sym| sym.name == "_foo").unwrap();
        assert!(foo.undefined);
        assert!(foo.global);

        let reloc_syms: Vec<_> = obj.text_relocs.iter()
            .map(|rel| obj.symbols[rel.symbol_idx as usize].name.as_str())
            .collect();
        assert_eq!(reloc_syms, vec!["_foo", "_foo"]);
    }
}
