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
use crate::macho::{self, ObjectFile, Relocation, Section, SectionKind, Symbol};
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
    asm.collect_layout(stmts)?;
    asm.reset_for_emission();
    asm.process(stmts)?;
    asm.finish()
}

/// Assemble a list of pre-encoded instructions into an ObjectFile (compiler API).
/// No parsing needed — the compiler builds Inst values directly.
pub fn assemble_instructions(insts: &[Inst], globals: &[&str]) -> ObjectFile {
    let mut obj = ObjectFile::new();
    let text = obj.text_section_mut();
    for inst in insts {
        let word = inst.encode();
        text.data.extend_from_slice(&word.to_le_bytes());
        text.size += 4;
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
    /// Current section index in `sections`.
    section: usize,
    sections: Vec<Section>,

    /// Labels → (section index, offset within section).
    labels: BTreeMap<String, (usize, u64)>,
    /// Global symbols.
    globals: Vec<String>,
    /// Pending relocations for each section.
    pending_relocs: Vec<Vec<PendingReloc>>,
}

struct PendingReloc {
    section: usize,
    offset: u32,
    symbol: String,
    reloc_type: u32,
    pcrel: bool,
}

impl Assembler {
    fn new() -> Self {
        Self {
            section: 0,
            sections: vec![Section::text()],
            labels: BTreeMap::new(),
            globals: Vec::new(),
            pending_relocs: vec![Vec::new()],
        }
    }

    fn current_offset(&self) -> u64 {
        self.sections[self.section].size
    }

    fn reset_for_emission(&mut self) {
        self.section = 0;
        for section in &mut self.sections {
            section.data.clear();
            section.relocations.clear();
            section.size = 0;
        }
        self.pending_relocs.clear();
        self.pending_relocs.resize_with(self.sections.len(), Vec::new);
    }

    fn collect_layout(&mut self, stmts: &[Stmt]) -> Result<(), AsmError> {
        self.section = 0;

        for stmt in stmts {
            match stmt {
                Stmt::Label(name) => {
                    let offset = self.current_offset();
                    if self.labels.insert(name.clone(), (self.section, offset)).is_some() {
                        return Err(AsmError(format!("duplicate label '{}'", name)));
                    }
                }
                Stmt::Directive(dir) => {
                    self.collect_directive_layout(dir)?;
                }
                Stmt::Instruction(_) | Stmt::InstructionWithReloc(_, _) => {
                    self.reserve_initialized_bytes(4, "instruction")?;
                }
            }
        }

        Ok(())
    }

    fn process(&mut self, stmts: &[Stmt]) -> Result<(), AsmError> {
        for stmt in stmts {
            match stmt {
                Stmt::Label(_) => {}
                Stmt::Directive(dir) => self.process_directive(dir)?,
                Stmt::Instruction(inst) => {
                    let word = inst.encode();
                    self.emit_initialized_bytes(&word.to_le_bytes(), "instruction")?;
                }
                Stmt::InstructionWithReloc(inst, label_ref) => {
                    let offset = self.current_offset() as u32;
                    match label_ref.kind {
                        RelocKind::Page21 | RelocKind::PageOff12 => {
                            let word = inst.encode();
                            self.emit_initialized_bytes(&word.to_le_bytes(), "relocated instruction")?;

                            let reloc_type = match label_ref.kind {
                                RelocKind::Page21 => crate::macho::ARM64_RELOC_PAGE21,
                                RelocKind::PageOff12 => crate::macho::ARM64_RELOC_PAGEOFF12,
                                _ => unreachable!(),
                            };
                            let pcrel = matches!(label_ref.kind, RelocKind::Page21);
                            self.pending_relocs[self.section].push(PendingReloc {
                                section: self.section,
                                offset,
                                symbol: label_ref.symbol.clone(),
                                reloc_type,
                                pcrel,
                            });
                        }
                        RelocKind::Branch26 => {
                            if let Some((section, target)) = self.labels.get(&label_ref.symbol) {
                                self.ensure_same_section(*section, &label_ref.symbol)?;
                                let resolved = self.resolve_branch_fixup(inst, (*target as i64) - (offset as i64), 26)?;
                                self.emit_initialized_bytes(&resolved.encode().to_le_bytes(), "branch instruction")?;
                            } else {
                                let word = inst.encode();
                                self.emit_initialized_bytes(&word.to_le_bytes(), "branch instruction")?;
                                self.pending_relocs[self.section].push(PendingReloc {
                                    section: self.section,
                                    offset,
                                    symbol: label_ref.symbol.clone(),
                                    reloc_type: crate::macho::ARM64_RELOC_BRANCH26,
                                    pcrel: true,
                                });
                            }
                        }
                        RelocKind::Branch19 => {
                            let (section, target) = self.labels.get(&label_ref.symbol)
                                .ok_or_else(|| AsmError(format!(
                                    "branch target '{}' requires an assembler-local label",
                                    label_ref.symbol
                                )))?;
                            self.ensure_same_section(*section, &label_ref.symbol)?;
                            let resolved = self.resolve_branch_fixup(inst, (*target as i64) - (offset as i64), 19)?;
                            self.emit_initialized_bytes(&resolved.encode().to_le_bytes(), "branch instruction")?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn collect_directive_layout(&mut self, dir: &Directive) -> Result<(), AsmError> {
        match dir {
            Directive::Text => self.switch_to("__TEXT", "__text")?,
            Directive::Data => self.switch_to("__DATA", "__data")?,
            Directive::Global(name) => {
                if !self.globals.contains(name) {
                    self.globals.push(name.clone());
                }
            }
            Directive::Align(n) | Directive::P2Align(n) => {
                if *n > 30 {
                    return Err(AsmError(format!("alignment power {} too large (max 30)", n)));
                }
                let section = &mut self.sections[self.section];
                section.align_pow2 = section.align_pow2.max(*n);
                section.size = align_value(section.size, *n);
            }
            Directive::Byte(vals) => self.reserve_initialized_bytes(vals.len() as u64, ".byte")?,
            Directive::Word(vals) => self.reserve_initialized_bytes((vals.len() as u64) * 4, ".word")?,
            Directive::Quad(vals) => self.reserve_initialized_bytes((vals.len() as u64) * 8, ".quad")?,
            Directive::Ascii(bytes) | Directive::Asciz(bytes) => {
                self.reserve_initialized_bytes(bytes.len() as u64, "string directive")?;
            }
            Directive::Space(n) => {
                if *n > 1024 * 1024 * 64 {
                    return Err(AsmError(format!(".space size {} too large (max 64MB)", n)));
                }
                self.sections[self.section].size += *n;
            }
            Directive::Section(seg, sect) => {
                self.switch_to(seg, sect)?;
            }
            Directive::Ignored(_) | Directive::SubsectionsViaSymbols | Directive::BuildVersion { .. } => {}
        }
        Ok(())
    }

    fn process_directive(&mut self, dir: &Directive) -> Result<(), AsmError> {
        match dir {
            Directive::Text => self.switch_to("__TEXT", "__text")?,
            Directive::Data => self.switch_to("__DATA", "__data")?,
            Directive::Global(_) => {}
            Directive::Align(n) | Directive::P2Align(n) => {
                if *n > 30 {
                    return Err(AsmError(format!("alignment power {} too large (max 30)", n)));
                }
                let section = &mut self.sections[self.section];
                section.align_pow2 = section.align_pow2.max(*n);
                let current = self.current_offset();
                let aligned = align_value(current, *n);
                self.emit_space(aligned - current)?;
            }
            Directive::Byte(vals) => self.emit_initialized_bytes(vals, ".byte")?,
            Directive::Word(vals) => {
                for v in vals {
                    self.emit_initialized_bytes(&v.to_le_bytes(), ".word")?;
                }
            }
            Directive::Quad(vals) => {
                for v in vals {
                    self.emit_initialized_bytes(&v.to_le_bytes(), ".quad")?;
                }
            }
            Directive::Ascii(bytes) => self.emit_initialized_bytes(bytes, ".ascii")?,
            Directive::Asciz(bytes) => self.emit_initialized_bytes(bytes, ".asciz")?,
            Directive::Space(n) => {
                if *n > 1024 * 1024 * 64 {
                    return Err(AsmError(format!(".space size {} too large (max 64MB)", n)));
                }
                self.emit_space(*n)?;
            }
            Directive::Section(seg, sect) => {
                self.switch_to(seg, sect)?;
            }
            Directive::Ignored(_) | Directive::SubsectionsViaSymbols | Directive::BuildVersion { .. } => {}
        }
        Ok(())
    }

    fn reserve_initialized_bytes(&mut self, amount: u64, context: &str) -> Result<(), AsmError> {
        if self.sections[self.section].kind == SectionKind::ZeroFill {
            return Err(AsmError(format!(
                "{} is not supported in zero-fill section {},{}",
                context,
                self.sections[self.section].segment,
                self.sections[self.section].name
            )));
        }
        self.sections[self.section].size += amount;
        Ok(())
    }

    fn emit_initialized_bytes(&mut self, bytes: &[u8], context: &str) -> Result<(), AsmError> {
        self.reserve_initialized_bytes(bytes.len() as u64, context)?;
        self.sections[self.section].data.extend_from_slice(bytes);
        Ok(())
    }

    fn emit_space(&mut self, amount: u64) -> Result<(), AsmError> {
        if self.sections[self.section].kind == SectionKind::ZeroFill {
            self.sections[self.section].size += amount;
            return Ok(());
        }
        let section = &mut self.sections[self.section];
        let new_len = section.data.len() + amount as usize;
        section.data.resize(new_len, 0);
        section.size += amount;
        Ok(())
    }

    fn switch_to(&mut self, seg: &str, sect: &str) -> Result<(), AsmError> {
        self.section = self.ensure_section(seg, sect)?;
        Ok(())
    }

    fn ensure_section(&mut self, seg: &str, sect: &str) -> Result<usize, AsmError> {
        let (segment, name, kind) = Self::supported_section(seg, sect)?;
        if let Some((index, _)) = self.sections.iter().enumerate().find(|(_, section)| {
            section.segment.eq_ignore_ascii_case(segment) && section.name.eq_ignore_ascii_case(name)
        }) {
            return Ok(index);
        }
        self.sections.push(Section::new(segment, name, kind));
        self.pending_relocs.push(Vec::new());
        Ok(self.sections.len() - 1)
    }

    fn supported_section(seg: &str, sect: &str) -> Result<(&'static str, &'static str, SectionKind), AsmError> {
        let seg_lower = seg.to_lowercase();
        let sect_lower = sect.to_lowercase();
        if seg_lower == "__text" && sect_lower == "__text" {
            Ok(("__TEXT", "__text", SectionKind::Text))
        } else if seg_lower == "__text" && sect_lower == "__cstring" {
            Ok(("__TEXT", "__cstring", SectionKind::CStringLiterals))
        } else if seg_lower == "__text" && sect_lower == "__const" {
            Ok(("__TEXT", "__const", SectionKind::ConstData))
        } else if seg_lower == "__data" && sect_lower == "__data" {
            Ok(("__DATA", "__data", SectionKind::Data))
        } else if seg_lower == "__data" && sect_lower == "__bss" {
            Ok(("__DATA", "__bss", SectionKind::ZeroFill))
        } else {
            Err(AsmError(format!(
                "unsupported section {},{} (supported sections: __TEXT,__text, __TEXT,__cstring, __TEXT,__const, __DATA,__data, __DATA,__bss)",
                seg, sect
            )))
        }
    }

    fn ensure_same_section(&self, target_section: usize, symbol: &str) -> Result<(), AsmError> {
        if target_section == self.section {
            Ok(())
        } else {
            Err(AsmError(format!(
                "branch target '{}' must be in the current section",
                symbol
            )))
        }
    }

    fn resolve_branch_fixup(&self, inst: &Inst, offset: i64, bits: u8) -> Result<Inst, AsmError> {
        let checked = check_branch_offset(offset, bits)?;
        match inst {
            Inst::B { .. } => Ok(Inst::B { offset: checked }),
            Inst::Bl { .. } => Ok(Inst::Bl { offset: checked }),
            Inst::BCond { cond, .. } => Ok(Inst::BCond { cond: *cond, offset: checked }),
            Inst::Cbz { rt, sf, .. } => Ok(Inst::Cbz { rt: *rt, offset: checked, sf: *sf }),
            Inst::Cbnz { rt, sf, .. } => Ok(Inst::Cbnz { rt: *rt, offset: checked, sf: *sf }),
            _ => Err(AsmError("internal error: invalid branch fixup instruction".into())),
        }
    }

    fn section_base_addresses(&self) -> Vec<u64> {
        let mut bases = Vec::with_capacity(self.sections.len());
        let mut addr = 0u64;
        for section in &self.sections {
            addr = align_value(addr, section.align_pow2);
            bases.push(addr);
            addr += section.size;
        }
        bases
    }

    fn finish(mut self) -> Result<ObjectFile, AsmError> {
        let section_bases = self.section_base_addresses();
        let mut symbols: Vec<Symbol> = Vec::new();

        for (index, base) in section_bases.iter().enumerate() {
            symbols.push(Symbol {
                name: format!("ltmp{}", index),
                section: (index + 1) as u8,
                value: *base,
                global: false,
                undefined: false,
            });
        }

        for (name, (section, offset)) in &self.labels {
            if !self.globals.contains(name) && !name.starts_with("ltmp") {
                let value = section_bases[*section] + offset;
                symbols.push(Symbol {
                    name: name.clone(),
                    section: (*section + 1) as u8,
                    value,
                    global: false,
                    undefined: false,
                });
            }
        }

        // External (global) symbols.
        for name in &self.globals {
            if let Some((section, offset)) = self.labels.get(name) {
                let value = section_bases[*section] + offset;
                symbols.push(Symbol {
                    name: name.clone(),
                    section: (*section + 1) as u8,
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

        for relocs in &self.pending_relocs {
            for reloc in relocs {
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
        }

        for relocs in self.pending_relocs {
            for pending in relocs {
                let sym_idx = symbols
                    .iter()
                    .position(|s| s.name == pending.symbol)
                    .ok_or_else(|| AsmError(format!("missing relocation symbol '{}'", pending.symbol)))?;
                self.sections[pending.section].relocations.push(Relocation {
                    offset: pending.offset,
                    symbol_idx: sym_idx as u32,
                    pcrel: pending.pcrel,
                    length: 2,
                    extern_: true,
                    reloc_type: pending.reloc_type,
                });
            }
        }

        Ok(ObjectFile { sections: self.sections, symbols })
    }
}

fn align_value(value: u64, power: u32) -> u64 {
    let alignment = 1u64 << power;
    (value + alignment - 1) & !(alignment - 1)
}

fn check_branch_offset(offset: i64, bits: u8) -> Result<i32, AsmError> {
    if offset % 4 != 0 {
        return Err(AsmError(format!("branch offset {} is not 4-byte aligned", offset)));
    }

    let scaled = offset / 4;
    let min = -(1i64 << (bits - 1));
    let max = (1i64 << (bits - 1)) - 1;
    if scaled < min || scaled > max {
        return Err(AsmError(format!("branch offset {} is out of range for {}-bit immediate", offset, bits)));
    }

    Ok(offset as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reg::*;

    fn text_bytes(obj: &ObjectFile) -> &[u8] {
        &obj.text_section().data
    }

    fn data_bytes(obj: &ObjectFile) -> &[u8] {
        &obj.section("__DATA", "__data").expect("missing __DATA,__data").data
    }

    fn text_relocs(obj: &ObjectFile) -> &[Relocation] {
        &obj.text_section().relocations
    }

    #[test]
    fn assemble_nop() {
        let obj = assemble_source(".text\nnop\n").unwrap();
        assert_eq!(text_bytes(&obj), vec![0x1F, 0x20, 0x03, 0xD5]);
    }

    #[test]
    fn assemble_ret() {
        let obj = assemble_source(".text\nret\n").unwrap();
        assert_eq!(text_bytes(&obj), vec![0xC0, 0x03, 0x5F, 0xD6]);
    }

    #[test]
    fn assemble_multiple_instructions() {
        let obj = assemble_source(".text\nadd x0, x1, x2\nsub x3, x4, x5\nret\n").unwrap();
        assert_eq!(text_bytes(&obj).len(), 12); // 3 instructions × 4 bytes
    }

    #[test]
    fn assemble_with_data() {
        let obj = assemble_source(".text\nnop\n.data\n.asciz \"hi\"\n").unwrap();
        assert_eq!(text_bytes(&obj).len(), 4);
        assert_eq!(data_bytes(&obj), b"hi\0");
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
        assert_eq!(text_bytes(&obj).len(), 8);
        assert!(obj.symbols.iter().any(|s| s.name == "_main" && s.global));
    }

    #[test]
    fn assemble_with_alignment() {
        let obj = assemble_source(".text\n.p2align 2\nnop\n").unwrap();
        assert_eq!(obj.text_section().align_pow2, 2);
    }

    #[test]
    fn assemble_data_directive_byte() {
        let obj = assemble_source(".data\n.byte 0x41, 0x42, 0x43\n").unwrap();
        assert_eq!(data_bytes(&obj), vec![0x41, 0x42, 0x43]);
    }

    #[test]
    fn assemble_data_directive_word() {
        let obj = assemble_source(".data\n.word 42\n").unwrap();
        assert_eq!(data_bytes(&obj), 42u32.to_le_bytes().to_vec());
    }

    #[test]
    fn assemble_data_directive_quad() {
        let obj = assemble_source(".data\n.quad 0xDEADBEEF\n").unwrap();
        assert_eq!(data_bytes(&obj), 0xDEADBEEFu64.to_le_bytes().to_vec());
    }

    #[test]
    fn assemble_space() {
        let obj = assemble_source(".data\n.space 16\n").unwrap();
        assert_eq!(data_bytes(&obj).len(), 16);
        assert!(data_bytes(&obj).iter().all(|&b| b == 0));
    }

    #[test]
    fn assemble_section_switching() {
        let src = ".text\nnop\n.data\n.byte 1\n.text\nret\n.data\n.byte 2\n";
        let obj = assemble_source(src).unwrap();
        assert_eq!(text_bytes(&obj).len(), 8); // nop + ret
        assert_eq!(data_bytes(&obj), vec![1, 2]);
    }

    #[test]
    fn assemble_ignored_directive_does_not_switch_sections() {
        let obj = assemble_source(".data\n.byte 1\n.cfi_startproc\n.byte 2\n").unwrap();
        assert_eq!(text_bytes(&obj), Vec::<u8>::new());
        assert_eq!(data_bytes(&obj), vec![1, 2]);
    }

    #[test]
    fn assemble_rejects_unsupported_section() {
        let err = assemble_source(".section __TEXT,__foo\n.space 16\n").unwrap_err();
        assert!(err.0.contains("unsupported section"), "got: {}", err.0);
    }

    #[test]
    fn assemble_supported_text_sections() {
        let obj = assemble_source(
            ".section __TEXT,__cstring\nmsg: .asciz \"hello\"\n.section __TEXT,__const\nvalue: .quad 42\n"
        ).unwrap();

        let cstring = obj.section("__TEXT", "__cstring").unwrap();
        let const_data = obj.section("__TEXT", "__const").unwrap();
        assert_eq!(cstring.data, b"hello\0");
        assert_eq!(const_data.data, 42u64.to_le_bytes());
        assert_eq!(obj.symbols.iter().find(|sym| sym.name == "msg").unwrap().value, 0);
        assert_eq!(obj.symbols.iter().find(|sym| sym.name == "value").unwrap().value, 6);
    }

    #[test]
    fn assemble_bss_is_zero_fill() {
        let obj = assemble_source(".section __DATA,__bss\n.p2align 4\nscratch:\n.space 16\n").unwrap();
        let bss = obj.section("__DATA", "__bss").unwrap();
        assert!(bss.data.is_empty());
        assert_eq!(bss.size, 16);
        assert_eq!(bss.align_pow2, 4);
        let scratch = obj.symbols.iter().find(|sym| sym.name == "scratch").unwrap();
        assert_eq!(scratch.value, 0);
    }

    #[test]
    fn assemble_bss_rejects_initialized_data() {
        let err = assemble_source(".section __DATA,__bss\n.byte 1\n").unwrap_err();
        assert!(err.0.contains("zero-fill"), "got: {}", err.0);
    }

    #[test]
    fn assemble_interleaved_sections_keep_independent_offsets() {
        let obj = assemble_source(
            ".text\ncode0:\nnop\n.section __TEXT,__const\nconst0: .quad 1\n.data\ndata0: .byte 7\n.section __DATA,__bss\n.p2align 4\nbss0:\n.space 16\n.text\nret\n"
        ).unwrap();

        let code0 = obj.symbols.iter().find(|sym| sym.name == "code0").unwrap();
        let const0 = obj.symbols.iter().find(|sym| sym.name == "const0").unwrap();
        let data0 = obj.symbols.iter().find(|sym| sym.name == "data0").unwrap();
        let bss0 = obj.symbols.iter().find(|sym| sym.name == "bss0").unwrap();

        assert_eq!(code0.value, 0);
        assert_eq!(const0.value, 8);
        assert_eq!(data0.value, 16);
        assert_eq!(bss0.value, 32);
    }

    #[test]
    fn assemble_page_reloc_uses_target_label_symbol() {
        let obj = assemble_source(
            ".global _main\n.text\n_main:\nadrp x0, second@PAGE\nadd x0, x0, second@PAGEOFF\nret\n.data\nfirst: .byte 1\nsecond: .byte 2\n"
        ).unwrap();

        let reloc_syms: Vec<_> = text_relocs(&obj).iter()
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

        let reloc_syms: Vec<_> = text_relocs(&obj).iter()
            .map(|rel| obj.symbols[rel.symbol_idx as usize].name.as_str())
            .collect();
        assert_eq!(reloc_syms, vec!["_foo", "_foo"]);
    }

    #[test]
    fn assemble_forward_branch_label() {
        let obj = assemble_source(".text\nstart:\nb done\nnop\ndone:\nret\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::B { offset: 8 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_local_cbz_label() {
        let obj = assemble_source(".text\nstart:\ncbz x0, done\nret\ndone:\nret\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::Cbz { rt: X0, offset: 8, sf: true }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_external_bl_creates_branch_relocation() {
        let obj = assemble_source(".text\nbl _puts\nret\n").unwrap();
        let reloc_name = &obj.symbols[text_relocs(&obj)[0].symbol_idx as usize].name;
        assert_eq!(reloc_name, "_puts");
        assert!(obj.symbols.iter().any(|sym| sym.name == "_puts" && sym.undefined));
    }

    #[test]
    fn assemble_branch19_requires_local_label() {
        let err = assemble_source(".text\nb.eq _foo\n").unwrap_err();
        assert!(err.0.contains("assembler-local label"), "got: {}", err.0);
    }
}
