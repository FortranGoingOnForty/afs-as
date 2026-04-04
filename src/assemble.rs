//! Assembler pipeline: source text → ObjectFile.
//!
//! Two-pass assembly:
//! 1. First pass: parse, collect labels and section sizes
//! 2. Second pass: encode instructions with resolved label offsets, emit relocations
//!
//! Also provides the library API for the compiler to call directly.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::encode::Inst;
use crate::expr::{self, ClassifiedExpr, Expr, SymbolValue};
use crate::macho::{self, BuildVersion, ObjectFile, Relocation, Section, SectionKind, Symbol};
use crate::parse::{
    self, BuildVersionDirective, Directive, LabelRef, LinkerOptimizationHintDirective, LocatedStmt,
    RelocKind, Stmt,
};
use crate::reg::{GpReg, SP};

/// Assemble a source file to a Mach-O object file.
pub fn assemble_file(input: &Path, output: &Path) -> Result<(), AsmError> {
    let src = fs::read_to_string(input)
        .map_err(|e| AsmError::new(format!("{}", e)).with_path(input))?;

    let obj = assemble_source(&src)
        .map_err(|e| e.with_source_context(input, &src))?;

    let file = fs::File::create(output)
        .map_err(|e| AsmError::new(format!("{}", e)).with_path(output))?;
    let mut w = BufWriter::new(file);
    macho::write_macho(&obj, &mut w)
        .map_err(|e| AsmError::new(format!("writing output: {}", e)).with_path(output))?;

    Ok(())
}

/// Assemble source text into an ObjectFile (library API).
pub fn assemble_source(src: &str) -> Result<ObjectFile, AsmError> {
    let stmts = parse::parse_with_locations(src).map_err(AsmError::from)?;
    assemble_located_stmts(&stmts)
}

/// Assemble pre-parsed statements into an ObjectFile.
pub fn assemble_stmts(stmts: &[Stmt]) -> Result<ObjectFile, AsmError> {
    let stmts: Vec<_> = stmts
        .iter()
        .cloned()
        .map(|stmt| LocatedStmt { stmt, line: 0, col: 0 })
        .collect();
    assemble_located_stmts(&stmts)
}

fn assemble_located_stmts(stmts: &[LocatedStmt]) -> Result<ObjectFile, AsmError> {
    let mut asm = Assembler::new();
    asm.collect_layout(stmts)?;
    asm.prepare_expression_state(stmts)?;
    asm.reset_for_emission();
    asm.process(stmts)?;
    asm.resolve_fixups()?;
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
    text.has_instructions = !insts.is_empty();
    // Add a section-start local symbol.
    obj.symbols.push(Symbol {
        name: "ltmp0".into(),
        section: 1,
        value: 0,
        global: false,
        undefined: false,
        absolute: false,
        common: false,
        common_align_pow2: 0,
        private_extern: false,
        weak_ref: false,
        weak_def: false,
    });
    for name in globals {
        obj.symbols.push(Symbol {
            name: name.to_string(),
            section: 1,
            value: 0,
            global: true,
            undefined: false,
            absolute: false,
            common: false,
            common_align_pow2: 0,
            private_extern: false,
            weak_ref: false,
            weak_def: false,
        });
    }
    obj
}

#[derive(Debug, Clone)]
pub struct AsmError {
    pub path: Option<PathBuf>,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub msg: String,
    pub snippet: Option<String>,
}

impl AsmError {
    pub fn new(msg: String) -> Self {
        Self { path: None, line: None, col: None, msg, snippet: None }
    }

    pub fn at(line: u32, col: u32, msg: String) -> Self {
        Self {
            path: None,
            line: Some(line),
            col: Some(col),
            msg,
            snippet: None,
        }
    }

    pub fn with_loc_if_absent(mut self, line: u32, col: u32) -> Self {
        if self.line.is_none() && self.col.is_none() && line > 0 && col > 0 {
            self.line = Some(line);
            self.col = Some(col);
        }
        self
    }

    pub fn with_path(mut self, path: &Path) -> Self {
        if self.path.is_none() {
            self.path = Some(path.to_path_buf());
        }
        self
    }

    pub fn with_source_context(mut self, path: &Path, src: &str) -> Self {
        self = self.with_path(path);
        if self.snippet.is_none() {
            if let Some(line) = self.line {
                self.snippet = src
                    .lines()
                    .nth(line.saturating_sub(1) as usize)
                    .map(|line| line.to_string());
            }
        }
        self
    }
}

#[allow(non_snake_case)]
fn AsmError(msg: String) -> AsmError {
    AsmError::new(msg)
}

impl std::fmt::Display for AsmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.path, self.line, self.col) {
            (Some(path), Some(line), Some(col)) => {
                writeln!(f, "{}:{}:{}: error: {}", path.display(), line, col, self.msg)?;
                if let Some(snippet) = &self.snippet {
                    writeln!(f, "{}", snippet)?;
                    write!(f, "{}^", " ".repeat(col.saturating_sub(1) as usize))?;
                }
                Ok(())
            }
            (Some(path), _, _) => write!(f, "{}: error: {}", path.display(), self.msg),
            (None, Some(line), Some(col)) => write!(f, "{}:{}: error: {}", line, col, self.msg),
            _ => write!(f, "error: {}", self.msg),
        }
    }
}

impl std::error::Error for AsmError {}

impl From<parse::ParseError> for AsmError {
    fn from(err: parse::ParseError) -> Self {
        AsmError::at(err.line, err.col, err.msg)
    }
}

/// Internal assembler state.
struct Assembler {
    /// Current section index in `sections`.
    section: usize,
    current_line: u32,
    current_col: u32,
    sections: Vec<Section>,

    /// Labels → (section index, offset within section).
    labels: BTreeMap<String, (usize, u64)>,
    /// Absolute symbol assignments declared via `.set` / `.equ`.
    absolute_defs: BTreeMap<String, Expr>,
    absolute_symbols: BTreeMap<String, i64>,
    common_symbols: BTreeMap<String, CommonSymbol>,
    symbol_order: BTreeMap<String, usize>,
    next_symbol_order: usize,
    section_bases: Vec<u64>,
    /// Symbol attributes declared via directives.
    symbol_attrs: BTreeMap<String, SymbolAttrs>,
    /// Unresolved fixups captured during emission.
    fixups: Vec<Fixup>,
    /// Pending relocations for each section.
    pending_relocs: Vec<Vec<PendingReloc>>,
    subsections_via_symbols: bool,
    build_version: Option<BuildVersionDirective>,
    linker_optimization_hints: Vec<LinkerOptimizationHint>,
    active_cfi_proc: Option<CfiProcState>,
    compact_unwind_rows: Vec<CompactUnwindRow>,
    eh_frame_rows: Vec<EhFrameRecord>,
}

#[derive(Debug, Clone, Copy, Default)]
struct SymbolAttrs {
    global: bool,
    private_extern: bool,
    weak_ref: bool,
    weak_def: bool,
}

#[derive(Debug, Clone, Copy)]
struct CommonSymbol {
    size: u64,
    align_pow2: u8,
}

#[derive(Debug, Clone)]
struct Fixup {
    section: usize,
    offset: u32,
    line: u32,
    col: u32,
    expr: Expr,
    kind: FixupKind,
}

#[derive(Debug, Clone)]
enum FixupKind {
    Branch26(Inst),
    Branch19(Inst),
    Branch14(Inst),
    Literal19(Inst),
    Adr21(Inst),
    Page21,
    GotLoadPage21,
    TlvpLoadPage21,
    PageOff12,
    GotLoadPageOff12,
    TlvpLoadPageOff12,
    Data32,
    Data64,
}

struct PendingReloc {
    section: usize,
    offset: u32,
    target: PendingRelocTarget,
    length: u8,
    reloc_type: u32,
    pcrel: bool,
    extern_: bool,
}

#[derive(Debug, Clone)]
enum PendingRelocTarget {
    Symbol(String),
    Raw(u32),
}

fn label_ref_expr(label_ref: &LabelRef) -> Expr {
    if label_ref.addend == 0 {
        Expr::Symbol(label_ref.symbol.clone())
    } else {
        Expr::Add(
            Box::new(Expr::Symbol(label_ref.symbol.clone())),
            Box::new(Expr::Int(label_ref.addend)),
        )
    }
}

fn encode_addend_symbolnum(addend: i64) -> Result<u32, AsmError> {
    const MIN: i64 = -(1 << 23);
    const MAX: i64 = (1 << 23) - 1;
    if !(MIN..=MAX).contains(&addend) {
        return Err(AsmError(format!(
            "relocation addend {} is out of range for ARM64_RELOC_ADDEND",
            addend
        )));
    }
    Ok((addend as i32 as u32) & 0x00FF_FFFF)
}

#[derive(Debug, Clone)]
struct LinkerOptimizationHint {
    kind: String,
    labels: Vec<String>,
}

#[derive(Debug, Clone)]
struct CfiProcState {
    start_section: usize,
    start_offset: u64,
    function_symbol: String,
    cfa_register: GpReg,
    cfa_offset: i64,
    saved_gp_offsets: BTreeMap<u8, i64>,
    compact_unwind_forbidden: bool,
    events: Vec<CfiEvent>,
}

#[derive(Debug, Clone, Copy)]
struct CompactUnwindRow {
    start_section: usize,
    start_offset: u64,
    length: u32,
    encoding: u32,
}

#[derive(Debug, Clone)]
struct EhFrameRecord {
    function_symbol: String,
    length: u64,
    instructions: Vec<u8>,
}

#[derive(Debug, Clone)]
struct CfiEvent {
    code_offset: u64,
    op: CfiOp,
}

#[derive(Debug, Clone, Copy)]
enum CfiOp {
    DefCfa { register: GpReg, offset: u64 },
    DefCfaOffset(u64),
    DefCfaRegister(GpReg),
    Offset { register: GpReg, offset: u64 },
    Restore(GpReg),
}

const UNWIND_ARM64_MODE_FRAMELESS: u32 = 0x02000000;
const UNWIND_ARM64_MODE_DWARF: u32 = 0x03000000;
const UNWIND_ARM64_MODE_FRAME: u32 = 0x04000000;
const UNWIND_ARM64_FRAME_X19_X20_PAIR: u32 = 0x00000001;
const UNWIND_ARM64_FRAME_X21_X22_PAIR: u32 = 0x00000002;
const UNWIND_ARM64_FRAME_X23_X24_PAIR: u32 = 0x00000004;
const UNWIND_ARM64_FRAME_X25_X26_PAIR: u32 = 0x00000008;
const UNWIND_ARM64_FRAME_X27_X28_PAIR: u32 = 0x00000010;

impl CfiProcState {
    fn new(start_section: usize, start_offset: u64, function_symbol: String) -> Self {
        Self {
            start_section,
            start_offset,
            function_symbol,
            cfa_register: SP,
            cfa_offset: 0,
            saved_gp_offsets: BTreeMap::new(),
            compact_unwind_forbidden: false,
            events: Vec::new(),
        }
    }

    fn code_offset(&self, current_offset: u64) -> u64 {
        current_offset.saturating_sub(self.start_offset)
    }

    fn push_event(&mut self, current_offset: u64, op: CfiOp) {
        self.events.push(CfiEvent {
            code_offset: self.code_offset(current_offset),
            op,
        });
    }

    fn compact_unwind_encoding(&self) -> Result<Option<u32>, AsmError> {
        if self.compact_unwind_forbidden {
            return Ok(None);
        }

        if self.cfa_register == SP {
            if !self.saved_gp_offsets.is_empty() {
                return Ok(None);
            }
            if self.cfa_offset < 0 {
                return Err(AsmError("compact unwind stack size must be non-negative".into()));
            }
            let stack_size = self.cfa_offset as u64;
            if !stack_size.is_multiple_of(16) {
                return Err(AsmError(format!(
                    "compact unwind frameless stack size {} must be a multiple of 16",
                    stack_size
                )));
            }
            let scaled = stack_size / 16;
            if scaled > 0xFFF {
                return Err(AsmError(format!(
                    "compact unwind frameless stack size {} is too large",
                    stack_size
                )));
            }
            return Ok(Some(UNWIND_ARM64_MODE_FRAMELESS | ((scaled as u32) << 12)));
        }

        if self.cfa_register.num() != 29 {
            return Ok(None);
        }
        if self.cfa_offset != 16 {
            return Ok(None);
        }
        if self.saved_gp_offsets.get(&29) != Some(&-16) || self.saved_gp_offsets.get(&30) != Some(&-8) {
            return Ok(None);
        }

        let mut encoding = UNWIND_ARM64_MODE_FRAME;
        let mut next_offset = -24;
        for (low, high, bit) in [
            (19u8, 20u8, UNWIND_ARM64_FRAME_X19_X20_PAIR),
            (21u8, 22u8, UNWIND_ARM64_FRAME_X21_X22_PAIR),
            (23u8, 24u8, UNWIND_ARM64_FRAME_X23_X24_PAIR),
            (25u8, 26u8, UNWIND_ARM64_FRAME_X25_X26_PAIR),
            (27u8, 28u8, UNWIND_ARM64_FRAME_X27_X28_PAIR),
        ] {
            match (
                self.saved_gp_offsets.get(&low),
                self.saved_gp_offsets.get(&high),
            ) {
                (None, None) => {}
                (Some(&low_offset), Some(&high_offset))
                    if low_offset == next_offset && high_offset == next_offset - 8 =>
                {
                    encoding |= bit;
                    next_offset -= 16;
                }
                (Some(_), Some(_)) => return Ok(None),
                _ => return Ok(None),
            }
        }

        for reg in self.saved_gp_offsets.keys() {
            if !matches!(*reg, 19..=30) {
                return Ok(None);
            }
        }

        Ok(Some(encoding))
    }

    fn eh_frame_record(&self, current_offset: u64) -> Result<EhFrameRecord, AsmError> {
        let mut instructions = Vec::new();
        let mut previous_code_offset = 0u64;
        for event in &self.events {
            if event.code_offset < previous_code_offset {
                return Err(AsmError("CFI events regressed in code offset".into()));
            }
            emit_advance_loc(&mut instructions, event.code_offset - previous_code_offset)?;
            emit_cfi_op(&mut instructions, event.op)?;
            previous_code_offset = event.code_offset;
        }

        Ok(EhFrameRecord {
            function_symbol: self.function_symbol.clone(),
            length: current_offset
                .checked_sub(self.start_offset)
                .ok_or_else(|| AsmError("internal error: invalid CFI range length".into()))?,
            instructions,
        })
    }
}

impl Assembler {
    fn new() -> Self {
        Self {
            section: 0,
            current_line: 0,
            current_col: 0,
            sections: vec![Section::text()],
            labels: BTreeMap::new(),
            absolute_defs: BTreeMap::new(),
            absolute_symbols: BTreeMap::new(),
            common_symbols: BTreeMap::new(),
            symbol_order: BTreeMap::from([(String::from("ltmp0"), 0)]),
            next_symbol_order: 1,
            section_bases: Vec::new(),
            symbol_attrs: BTreeMap::new(),
            fixups: Vec::new(),
            pending_relocs: vec![Vec::new()],
            subsections_via_symbols: false,
            build_version: None,
            linker_optimization_hints: Vec::new(),
            active_cfi_proc: None,
            compact_unwind_rows: Vec::new(),
            eh_frame_rows: Vec::new(),
        }
    }

    fn current_offset(&self) -> u64 {
        self.sections[self.section].size
    }

    fn symbol_attrs_mut(&mut self, name: &str) -> &mut SymbolAttrs {
        self.symbol_attrs.entry(name.to_string()).or_default()
    }

    fn note_symbol(&mut self, name: &str) {
        if self.symbol_order.contains_key(name) {
            return;
        }
        let order = self.next_symbol_order;
        self.next_symbol_order += 1;
        self.symbol_order.insert(name.to_string(), order);
    }

    fn note_section_temp(&mut self, index: usize) {
        self.note_symbol(&format!("ltmp{}", index));
    }

    fn reset_for_emission(&mut self) {
        self.section = 0;
        self.current_line = 0;
        self.current_col = 0;
        for section in &mut self.sections {
            section.data.clear();
            section.relocations.clear();
            section.has_instructions = false;
            section.size = 0;
        }
        self.fixups.clear();
        self.pending_relocs.clear();
        self.pending_relocs.resize_with(self.sections.len(), Vec::new);
        self.active_cfi_proc = None;
        self.linker_optimization_hints.clear();
        self.compact_unwind_rows.clear();
        self.eh_frame_rows.clear();
    }

    fn note_stmt_location(&mut self, line: u32, col: u32) {
        self.current_line = line;
        self.current_col = col;
    }

    fn prepare_expression_state(&mut self, _stmts: &[LocatedStmt]) -> Result<(), AsmError> {
        self.section_bases = self.section_base_addresses();
        self.absolute_symbols = self.resolve_absolute_symbols()?;
        Ok(())
    }

    fn collect_layout(&mut self, stmts: &[LocatedStmt]) -> Result<(), AsmError> {
        self.section = 0;

        for stmt in stmts {
            match &stmt.stmt {
                Stmt::Label(name) => {
                    if self.common_symbols.contains_key(name) {
                        return Err(AsmError(format!("duplicate symbol '{}'", name))
                            .with_loc_if_absent(stmt.line, stmt.col));
                    }
                    self.note_symbol(name);
                    let offset = self.current_offset();
                    if self.labels.insert(name.clone(), (self.section, offset)).is_some() {
                        return Err(AsmError(format!("duplicate label '{}'", name))
                            .with_loc_if_absent(stmt.line, stmt.col));
                    }
                }
                Stmt::Directive(dir) => {
                    self.collect_directive_layout(dir)
                        .map_err(|e| e.with_loc_if_absent(stmt.line, stmt.col))?;
                }
                Stmt::Instruction(_) | Stmt::InstructionWithReloc(_, _) => {
                    self.reserve_initialized_bytes(4, "instruction")
                        .map_err(|e| e.with_loc_if_absent(stmt.line, stmt.col))?;
                }
            }
        }

        Ok(())
    }

    fn process(&mut self, stmts: &[LocatedStmt]) -> Result<(), AsmError> {
        for stmt in stmts {
            self.note_stmt_location(stmt.line, stmt.col);
            match &stmt.stmt {
                Stmt::Label(_) => {}
                Stmt::Directive(dir) => {
                    self.process_directive(dir)
                        .map_err(|e| e.with_loc_if_absent(stmt.line, stmt.col))?
                }
                Stmt::Instruction(inst) => {
                    self.sections[self.section].has_instructions = true;
                    let word = inst.encode();
                    self.emit_initialized_bytes(&word.to_le_bytes(), "instruction")
                        .map_err(|e| e.with_loc_if_absent(stmt.line, stmt.col))?;
                }
                Stmt::InstructionWithReloc(inst, label_ref) => {
                    self.sections[self.section].has_instructions = true;
                    let offset = self.current_offset() as u32;
                    self.emit_initialized_bytes(&inst.encode().to_le_bytes(), "fixup instruction")
                        .map_err(|e| e.with_loc_if_absent(stmt.line, stmt.col))?;
                    self.fixups.push(Fixup {
                        section: self.section,
                        offset,
                        line: stmt.line,
                        col: stmt.col,
                        expr: label_ref_expr(label_ref),
                        kind: match label_ref.kind {
                            RelocKind::Page21 => FixupKind::Page21,
                            RelocKind::GotLoadPage21 => FixupKind::GotLoadPage21,
                            RelocKind::TlvpLoadPage21 => FixupKind::TlvpLoadPage21,
                            RelocKind::PageOff12 => FixupKind::PageOff12,
                            RelocKind::GotLoadPageOff12 => FixupKind::GotLoadPageOff12,
                            RelocKind::TlvpLoadPageOff12 => FixupKind::TlvpLoadPageOff12,
                            RelocKind::Branch26 => FixupKind::Branch26(inst.clone()),
                            RelocKind::Branch19 => FixupKind::Branch19(inst.clone()),
                            RelocKind::Branch14 => FixupKind::Branch14(inst.clone()),
                            RelocKind::Literal19 => FixupKind::Literal19(inst.clone()),
                            RelocKind::Adr21 => FixupKind::Adr21(inst.clone()),
                        },
                    });
                }
            }
        }
        Ok(())
    }

    fn collect_directive_layout(&mut self, dir: &Directive) -> Result<(), AsmError> {
        match dir {
            Directive::Text => self.switch_to("__TEXT", "__text")?,
            Directive::Data => self.switch_to("__DATA", "__data")?,
            Directive::Set(name, expr) => {
                self.note_symbol(name);
                self.absolute_defs.insert(name.clone(), expr.clone());
            }
            Directive::Comm { name, size, align_pow2 } => {
                self.record_common_symbol(name, *size, *align_pow2)?;
            }
            Directive::Extern(name) => {
                self.note_symbol(name);
            }
            Directive::Global(name) => {
                self.note_symbol(name);
                self.symbol_attrs_mut(name).global = true;
            }
            Directive::PrivateExtern(name) => {
                self.note_symbol(name);
                let attrs = self.symbol_attrs_mut(name);
                attrs.global = true;
                attrs.private_extern = true;
            }
            Directive::WeakReference(name) => {
                self.note_symbol(name);
                let attrs = self.symbol_attrs_mut(name);
                attrs.global = true;
                attrs.weak_ref = true;
            }
            Directive::WeakDefinition(name) => {
                self.note_symbol(name);
                let attrs = self.symbol_attrs_mut(name);
                attrs.weak_def = true;
            }
            Directive::Align { power, max_skip, .. }
            | Directive::P2Align { power, max_skip, .. } => {
                self.collect_alignment(*power, *max_skip)?;
            }
            Directive::Byte(vals) => self.reserve_initialized_bytes(vals.len() as u64, ".byte")?,
            Directive::Short(vals) => self.reserve_initialized_bytes((vals.len() as u64) * 2, ".short")?,
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
            Directive::Fill { repeat, size, .. } => {
                let total = (*repeat)
                    .checked_mul((*size).into())
                    .ok_or_else(|| AsmError(".fill size overflows u64".into()))?;
                self.reserve_initialized_bytes(total, ".fill")?;
            }
            Directive::Zerofill { segment, section, symbol, size, align_pow2 } => {
                self.reserve_zerofill(segment, section, symbol.as_deref(), *size, *align_pow2)?;
            }
            Directive::CfiStartProc
            | Directive::CfiEndProc
            | Directive::CfiDefCfa { .. }
            | Directive::CfiDefCfaOffset(_)
            | Directive::CfiDefCfaRegister(_)
            | Directive::CfiOffset { .. }
            | Directive::CfiRestore(_)
            | Directive::CfiAdjustCfaOffset(_) => {}
            Directive::Section(seg, sect) => {
                self.switch_to(seg, sect)?;
            }
            Directive::SubsectionsViaSymbols => {
                self.subsections_via_symbols = true;
            }
            Directive::BuildVersion(build_version) => {
                self.record_build_version(build_version)?;
            }
            Directive::LinkerOptimizationHint(hint) => {
                self.record_linker_optimization_hint(hint);
            }
        }
        Ok(())
    }

    fn process_directive(&mut self, dir: &Directive) -> Result<(), AsmError> {
        match dir {
            Directive::Text => self.switch_to("__TEXT", "__text")?,
            Directive::Data => self.switch_to("__DATA", "__data")?,
            Directive::Set(_ , _)
            | Directive::Comm { .. }
            | Directive::Extern(_)
            | Directive::Global(_)
            | Directive::PrivateExtern(_)
            | Directive::WeakReference(_)
            | Directive::WeakDefinition(_) => {}
            Directive::Align { power, fill, max_skip }
            | Directive::P2Align { power, fill, max_skip } => {
                self.emit_alignment(*power, *fill, *max_skip)?;
            }
            Directive::Byte(vals) => {
                for expr in vals {
                    let value = self.require_absolute_expr(expr, ".byte expression")?;
                    self.emit_initialized_bytes(&[(value as u8)], ".byte")?;
                }
            }
            Directive::Short(vals) => {
                for expr in vals {
                    let value = self.require_absolute_expr(expr, ".short expression")?;
                    self.emit_initialized_bytes(&(value as u16).to_le_bytes(), ".short")?;
                }
            }
            Directive::Word(vals) => {
                for expr in vals {
                    match self.classify_expr(expr)? {
                        ClassifiedExpr::Absolute(value) => {
                            self.emit_initialized_bytes(&(value as u32).to_le_bytes(), ".word")?;
                        }
                        ClassifiedExpr::PointerToGot { .. } => {
                            let offset = self.current_offset() as u32;
                            self.emit_initialized_bytes(&0u32.to_le_bytes(), ".word")?;
                            self.fixups.push(Fixup {
                                section: self.section,
                                offset,
                                line: self.current_line,
                                col: self.current_col,
                                expr: expr.clone(),
                                kind: FixupKind::Data32,
                            });
                        }
                        _ => {
                            return Err(AsmError(
                                ".word expression must resolve to an absolute value or pointer-to-GOT relocation"
                                    .into(),
                            ));
                        }
                    }
                }
            }
            Directive::Quad(vals) => {
                for expr in vals {
                    let offset = self.current_offset() as u32;
                    self.emit_initialized_bytes(&0u64.to_le_bytes(), ".quad")?;
                    self.fixups.push(Fixup {
                        section: self.section,
                        offset,
                        line: self.current_line,
                        col: self.current_col,
                        expr: expr.clone(),
                        kind: FixupKind::Data64,
                    });
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
            Directive::Fill { repeat, size, value } => {
                self.emit_fill(*repeat, *size, *value)?;
            }
            Directive::Zerofill { segment, section, size, align_pow2, .. } => {
                self.emit_zerofill(segment, section, *size, *align_pow2)?;
            }
            Directive::CfiStartProc => {
                self.start_cfi_proc()?;
            }
            Directive::CfiEndProc => {
                self.finish_cfi_proc()?;
            }
            Directive::CfiDefCfa { .. }
            | Directive::CfiDefCfaOffset(_)
            | Directive::CfiDefCfaRegister(_)
            | Directive::CfiOffset { .. }
            | Directive::CfiRestore(_)
            | Directive::CfiAdjustCfaOffset(_) => self.apply_cfi_directive(dir)?,
            Directive::Section(seg, sect) => {
                self.switch_to(seg, sect)?;
            }
            Directive::SubsectionsViaSymbols => {
                self.subsections_via_symbols = true;
            }
            Directive::BuildVersion(build_version) => {
                self.record_build_version(build_version)?;
            }
            Directive::LinkerOptimizationHint(hint) => {
                self.record_linker_optimization_hint(hint);
            }
        }
        Ok(())
    }

    fn function_symbol_at(&self, section: usize, offset: u64) -> Option<String> {
        let mut exact: Vec<_> = self.labels
            .iter()
            .filter(|(_, (label_section, label_offset))| *label_section == section && *label_offset == offset)
            .map(|(name, _)| name.as_str())
            .collect();

        exact.sort_unstable();

        exact
            .iter()
            .copied()
            .find(|name| {
                !is_assembler_local_symbol(name)
                    && self.symbol_attrs.get(*name).is_some_and(|attrs| attrs.global)
            })
            .or_else(|| exact.iter().copied().find(|name| !is_assembler_local_symbol(name)))
            .or_else(|| exact.into_iter().next())
            .map(str::to_string)
    }

    fn start_cfi_proc(&mut self) -> Result<(), AsmError> {
        if self.active_cfi_proc.is_some() {
            return Err(AsmError("nested .cfi_startproc directives are not supported".into()));
        }
        if self.sections[self.section].kind != SectionKind::Text {
            return Err(AsmError(
                ".cfi_startproc is only supported in __TEXT,__text".into(),
            ));
        }
        let start_offset = self.current_offset();
        let function_symbol = self
            .function_symbol_at(self.section, start_offset)
            .ok_or_else(|| AsmError(".cfi_startproc must follow a function label".into()))?;
        self.active_cfi_proc = Some(CfiProcState::new(self.section, start_offset, function_symbol));
        Ok(())
    }

    fn apply_cfi_directive(&mut self, dir: &Directive) -> Result<(), AsmError> {
        let current_offset = self.current_offset();
        let proc = self
            .active_cfi_proc
            .as_mut()
            .ok_or_else(|| AsmError("CFI directives require an active .cfi_startproc".into()))?;

        match dir {
            Directive::CfiDefCfa { register, offset } => {
                if *offset < 0 {
                    return Err(AsmError(format!(".cfi_def_cfa offset {} must be non-negative", offset)));
                }
                proc.cfa_register = *register;
                proc.cfa_offset = *offset;
                proc.push_event(
                    current_offset,
                    CfiOp::DefCfa {
                        register: *register,
                        offset: *offset as u64,
                    },
                );
            }
            Directive::CfiDefCfaOffset(offset) => {
                if *offset < 0 {
                    return Err(AsmError(format!(
                        ".cfi_def_cfa_offset {} must be non-negative",
                        offset
                    )));
                }
                proc.cfa_offset = *offset;
                proc.push_event(current_offset, CfiOp::DefCfaOffset(*offset as u64));
            }
            Directive::CfiDefCfaRegister(register) => {
                proc.cfa_register = *register;
                proc.push_event(current_offset, CfiOp::DefCfaRegister(*register));
            }
            Directive::CfiOffset { register, offset } => {
                if *offset > 0 || offset.rem_euclid(8) != 0 {
                    return Err(AsmError(format!(
                        ".cfi_offset for x{} requires a negative 8-byte-aligned offset, got {}",
                        register.num(),
                        offset
                    )));
                }
                proc.saved_gp_offsets.insert(register.num(), *offset);
                proc.push_event(
                    current_offset,
                    CfiOp::Offset {
                        register: *register,
                        offset: (-*offset) as u64,
                    },
                );
            }
            Directive::CfiRestore(register) => {
                proc.saved_gp_offsets.remove(&register.num());
                proc.compact_unwind_forbidden = true;
                proc.push_event(current_offset, CfiOp::Restore(*register));
            }
            Directive::CfiAdjustCfaOffset(delta) => {
                let next_offset = proc
                    .cfa_offset
                    .checked_add(*delta)
                    .ok_or_else(|| AsmError("CFA offset overflows i64".into()))?;
                if next_offset < 0 {
                    return Err(AsmError(format!(
                        ".cfi_adjust_cfa_offset would make CFA offset negative ({})",
                        next_offset
                    )));
                }
                proc.cfa_offset = next_offset;
                proc.compact_unwind_forbidden = true;
                proc.push_event(current_offset, CfiOp::DefCfaOffset(next_offset as u64));
            }
            _ => unreachable!("non-CFI directive passed to apply_cfi_directive"),
        }

        Ok(())
    }

    fn finish_cfi_proc(&mut self) -> Result<(), AsmError> {
        let proc = self
            .active_cfi_proc
            .take()
            .ok_or_else(|| AsmError(".cfi_endproc requires an active .cfi_startproc".into()))?;

        if proc.start_section != self.section {
            return Err(AsmError(
                ".cfi_endproc must be in the same section as .cfi_startproc".into(),
            ));
        }

        let length = self
            .current_offset()
            .checked_sub(proc.start_offset)
            .ok_or_else(|| AsmError("internal error: invalid CFI range length".into()))?;

        let encoding = proc.compact_unwind_encoding()?;
        self.compact_unwind_rows.push(CompactUnwindRow {
            start_section: proc.start_section,
            start_offset: proc.start_offset,
            length: u32::try_from(length)
                .map_err(|_| AsmError("compact unwind function range exceeds u32".into()))?,
            encoding: encoding.unwrap_or(UNWIND_ARM64_MODE_DWARF),
        });

        if encoding.is_none() {
            self.eh_frame_rows.push(proc.eh_frame_record(self.current_offset())?);
        }
        Ok(())
    }

    fn record_build_version(&mut self, build_version: &BuildVersionDirective) -> Result<(), AsmError> {
        match &self.build_version {
            Some(existing) if existing == build_version => Ok(()),
            Some(existing) => Err(AsmError(format!(
                "conflicting .build_version directives: already saw {:?}, then {:?}",
                existing, build_version
            ))),
            None => {
                self.build_version = Some(build_version.clone());
                Ok(())
            }
        }
    }

    fn record_linker_optimization_hint(&mut self, hint: &LinkerOptimizationHintDirective) {
        self.linker_optimization_hints.push(LinkerOptimizationHint {
            kind: hint.kind.clone(),
            labels: hint.labels.clone(),
        });
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

    fn emit_fill(&mut self, repeat: u64, size: u8, value: u64) -> Result<(), AsmError> {
        let byte_count: usize = size.into();
        let total = repeat
            .checked_mul(byte_count as u64)
            .ok_or_else(|| AsmError(".fill size overflows u64".into()))?;
        if total > 1024 * 1024 * 64 {
            return Err(AsmError(format!(".fill size {} too large (max 64MB)", total)));
        }
        if byte_count == 0 {
            return Ok(());
        }
        if byte_count > 8 {
            return Err(AsmError(format!(".fill element size {} too large (max 8)", size)));
        }

        let pattern = value.to_le_bytes();
        for _ in 0..repeat {
            self.emit_initialized_bytes(&pattern[..byte_count], ".fill")?;
        }
        Ok(())
    }

    fn collect_alignment(&mut self, power: u32, max_skip: Option<u64>) -> Result<(), AsmError> {
        if power > 30 {
            return Err(AsmError(format!("alignment power {} too large (max 30)", power)));
        }

        let current = self.current_offset();
        let aligned = align_value(current, power);
        let padding = aligned - current;
        let section = &mut self.sections[self.section];
        section.align_pow2 = section.align_pow2.max(power);
        if max_skip.is_some_and(|limit| padding > limit) {
            return Ok(());
        }
        section.size = aligned;
        Ok(())
    }

    fn emit_alignment(
        &mut self,
        power: u32,
        fill: Option<u8>,
        max_skip: Option<u64>,
    ) -> Result<(), AsmError> {
        if power > 30 {
            return Err(AsmError(format!("alignment power {} too large (max 30)", power)));
        }

        {
            let section = &mut self.sections[self.section];
            section.align_pow2 = section.align_pow2.max(power);
        }

        let current = self.current_offset();
        let aligned = align_value(current, power);
        let padding = aligned - current;
        if max_skip.is_some_and(|limit| padding > limit) {
            return Ok(());
        }

        self.emit_alignment_padding(padding, fill)
    }

    fn emit_alignment_padding(&mut self, padding: u64, fill: Option<u8>) -> Result<(), AsmError> {
        if padding == 0 {
            return Ok(());
        }
        if self.sections[self.section].kind == SectionKind::ZeroFill {
            return self.emit_space(padding);
        }

        if let Some(fill) = fill {
            return self.emit_repeated_byte(fill, padding, "alignment padding");
        }

        if self.sections[self.section].kind != SectionKind::Text {
            return self.emit_space(padding);
        }

        let zero_prefix = ((4 - (self.current_offset() % 4)) % 4).min(padding);
        self.emit_space(zero_prefix)?;

        let remaining = padding - zero_prefix;
        let nop = Inst::Nop.encode().to_le_bytes();
        for _ in 0..(remaining / 4) {
            self.emit_initialized_bytes(&nop, ".p2align")?;
        }
        self.emit_space(remaining % 4)
    }

    fn emit_repeated_byte(&mut self, byte: u8, amount: u64, context: &str) -> Result<(), AsmError> {
        if amount == 0 {
            return Ok(());
        }
        if byte == 0 {
            return self.emit_space(amount);
        }
        self.reserve_initialized_bytes(amount, context)?;
        let section = &mut self.sections[self.section];
        let new_len = section.data.len() + amount as usize;
        section.data.resize(new_len, byte);
        Ok(())
    }

    fn resolve_fixups(&mut self) -> Result<(), AsmError> {
        let fixups = std::mem::take(&mut self.fixups);
        for fixup in fixups {
            let line = fixup.line;
            let col = fixup.col;
            let result = match fixup.kind.clone() {
                FixupKind::Branch26(template) => self.resolve_branch_or_reloc(fixup, template, 26, true),
                FixupKind::Branch19(template) => self.resolve_branch_or_reloc(fixup, template, 19, false),
                FixupKind::Branch14(template) => self.resolve_branch_or_reloc(fixup, template, 14, false),
                FixupKind::Literal19(template) => self.resolve_literal_fixup(fixup, template),
                FixupKind::Adr21(template) => self.resolve_adr_fixup(fixup, template),
                FixupKind::Page21 => self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_PAGE21, true),
                FixupKind::GotLoadPage21 => self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_GOT_LOAD_PAGE21, true),
                FixupKind::TlvpLoadPage21 => self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_TLVP_LOAD_PAGE21, true),
                FixupKind::PageOff12 => self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_PAGEOFF12, false),
                FixupKind::GotLoadPageOff12 => self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_GOT_LOAD_PAGEOFF12, false),
                FixupKind::TlvpLoadPageOff12 => self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_TLVP_LOAD_PAGEOFF12, false),
                FixupKind::Data32 => self.resolve_data32_fixup(fixup),
                FixupKind::Data64 => self.resolve_data64_fixup(fixup),
            };
            result.map_err(|e| e.with_loc_if_absent(line, col))?;
        }
        Ok(())
    }

    fn resolve_branch_or_reloc(
        &mut self,
        fixup: Fixup,
        template: Inst,
        bits: u8,
        allow_external: bool,
    ) -> Result<(), AsmError> {
        let (symbol, addend) = self.require_relocatable_symbol(&fixup.expr, "branch target")?;
        if let Some((target_section, target_offset)) = self.labels.get(&symbol) {
            self.ensure_same_section(fixup.section, *target_section, &symbol)?;
            let delta = (*target_offset as i64)
                .checked_sub(fixup.offset as i64)
                .and_then(|value| value.checked_add(addend))
                .ok_or_else(|| AsmError("branch offset overflows i64".into()))?;
            let resolved = self.resolve_branch_fixup(&template, delta, bits)?;
            self.patch_section_data(fixup.section, fixup.offset, &resolved.encode().to_le_bytes(), "branch fixup")?;
            return Ok(());
        }

        if !allow_external {
            return Err(AsmError(format!(
                "branch target '{}' requires an assembler-local label",
                symbol
            )));
        }
        if addend != 0 {
            self.record_addend_reloc(fixup.section, fixup.offset, 2, addend)?;
        }

        self.record_pending_reloc(
            fixup.section,
            fixup.offset,
            symbol,
            2,
            crate::macho::ARM64_RELOC_BRANCH26,
            true,
        );
        Ok(())
    }

    fn resolve_page_fixup(
        &mut self,
        fixup: Fixup,
        reloc_type: u32,
        pcrel: bool,
    ) -> Result<(), AsmError> {
        let context = if pcrel { "page fixup" } else { "pageoff fixup" };
        let (symbol, addend) = self.require_relocatable_symbol(&fixup.expr, context)?;
        if addend != 0 {
            self.record_addend_reloc(fixup.section, fixup.offset, 2, addend)?;
        }
        self.record_pending_reloc(fixup.section, fixup.offset, symbol, 2, reloc_type, pcrel);
        Ok(())
    }

    fn resolve_literal_fixup(&mut self, fixup: Fixup, template: Inst) -> Result<(), AsmError> {
        let (symbol, addend) = self.require_relocatable_symbol(&fixup.expr, "ldr literal target")?;
        let Some((target_section, target_offset)) = self.labels.get(&symbol) else {
            return Err(AsmError(format!(
                "ldr literal target '{}' requires an assembler-local label",
                symbol
            )));
        };
        self.ensure_same_section(fixup.section, *target_section, &symbol)?;
        let delta = (*target_offset as i64)
            .checked_sub(fixup.offset as i64)
            .and_then(|value| value.checked_add(addend))
            .ok_or_else(|| AsmError("ldr literal offset overflows i64".into()))?;
        let resolved = self.resolve_literal_inst(&template, delta)?;
        self.patch_section_data(fixup.section, fixup.offset, &resolved.encode().to_le_bytes(), "ldr literal fixup")?;
        Ok(())
    }

    fn resolve_adr_fixup(&mut self, fixup: Fixup, template: Inst) -> Result<(), AsmError> {
        let (symbol, addend) = self.require_relocatable_symbol(&fixup.expr, "adr target")?;
        let Some((target_section, target_offset)) = self.labels.get(&symbol) else {
            return Err(AsmError(format!(
                "adr target '{}' requires an assembler-local label",
                symbol
            )));
        };
        self.ensure_same_section(fixup.section, *target_section, &symbol)?;
        let delta = (*target_offset as i64)
            .checked_sub(fixup.offset as i64)
            .and_then(|value| value.checked_add(addend))
            .ok_or_else(|| AsmError("adr offset overflows i64".into()))?;
        let resolved = self.resolve_adr_inst(&template, delta)?;
        self.patch_section_data(fixup.section, fixup.offset, &resolved.encode().to_le_bytes(), "adr fixup")?;
        Ok(())
    }

    fn resolve_data64_fixup(&mut self, fixup: Fixup) -> Result<(), AsmError> {
        match self.classify_expr(&fixup.expr)? {
            ClassifiedExpr::Absolute(value) => {
                self.patch_section_data(fixup.section, fixup.offset, &(value as u64).to_le_bytes(), ".quad")?;
            }
            ClassifiedExpr::Relocatable { symbol, addend } => {
                self.patch_section_data(fixup.section, fixup.offset, &(addend as u64).to_le_bytes(), ".quad")?;
                self.record_pending_reloc(
                    fixup.section,
                    fixup.offset,
                    symbol,
                    3,
                    crate::macho::ARM64_RELOC_UNSIGNED,
                    false,
                );
            }
            ClassifiedExpr::Difference { minuend, subtrahend, addend } => {
                self.patch_section_data(fixup.section, fixup.offset, &(addend as u64).to_le_bytes(), ".quad")?;
                self.record_pending_reloc(
                    fixup.section,
                    fixup.offset,
                    subtrahend,
                    3,
                    crate::macho::ARM64_RELOC_SUBTRACTOR,
                    false,
                );
                self.record_pending_reloc(
                    fixup.section,
                    fixup.offset,
                    minuend,
                    3,
                    crate::macho::ARM64_RELOC_UNSIGNED,
                    false,
                );
            }
            ClassifiedExpr::PointerToGot { symbol, addend, pcrel } => {
                self.patch_section_data(fixup.section, fixup.offset, &(addend as u64).to_le_bytes(), ".quad")?;
                self.record_pending_reloc(
                    fixup.section,
                    fixup.offset,
                    symbol,
                    3,
                    crate::macho::ARM64_RELOC_POINTER_TO_GOT,
                    pcrel,
                );
            }
        }
        Ok(())
    }

    fn resolve_data32_fixup(&mut self, fixup: Fixup) -> Result<(), AsmError> {
        match self.classify_expr(&fixup.expr)? {
            ClassifiedExpr::Absolute(value) => {
                self.patch_section_data(fixup.section, fixup.offset, &(value as u32).to_le_bytes(), ".word")?;
                Ok(())
            }
            ClassifiedExpr::PointerToGot { symbol, addend, pcrel } => {
                self.patch_section_data(fixup.section, fixup.offset, &(addend as u32).to_le_bytes(), ".word")?;
                self.record_pending_reloc(
                    fixup.section,
                    fixup.offset,
                    symbol,
                    2,
                    crate::macho::ARM64_RELOC_POINTER_TO_GOT,
                    pcrel,
                );
                Ok(())
            }
            ClassifiedExpr::Relocatable { .. } | ClassifiedExpr::Difference { .. } => {
                Err(AsmError(
                    ".word expression must resolve to an absolute value or pointer-to-GOT relocation"
                        .into(),
                ))
            }
        }
    }

    fn require_relocatable_symbol(&self, expr: &Expr, context: &str) -> Result<(String, i64), AsmError> {
        match self.classify_expr(expr)? {
            ClassifiedExpr::Relocatable { symbol, addend } => Ok((symbol, addend)),
            ClassifiedExpr::Absolute(_) => Err(AsmError(format!(
                "{} must resolve to a relocatable symbol",
                context
            ))),
            ClassifiedExpr::Difference { .. } | ClassifiedExpr::PointerToGot { .. } => Err(AsmError(format!(
                "{} must resolve to a single relocatable symbol",
                context
            ))),
        }
    }

    fn patch_section_data(
        &mut self,
        section_index: usize,
        offset: u32,
        bytes: &[u8],
        context: &str,
    ) -> Result<(), AsmError> {
        let start = offset as usize;
        let end = start + bytes.len();
        let section = &mut self.sections[section_index];
        if end > section.data.len() {
            return Err(AsmError(format!(
                "{} exceeds section {},{} bounds",
                context,
                section.segment,
                section.name
            )));
        }
        section.data[start..end].copy_from_slice(bytes);
        Ok(())
    }

    fn record_pending_reloc(
        &mut self,
        section: usize,
        offset: u32,
        symbol: String,
        length: u8,
        reloc_type: u32,
        pcrel: bool,
    ) {
        self.pending_relocs[section].push(PendingReloc {
            section,
            offset,
            target: PendingRelocTarget::Symbol(symbol),
            length,
            reloc_type,
            pcrel,
            extern_: true,
        });
    }

    fn record_addend_reloc(
        &mut self,
        section: usize,
        offset: u32,
        length: u8,
        addend: i64,
    ) -> Result<(), AsmError> {
        self.pending_relocs[section].push(PendingReloc {
            section,
            offset,
            target: PendingRelocTarget::Raw(encode_addend_symbolnum(addend)?),
            length,
            reloc_type: crate::macho::ARM64_RELOC_ADDEND,
            pcrel: false,
            extern_: false,
        });
        Ok(())
    }

    fn record_common_symbol(&mut self, name: &str, size: u64, align_pow2: u8) -> Result<(), AsmError> {
        if align_pow2 > 15 {
            return Err(AsmError(format!(
                "common symbol '{}' alignment power {} too large (max 15)",
                name, align_pow2
            )));
        }
        if self.labels.contains_key(name) || self.absolute_defs.contains_key(name) {
            return Err(AsmError(format!("duplicate symbol '{}'", name)));
        }
        if self.common_symbols.insert(name.to_string(), CommonSymbol { size, align_pow2 }).is_some() {
            return Err(AsmError(format!("duplicate common symbol '{}'", name)));
        }
        self.note_symbol(name);
        Ok(())
    }

    fn reserve_zerofill(
        &mut self,
        seg: &str,
        sect: &str,
        symbol: Option<&str>,
        size: u64,
        align_pow2: u32,
    ) -> Result<(), AsmError> {
        if align_pow2 > 30 {
            return Err(AsmError(format!(
                "zerofill alignment power {} too large (max 30)",
                align_pow2
            )));
        }
        let target = self.ensure_section(seg, sect)?;
        if self.sections[target].kind != SectionKind::ZeroFill {
            return Err(AsmError(format!(
                ".zerofill requires a zero-fill section, got {},{}",
                seg, sect
            )));
        }

        let offset = {
            let section = &mut self.sections[target];
            section.align_pow2 = section.align_pow2.max(align_pow2);
            let offset = align_value(section.size, align_pow2);
            section.size = offset;
            offset
        };

        if let Some(symbol) = symbol {
            if self.common_symbols.contains_key(symbol) || self.absolute_defs.contains_key(symbol) {
                return Err(AsmError(format!("duplicate symbol '{}'", symbol)));
            }
            self.note_symbol(symbol);
            if self.labels.insert(symbol.to_string(), (target, offset)).is_some() {
                return Err(AsmError(format!("duplicate symbol '{}'", symbol)));
            }
        }

        self.sections[target].size = self.sections[target]
            .size
            .checked_add(size)
            .ok_or_else(|| AsmError(".zerofill size overflows u64".into()))?;
        Ok(())
    }

    fn emit_zerofill(
        &mut self,
        seg: &str,
        sect: &str,
        size: u64,
        align_pow2: u32,
    ) -> Result<(), AsmError> {
        if align_pow2 > 30 {
            return Err(AsmError(format!(
                "zerofill alignment power {} too large (max 30)",
                align_pow2
            )));
        }
        let target = self.ensure_section(seg, sect)?;
        if self.sections[target].kind != SectionKind::ZeroFill {
            return Err(AsmError(format!(
                ".zerofill requires a zero-fill section, got {},{}",
                seg, sect
            )));
        }

        let section = &mut self.sections[target];
        section.align_pow2 = section.align_pow2.max(align_pow2);
        section.size = align_value(section.size, align_pow2);
        section.size = section
            .size
            .checked_add(size)
            .ok_or_else(|| AsmError(".zerofill size overflows u64".into()))?;
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
        let index = self.sections.len() - 1;
        self.note_section_temp(index);
        Ok(index)
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

    fn ensure_same_section(&self, current_section: usize, target_section: usize, symbol: &str) -> Result<(), AsmError> {
        if target_section == current_section {
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
            Inst::Tbz { rt, bit, sf, .. } => Ok(Inst::Tbz { rt: *rt, bit: *bit, offset: checked, sf: *sf }),
            Inst::Tbnz { rt, bit, sf, .. } => Ok(Inst::Tbnz { rt: *rt, bit: *bit, offset: checked, sf: *sf }),
            _ => Err(AsmError("internal error: invalid branch fixup instruction".into())),
        }
    }

    fn resolve_literal_inst(&self, inst: &Inst, offset: i64) -> Result<Inst, AsmError> {
        let checked = check_branch_offset(offset, 19)?;
        match inst {
            Inst::LdrLit64 { rt, .. } => Ok(Inst::LdrLit64 { rt: *rt, offset: checked }),
            Inst::LdrLit32 { rt, .. } => Ok(Inst::LdrLit32 { rt: *rt, offset: checked }),
            Inst::LdrswLit { rt, .. } => Ok(Inst::LdrswLit { rt: *rt, offset: checked }),
            Inst::LdrFpLit64 { rt, .. } => Ok(Inst::LdrFpLit64 { rt: *rt, offset: checked }),
            Inst::LdrFpLit32 { rt, .. } => Ok(Inst::LdrFpLit32 { rt: *rt, offset: checked }),
            _ => Err(AsmError("internal error: invalid literal fixup instruction".into())),
        }
    }

    fn resolve_adr_inst(&self, inst: &Inst, offset: i64) -> Result<Inst, AsmError> {
        let checked = check_pcrel_offset(offset, 21, "adr offset")?;
        match inst {
            Inst::Adr { rd, .. } => Ok(Inst::Adr { rd: *rd, imm: checked }),
            _ => Err(AsmError("internal error: invalid adr fixup instruction".into())),
        }
    }

    fn section_base_addresses(&self) -> Vec<u64> {
        let mut bases = vec![0u64; self.sections.len()];
        let mut addr = 0u64;
        for index in section_allocation_order(&self.sections) {
            let section = &self.sections[index];
            addr = align_value(addr, section.align_pow2);
            bases[index] = addr;
            addr += section.size;
        }
        bases
    }

    fn symbol_values_for_expr(&self) -> BTreeMap<String, SymbolValue> {
        let mut values = BTreeMap::new();

        for (name, value) in &self.absolute_symbols {
            values.insert(name.clone(), SymbolValue::Absolute(*value));
        }

        for (name, (section, offset)) in &self.labels {
            values.insert(
                name.clone(),
                SymbolValue::Defined {
                    section: *section,
                    value: (self.section_bases[*section] + offset) as i64,
                },
            );
        }

        values
    }

    fn classify_expr(&self, expr: &Expr) -> Result<ClassifiedExpr, AsmError> {
        expr::classify(expr, &self.symbol_values_for_expr())
            .map_err(|err| AsmError(err.to_string()))
    }

    fn require_absolute_expr(&self, expr: &Expr, context: &str) -> Result<i64, AsmError> {
        match self.classify_expr(expr)? {
            ClassifiedExpr::Absolute(value) => Ok(value),
            ClassifiedExpr::Relocatable { .. }
            | ClassifiedExpr::Difference { .. }
            | ClassifiedExpr::PointerToGot { .. } => Err(AsmError(format!(
                "{} must resolve to an absolute value",
                context
            ))),
        }
    }

    fn metadata_flags(&self) -> u32 {
        if self.subsections_via_symbols {
            macho::MH_SUBSECTIONS_VIA_SYMBOLS
        } else {
            0
        }
    }

    fn build_version_command(&self) -> Result<BuildVersion, AsmError> {
        let Some(build_version) = &self.build_version else {
            return Ok(BuildVersion::default());
        };

        let platform = match build_version.platform.as_str() {
            "macos" => macho::PLATFORM_MACOS,
            other => {
                return Err(AsmError(format!(
                    "unsupported .build_version platform '{}' (supported: macos)",
                    other
                )));
            }
        };

        Ok(BuildVersion {
            platform,
            minos: macho::pack_version(
                build_version.minos.major,
                build_version.minos.minor,
                build_version.minos.patch,
            ),
            sdk: build_version
                .sdk
                .map(|sdk| macho::pack_version(sdk.major, sdk.minor, sdk.patch))
                .unwrap_or(0),
        })
    }

    fn materialize_eh_frame_section(&mut self) -> Result<(), AsmError> {
        if self.eh_frame_rows.is_empty() {
            return Ok(());
        }

        let eh_frame_index = self.sections.len();
        let eh_frame_symbol = format!("ltmp{}", eh_frame_index);
        self.note_section_temp(eh_frame_index);
        let mut section = Section::new("__TEXT", "__eh_frame", SectionKind::EhFrame);
        section.align_pow2 = 3;
        section.data.extend_from_slice(&[
            0x10, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
            0x01, 0x7a, 0x52, 0x00,
            0x01, 0x78, 0x1e, 0x01,
            0x10, 0x0c, 0x1f, 0x00,
        ]);

        self.pending_relocs.push(Vec::new());

        let rows = self.eh_frame_rows.clone();
        for row in &rows {
            let fde_start = section.data.len() as u32;
            let cie_pointer = fde_start + 4;
            let pc_field_offset = fde_start + 8;

            let mut fde = Vec::new();
            fde.extend_from_slice(&cie_pointer.to_le_bytes());
            fde.extend_from_slice(&(-(pc_field_offset as i64)).to_le_bytes());
            fde.extend_from_slice(&row.length.to_le_bytes());
            fde.push(0);
            fde.extend_from_slice(&row.instructions);

            let fde_length = u32::try_from(fde.len())
                .map_err(|_| AsmError("eh_frame FDE exceeds u32".into()))?;
            section.data.extend_from_slice(&fde_length.to_le_bytes());
            section.data.extend_from_slice(&fde);

            self.record_pending_reloc(
                eh_frame_index,
                pc_field_offset,
                eh_frame_symbol.clone(),
                3,
                macho::ARM64_RELOC_SUBTRACTOR,
                false,
            );
            self.record_pending_reloc(
                eh_frame_index,
                pc_field_offset,
                row.function_symbol.clone(),
                3,
                macho::ARM64_RELOC_UNSIGNED,
                false,
            );
        }

        section.size = section.data.len() as u64;
        self.sections.push(section);
        Ok(())
    }

    fn materialize_compact_unwind_section(&mut self) {
        if self.compact_unwind_rows.is_empty() {
            return;
        }

        let mut section = Section::new("__LD", "__compact_unwind", SectionKind::CompactUnwind);
        self.note_section_temp(self.sections.len());
        section.align_pow2 = 3;
        for row in &self.compact_unwind_rows {
            let reloc_offset = section.data.len() as u32;
            section.data.extend_from_slice(&row.start_offset.to_le_bytes());
            section.data.extend_from_slice(&row.length.to_le_bytes());
            section.data.extend_from_slice(&row.encoding.to_le_bytes());
            section.data.extend_from_slice(&0u64.to_le_bytes());
            section.data.extend_from_slice(&0u64.to_le_bytes());
            section.relocations.push(Relocation {
                offset: reloc_offset,
                symbol_idx: (row.start_section + 1) as u32,
                pcrel: false,
                length: 3,
                extern_: false,
                reloc_type: macho::ARM64_RELOC_UNSIGNED,
            });
        }
        section.size = section.data.len() as u64;
        self.sections.push(section);
        self.pending_relocs.push(Vec::new());
    }

    fn finish(mut self) -> Result<ObjectFile, AsmError> {
        if self.active_cfi_proc.is_some() {
            return Err(AsmError("unterminated .cfi_startproc before end of file".into()));
        }

        self.materialize_compact_unwind_section();
        self.materialize_eh_frame_section()?;

        let section_bases = self.section_base_addresses();
        let absolute_symbols = self.absolute_symbols.clone();
        let mut symbols: Vec<Symbol> = Vec::new();
        let flags = self.metadata_flags();
        let build_version = self.build_version_command()?;

        for name in absolute_symbols.keys() {
            if self.labels.contains_key(name) {
                return Err(AsmError(format!(
                    "symbol '{}' cannot be both a label and an absolute assignment",
                    name
                )));
            }
        }

        for name in self.common_symbols.keys() {
            if self.labels.contains_key(name) || absolute_symbols.contains_key(name) {
                return Err(AsmError(format!(
                    "symbol '{}' cannot be both common and defined in this object",
                    name
                )));
            }
        }

        for (index, base) in section_bases.iter().enumerate() {
            symbols.push(Symbol {
                name: format!("ltmp{}", index),
                section: (index + 1) as u8,
                value: *base,
                global: false,
                undefined: false,
                absolute: false,
                common: false,
                common_align_pow2: 0,
                private_extern: false,
                weak_ref: false,
                weak_def: false,
            });
        }

        for (name, (section, offset)) in &self.labels {
            if !self.symbol_attrs.contains_key(name)
                && !name.starts_with("ltmp")
                && !is_hidden_local_object_symbol(name)
            {
                let value = section_bases[*section] + offset;
                symbols.push(Symbol {
                    name: name.clone(),
                    section: (*section + 1) as u8,
                    value,
                    global: false,
                    undefined: false,
                    absolute: false,
                    common: false,
                    common_align_pow2: 0,
                    private_extern: false,
                    weak_ref: false,
                    weak_def: false,
                });
            }
        }

        for (name, value) in &absolute_symbols {
            if !self.symbol_attrs.contains_key(name) {
                symbols.push(Symbol {
                    name: name.clone(),
                    section: 0,
                    value: *value as u64,
                    global: false,
                    undefined: false,
                    absolute: true,
                    common: false,
                    common_align_pow2: 0,
                    private_extern: false,
                    weak_ref: false,
                    weak_def: false,
                });
            }
        }

        for (name, common) in &self.common_symbols {
            let attrs = self.symbol_attrs.get(name).copied().unwrap_or_default();
            if attrs.private_extern {
                return Err(AsmError(format!(
                    "common symbol '{}' cannot be private extern",
                    name
                )));
            }
            if attrs.weak_def {
                return Err(AsmError(format!(
                    "common symbol '{}' cannot be a weak definition",
                    name
                )));
            }
            symbols.push(Symbol {
                name: name.clone(),
                section: 0,
                value: common.size,
                global: true,
                undefined: true,
                absolute: false,
                common: true,
                common_align_pow2: common.align_pow2,
                private_extern: false,
                weak_ref: attrs.weak_ref,
                weak_def: false,
            });
        }

        // Explicit symbol directives.
        for (name, attrs) in &self.symbol_attrs {
            if let Some(value) = absolute_symbols.get(name) {
                if attrs.weak_ref {
                    return Err(AsmError(format!(
                        "weak reference '{}' must remain undefined",
                        name
                    )));
                }
                symbols.push(Symbol {
                    name: name.clone(),
                    section: 0,
                    value: *value as u64,
                    global: attrs.global,
                    undefined: false,
                    absolute: true,
                    common: false,
                    common_align_pow2: 0,
                    private_extern: attrs.private_extern,
                    weak_ref: false,
                    weak_def: attrs.weak_def,
                });
            } else if let Some((section, offset)) = self.labels.get(name) {
                if attrs.weak_ref {
                    return Err(AsmError(format!(
                        "weak reference '{}' must remain undefined",
                        name
                    )));
                }
                let value = section_bases[*section] + offset;
                symbols.push(Symbol {
                    name: name.clone(),
                    section: (*section + 1) as u8,
                    value,
                    global: attrs.global,
                    undefined: false,
                    absolute: false,
                    common: false,
                    common_align_pow2: 0,
                    private_extern: attrs.private_extern,
                    weak_ref: false,
                    weak_def: attrs.weak_def,
                });
            } else if !symbols.iter().any(|s| s.name == *name) {
                if attrs.private_extern {
                    return Err(AsmError(format!(
                        "private extern '{}' must be defined in this object",
                        name
                    )));
                }
                if attrs.weak_def {
                    return Err(AsmError(format!(
                        "weak definition '{}' must be defined in this object",
                        name
                    )));
                }
                symbols.push(Symbol {
                    name: name.clone(),
                    section: 0,
                    value: 0,
                    global: true,
                    undefined: true,
                    absolute: false,
                    common: false,
                    common_align_pow2: 0,
                    private_extern: false,
                    weak_ref: attrs.weak_ref,
                    weak_def: false,
                });
            }
        }

        let mut missing_reloc_symbols = Vec::new();
        for relocs in &self.pending_relocs {
            for reloc in relocs {
                let PendingRelocTarget::Symbol(symbol) = &reloc.target else {
                    continue;
                };
                if !self.labels.contains_key(symbol)
                    && !symbols.iter().any(|s| s.name == *symbol)
                    && !missing_reloc_symbols.iter().any(|name| name == symbol)
                {
                    if is_assembler_local_symbol(symbol) {
                        return Err(AsmError(format!(
                            "local symbol '{}' must be defined in this object",
                            symbol
                        )));
                    }
                    missing_reloc_symbols.push(symbol.clone());
                }
            }
        }

        for name in missing_reloc_symbols {
            self.note_symbol(&name);
            symbols.push(Symbol {
                name,
                section: 0,
                value: 0,
                global: true,
                undefined: true,
                absolute: false,
                common: false,
                common_align_pow2: 0,
                private_extern: false,
                weak_ref: false,
                weak_def: false,
            });
        }

        let page_reloc_targets: BTreeSet<_> = self.pending_relocs
            .iter()
            .flat_map(|relocs| relocs.iter())
            .filter(|reloc| {
                reloc.reloc_type == macho::ARM64_RELOC_PAGE21
                    || reloc.reloc_type == macho::ARM64_RELOC_PAGEOFF12
            })
            .filter_map(|reloc| match &reloc.target {
                PendingRelocTarget::Symbol(symbol) => Some(symbol.clone()),
                PendingRelocTarget::Raw(_) => None,
            })
            .collect();
        let symbol_order = self.symbol_order.clone();
        symbols.sort_by(|a, b| {
            let rank_a = symbol_class_rank(a);
            let rank_b = symbol_class_rank(b);
            rank_a
                .cmp(&rank_b)
                .then_with(|| match rank_a {
                    0 => {
                        if a.section == b.section && a.value == b.value {
                            let a_is_section_temp = a.name.starts_with("ltmp");
                            let b_is_section_temp = b.name.starts_with("ltmp");
                            if a_is_section_temp != b_is_section_temp {
                                let a_prefers_front =
                                    !a_is_section_temp && page_reloc_targets.contains(&a.name);
                                let b_prefers_front =
                                    !b_is_section_temp && page_reloc_targets.contains(&b.name);
                                if a_prefers_front != b_prefers_front {
                                    return b_prefers_front.cmp(&a_prefers_front);
                                }
                            }
                        }
                        symbol_order
                            .get(&a.name)
                            .copied()
                            .unwrap_or(usize::MAX)
                            .cmp(&symbol_order.get(&b.name).copied().unwrap_or(usize::MAX))
                            .then_with(|| a.name.cmp(&b.name))
                    }
                    _ => a.name.cmp(&b.name),
                })
        });

        let linker_optimization_hints = self.build_linker_optimization_hint_data()?;

        for relocs in self.pending_relocs {
            for pending in relocs {
                let symbol_idx = match pending.target {
                    PendingRelocTarget::Symbol(symbol) => symbols
                        .iter()
                        .position(|s| s.name == symbol)
                        .ok_or_else(|| AsmError(format!("missing relocation symbol '{}'", symbol)))?
                        as u32,
                    PendingRelocTarget::Raw(value) => value,
                };
                self.sections[pending.section].relocations.push(Relocation {
                    offset: pending.offset,
                    symbol_idx,
                    pcrel: pending.pcrel,
                    length: pending.length,
                    extern_: pending.extern_,
                    reloc_type: pending.reloc_type,
                });
            }
        }

        Ok(ObjectFile {
            sections: self.sections,
            symbols,
            flags,
            build_version,
            linker_optimization_hints,
        })
    }

    fn build_linker_optimization_hint_data(&self) -> Result<Vec<u8>, AsmError> {
        let mut data = Vec::new();
        for hint in &self.linker_optimization_hints {
            let Some(kind) = linker_optimization_hint_kind(&hint.kind) else {
                continue;
            };
            if hint.labels.len() > u8::MAX as usize {
                return Err(AsmError(format!(
                    ".loh {} has too many labels ({})",
                    hint.kind,
                    hint.labels.len()
                )));
            }
            data.push(kind);
            data.push(hint.labels.len() as u8);
            for label in &hint.labels {
                let (section, offset) = self.labels.get(label).copied().ok_or_else(|| {
                    AsmError(format!(".loh {} references undefined label '{}'", hint.kind, label))
                })?;
                if self.sections[section].kind != SectionKind::Text {
                    return Err(AsmError(format!(
                        ".loh {} requires __TEXT,__text labels, got '{}' in {},{}",
                        hint.kind,
                        label,
                        self.sections[section].segment,
                        self.sections[section].name
                    )));
                }
                append_uleb128(&mut data, offset);
            }
        }
        while !data.is_empty() && !data.len().is_multiple_of(8) {
            data.push(0);
        }
        Ok(data)
    }

    fn resolve_absolute_symbols(&self) -> Result<BTreeMap<String, i64>, AsmError> {
        let mut resolved = BTreeMap::new();
        let mut visiting = Vec::new();
        let names: Vec<_> = self.absolute_defs.keys().cloned().collect();
        for name in names {
            let value = self.resolve_absolute_symbol(&name, &mut resolved, &mut visiting)?;
            resolved.insert(name, value);
        }
        Ok(resolved)
    }

    fn resolve_absolute_symbol(
        &self,
        name: &str,
        resolved: &mut BTreeMap<String, i64>,
        visiting: &mut Vec<String>,
    ) -> Result<i64, AsmError> {
        if let Some(value) = resolved.get(name) {
            return Ok(*value);
        }
        if visiting.iter().any(|entry| entry == name) {
            return Err(AsmError(format!(
                "absolute symbol '{}' has a cyclic definition",
                name
            )));
        }

        let expr = self
            .absolute_defs
            .get(name)
            .ok_or_else(|| AsmError(format!("missing absolute symbol definition '{}'", name)))?;

        visiting.push(name.to_string());
        let mut symbols = BTreeMap::new();
        for referenced in expr::referenced_symbols(expr) {
            if let Some(value) = resolved.get(&referenced) {
                symbols.insert(referenced, SymbolValue::Absolute(*value));
            } else if self.absolute_defs.contains_key(&referenced) {
                let value = self.resolve_absolute_symbol(&referenced, resolved, visiting)?;
                symbols.insert(referenced, SymbolValue::Absolute(value));
            } else if let Some((section, offset)) = self.labels.get(&referenced) {
                symbols.insert(
                    referenced,
                    SymbolValue::Defined {
                        section: *section,
                        value: (self.section_bases[*section] + offset) as i64,
                    },
                );
            } else {
                visiting.pop();
                return Err(AsmError(format!(
                    "absolute symbol '{}' references undefined symbol '{}'",
                    name,
                    referenced
                )));
            }
        }

        let value = match expr::classify(expr, &symbols) {
            Ok(ClassifiedExpr::Absolute(value)) => value,
            Ok(_) => {
                visiting.pop();
                return Err(AsmError(format!(
                    "absolute symbol '{}' must resolve to an absolute value",
                    name
                )));
            }
            Err(err) => {
                visiting.pop();
                return Err(AsmError(format!(
                    "absolute symbol '{}': {}",
                    name,
                    err
                )));
            }
        };

        visiting.pop();
        resolved.insert(name.to_string(), value);
        Ok(value)
    }
}

fn emit_advance_loc(buf: &mut Vec<u8>, delta: u64) -> Result<(), AsmError> {
    match delta {
        0 => {}
        1..=0x3f => buf.push(0x40 | (delta as u8)),
        0x40..=0xff => {
            buf.push(0x02);
            buf.push(delta as u8);
        }
        0x100..=0xffff => {
            buf.push(0x03);
            buf.extend_from_slice(&(delta as u16).to_le_bytes());
        }
        _ => {
            let delta = u32::try_from(delta)
                .map_err(|_| AsmError(format!("CFI advance {} exceeds u32", delta)))?;
            buf.push(0x04);
            buf.extend_from_slice(&delta.to_le_bytes());
        }
    }
    Ok(())
}

fn emit_cfi_op(buf: &mut Vec<u8>, op: CfiOp) -> Result<(), AsmError> {
    match op {
        CfiOp::DefCfa { register, offset } => {
            buf.push(0x0c);
            encode_uleb128(buf, register.num() as u64);
            encode_uleb128(buf, offset);
        }
        CfiOp::DefCfaRegister(register) => {
            buf.push(0x0d);
            encode_uleb128(buf, register.num() as u64);
        }
        CfiOp::DefCfaOffset(offset) => {
            buf.push(0x0e);
            encode_uleb128(buf, offset);
        }
        CfiOp::Offset { register, offset } => {
            let scaled = offset / 8;
            if scaled == 0 || scaled * 8 != offset {
                return Err(AsmError(format!(
                    "CFI offset {} for x{} is not representable with DWARF data alignment",
                    offset,
                    register.num()
                )));
            }
            buf.push(0x80 | register.num());
            encode_uleb128(buf, scaled);
        }
        CfiOp::Restore(register) => {
            buf.push(0xc0 | register.num());
        }
    }
    Ok(())
}

fn encode_uleb128(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn align_value(value: u64, power: u32) -> u64 {
    let alignment = 1u64 << power;
    (value + alignment - 1) & !(alignment - 1)
}

fn section_allocation_order(sections: &[Section]) -> Vec<usize> {
    let mut order: Vec<_> = (0..sections.len()).collect();
    order.sort_by_key(|&index| sections[index].kind == SectionKind::ZeroFill);
    order
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

fn check_pcrel_offset(offset: i64, bits: u8, context: &str) -> Result<i32, AsmError> {
    let min = -(1i64 << (bits - 1));
    let max = (1i64 << (bits - 1)) - 1;
    if offset < min || offset > max {
        return Err(AsmError(format!(
            "{} {} is out of range for {}-bit immediate",
            context, offset, bits
        )));
    }

    Ok(offset as i32)
}

fn symbol_class_rank(symbol: &Symbol) -> u8 {
    if symbol.undefined {
        2
    } else if symbol.global {
        1
    } else {
        0
    }
}

fn linker_optimization_hint_kind(name: &str) -> Option<u8> {
    match name {
        "AdrpAdd" => Some(7),
        "AdrpLdrGot" => Some(8),
        _ => None,
    }
}

fn append_uleb128(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn is_assembler_local_symbol(name: &str) -> bool {
    name.starts_with(".L") || name.starts_with('L')
}

fn is_hidden_local_object_symbol(name: &str) -> bool {
    name.starts_with(".Ltmp$") || name.starts_with('L')
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

    fn data_relocs(obj: &ObjectFile) -> &[Relocation] {
        &obj.section("__DATA", "__data").expect("missing __DATA,__data").relocations
    }

    fn compact_unwind_section(obj: &ObjectFile) -> &Section {
        obj.section("__LD", "__compact_unwind")
            .expect("missing __LD,__compact_unwind")
    }

    fn eh_frame_section(obj: &ObjectFile) -> &Section {
        obj.section("__TEXT", "__eh_frame")
            .expect("missing __TEXT,__eh_frame")
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
        assert!(!main.absolute);
        assert!(!main.private_extern);
        assert!(!main.weak_ref);
        assert!(!main.weak_def);
        assert_eq!(main.section, 1);
        assert_eq!(main.value, 0);
    }

    #[test]
    fn assemble_symbol_order_groups_classes_with_local_source_order() {
        let obj = assemble_source(
            ".text\n\
            .globl _aaa\n\
            .weak_definition zlocal\n\
            .weak_reference _puts\n\
            _aaa:\n\
            ret\n\
            zlocal:\n\
            ret\n\
            bl _puts\n"
        )
        .unwrap();

        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        assert_eq!(names, vec!["ltmp0", "zlocal", "_aaa", "_puts"]);
    }

    #[test]
    fn assemble_absolute_symbol() {
        let obj = assemble_source(".set ABS1, 7\n.text\nret\n").unwrap();
        let abs1 = obj.symbols.iter().find(|s| s.name == "ABS1").unwrap();
        assert!(abs1.absolute);
        assert!(!abs1.global);
        assert!(!abs1.undefined);
        assert_eq!(abs1.section, 0);
        assert_eq!(abs1.value, 7);
    }

    #[test]
    fn assemble_global_absolute_symbol() {
        let obj = assemble_source(".set ABS1, 7\n.globl ABS1\n.text\nret\n").unwrap();
        let abs1 = obj.symbols.iter().find(|s| s.name == "ABS1").unwrap();
        assert!(abs1.absolute);
        assert!(abs1.global);
        assert_eq!(abs1.value, 7);
    }

    #[test]
    fn assemble_absolute_symbol_chain() {
        let obj = assemble_source(".set ABS1, 7\n.equ ABS2, ABS1 + 5\n.text\nret\n").unwrap();
        let abs2 = obj.symbols.iter().find(|s| s.name == "ABS2").unwrap();
        assert!(abs2.absolute);
        assert_eq!(abs2.value, 12);
    }

    #[test]
    fn assemble_absolute_symbol_from_label_difference() {
        let obj = assemble_source(".text\nfoo:\nnop\nbar:\nret\n.set DIFF, bar - foo\n").unwrap();
        let diff = obj.symbols.iter().find(|s| s.name == "DIFF").unwrap();
        assert!(diff.absolute);
        assert_eq!(diff.value, 4);
    }

    #[test]
    fn assemble_private_extern_symbol() {
        let obj = assemble_source(".private_extern _helper\n.text\n_helper:\nret\n").unwrap();
        let helper = obj.symbols.iter().find(|s| s.name == "_helper").unwrap();
        assert!(helper.global);
        assert!(helper.private_extern);
        assert!(!helper.undefined);
    }

    #[test]
    fn assemble_weak_reference_symbol() {
        let obj = assemble_source(".weak_reference _puts\n.text\nbl _puts\nret\n").unwrap();
        let puts = obj.symbols.iter().find(|s| s.name == "_puts").unwrap();
        assert!(puts.global);
        assert!(puts.undefined);
        assert!(puts.weak_ref);
    }

    #[test]
    fn assemble_weak_definition_symbol() {
        let obj = assemble_source(".weak_definition _entry\n.text\n_entry:\nret\n").unwrap();
        let entry = obj.symbols.iter().find(|s| s.name == "_entry").unwrap();
        assert!(!entry.global);
        assert!(!entry.undefined);
        assert!(entry.weak_def);
    }

    #[test]
    fn assemble_private_extern_requires_definition() {
        let err = assemble_source(".private_extern _hidden\n.text\nret\n").unwrap_err();
        assert!(err.msg.contains("private extern"), "got: {}", err);
    }

    #[test]
    fn assemble_weak_definition_requires_definition() {
        let err = assemble_source(".weak_definition _entry\n.text\nret\n").unwrap_err();
        assert!(err.msg.contains("weak definition"), "got: {}", err);
    }

    #[test]
    fn assemble_weak_reference_requires_undefined_symbol() {
        let err = assemble_source(".weak_reference _helper\n.text\n_helper:\nret\n").unwrap_err();
        assert!(err.msg.contains("must remain undefined"), "got: {}", err);
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
    fn assemble_text_alignment_uses_zero_then_nop_padding() {
        let obj = assemble_source(".text\n.byte 1\n.p2align 3\n.byte 2\n").unwrap();
        assert_eq!(
            text_bytes(&obj),
            vec![0x01, 0x00, 0x00, 0x00, 0x1f, 0x20, 0x03, 0xd5, 0x02]
        );
    }

    #[test]
    fn assemble_alignment_with_fill_byte_repeats_fill() {
        let obj = assemble_source(".text\n.byte 1\n.p2align 3, 0xAA\n.byte 2\n").unwrap();
        assert_eq!(
            text_bytes(&obj),
            vec![0x01, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x02]
        );
    }

    #[test]
    fn assemble_alignment_max_skip_can_suppress_padding() {
        let obj = assemble_source(".text\n.byte 1\n.p2align 4, 0xAA, 2\n.byte 2\n").unwrap();
        assert_eq!(text_bytes(&obj), vec![0x01, 0x02]);
        assert_eq!(obj.text_section().align_pow2, 4);
    }

    #[test]
    fn assemble_data_alignment_defaults_to_zero_fill() {
        let obj = assemble_source(".data\n.byte 1\n.align 3\n.byte 2\n").unwrap();
        assert_eq!(data_bytes(&obj), vec![0x01, 0, 0, 0, 0, 0, 0, 0, 0x02]);
    }

    #[test]
    fn assemble_data_directive_byte() {
        let obj = assemble_source(".data\n.byte 0x41, 0x42, 0x43\n").unwrap();
        assert_eq!(data_bytes(&obj), vec![0x41, 0x42, 0x43]);
    }

    #[test]
    fn assemble_data_directive_short() {
        let obj = assemble_source(".data\n.short 0x1234, 2\n").unwrap();
        assert_eq!(data_bytes(&obj), vec![0x34, 0x12, 0x02, 0x00]);
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
    fn assemble_zero_aliases_space() {
        let obj = assemble_source(".data\n.zero 3\n").unwrap();
        assert_eq!(data_bytes(&obj), vec![0, 0, 0]);
    }

    #[test]
    fn assemble_fill_repeats_truncated_little_endian_pattern() {
        let obj = assemble_source(".data\n.fill 2, 2, 0x3344\n").unwrap();
        assert_eq!(data_bytes(&obj), vec![0x44, 0x33, 0x44, 0x33]);
    }

    #[test]
    fn assemble_cstring_switches_to_cstring_section() {
        let obj = assemble_source(".cstring\nmsg: .asciz \"hello\"\n").unwrap();
        let cstring = obj.section("__TEXT", "__cstring").unwrap();
        assert_eq!(cstring.data, b"hello\0");
        assert_eq!(obj.symbols.iter().find(|sym| sym.name == "msg").unwrap().value, 0);
    }

    #[test]
    fn assemble_extern_declaration_does_not_emit_symbol_by_itself() {
        let obj = assemble_source(".extern _puts\n.text\nret\n").unwrap();
        assert!(!obj.symbols.iter().any(|sym| sym.name == "_puts"));
    }

    #[test]
    fn assemble_comm_emits_common_symbol() {
        let obj = assemble_source(".comm _common, 24, 3\n.text\nret\n").unwrap();
        let common = obj.symbols.iter().find(|sym| sym.name == "_common").unwrap();
        assert!(common.global);
        assert!(common.undefined);
        assert!(common.common);
        assert_eq!(common.common_align_pow2, 3);
        assert_eq!(common.value, 24);
        assert_eq!(text_bytes(&obj), Inst::Ret { rn: X30 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_zerofill_reserves_bss_without_switching_sections() {
        let obj = assemble_source(".text\nret\n.zerofill __DATA,__bss,_scratch,16,4\nret\n").unwrap();
        let bss = obj.section("__DATA", "__bss").unwrap();
        let scratch = obj.symbols.iter().find(|sym| sym.name == "_scratch").unwrap();

        assert_eq!(text_bytes(&obj), [
            Inst::Ret { rn: X30 }.encode().to_le_bytes(),
            Inst::Ret { rn: X30 }.encode().to_le_bytes(),
        ]
        .concat());
        assert!(bss.data.is_empty());
        assert_eq!(bss.size, 16);
        assert_eq!(bss.align_pow2, 4);
        assert_eq!(scratch.value, 16);
    }

    #[test]
    fn assemble_zerofill_without_symbol_is_allowed() {
        let obj = assemble_source(".text\nret\n.zerofill __DATA,__bss,,8,2\n").unwrap();
        let bss = obj.section("__DATA", "__bss").unwrap();
        assert_eq!(bss.size, 8);
        assert_eq!(bss.align_pow2, 2);
        assert!(!obj.symbols.iter().any(|sym| sym.name.is_empty()));
    }

    #[test]
    fn assemble_zerofill_keeps_declared_section_order_and_trailing_address() {
        let obj = assemble_source(
            ".text\nret\n.zerofill __DATA,__bss,_scratch,16,4\n.data\n.byte 1\n"
        )
        .unwrap();

        let section_names: Vec<_> = obj.sections.iter()
            .map(|section| (section.segment.as_str(), section.name.as_str()))
            .collect();
        assert_eq!(
            section_names,
            vec![("__TEXT", "__text"), ("__DATA", "__bss"), ("__DATA", "__data")]
        );

        let scratch = obj.symbols.iter().find(|sym| sym.name == "_scratch").unwrap();
        assert_eq!(scratch.section, 2);
        assert_eq!(scratch.value, 16);
    }

    #[test]
    fn assemble_section_switching() {
        let src = ".text\nnop\n.data\n.byte 1\n.text\nret\n.data\n.byte 2\n";
        let obj = assemble_source(src).unwrap();
        assert_eq!(text_bytes(&obj).len(), 8); // nop + ret
        assert_eq!(data_bytes(&obj), vec![1, 2]);
    }

    #[test]
    fn assemble_unknown_directive_is_rejected() {
        let err = assemble_source(".data\n.byte 1\n.unknown_directive\n.byte 2\n").unwrap_err();
        assert_eq!(err.line, Some(3));
        assert_eq!(err.col, Some(1));
        assert!(err.msg.contains("unsupported directive '.unknown_directive'"), "got: {}", err);
    }

    #[test]
    fn assemble_frameless_cfi_emits_compact_unwind() {
        let obj = assemble_source(
            ".text\n\
            frameless_target:\n\
            .cfi_startproc\n\
            sub sp, sp, #16\n\
            .cfi_def_cfa_offset 16\n\
            add sp, sp, #16\n\
            ret\n\
            .cfi_endproc\n"
        )
        .unwrap();
        let compact = compact_unwind_section(&obj);
        assert_eq!(compact.align_pow2, 3);
        assert_eq!(compact.data.len(), 32);
        assert_eq!(
            compact.data,
            [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x0C, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x02,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ]
        );
        assert_eq!(
            compact.relocations,
            vec![Relocation {
                offset: 0,
                symbol_idx: 1,
                pcrel: false,
                length: 3,
                extern_: false,
                reloc_type: macho::ARM64_RELOC_UNSIGNED,
            }]
        );
    }

    #[test]
    fn assemble_frame_cfi_emits_compact_unwind() {
        let obj = assemble_source(
            ".text\n\
            frame_target:\n\
            .cfi_startproc\n\
            stp x22, x21, [sp, #-48]!\n\
            stp x20, x19, [sp, #16]\n\
            stp x29, x30, [sp, #32]\n\
            add x29, sp, #32\n\
            .cfi_def_cfa w29, 16\n\
            .cfi_offset w30, -8\n\
            .cfi_offset w29, -16\n\
            .cfi_offset w19, -24\n\
            .cfi_offset w20, -32\n\
            .cfi_offset w21, -40\n\
            .cfi_offset w22, -48\n\
            ldp x29, x30, [sp, #32]\n\
            ldp x20, x19, [sp, #16]\n\
            ldp x22, x21, [sp], #48\n\
            ret\n\
            .cfi_endproc\n"
        )
        .unwrap();
        let compact = compact_unwind_section(&obj);
        assert_eq!(
            compact.data,
            [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x20, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x04,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ]
        );
    }

    #[test]
    fn assemble_cfi_endproc_requires_active_proc() {
        let err = assemble_source(".cfi_endproc\n").unwrap_err();
        assert!(err.msg.contains(".cfi_endproc requires an active .cfi_startproc"), "got: {}", err);
    }

    #[test]
    fn assemble_cfi_directive_requires_active_proc() {
        let err = assemble_source(".cfi_def_cfa_offset 16\n").unwrap_err();
        assert!(err.msg.contains("CFI directives require an active .cfi_startproc"), "got: {}", err);
    }

    #[test]
    fn assemble_unterminated_cfi_proc_is_rejected() {
        let err = assemble_source(".text\nunterminated_target:\n.cfi_startproc\nret\n").unwrap_err();
        assert!(err.msg.contains("unterminated .cfi_startproc"), "got: {}", err);
    }

    #[test]
    fn assemble_cfi_restore_falls_back_to_eh_frame() {
        let obj = assemble_source(
            ".text\n\
            restore_target:\n\
            .cfi_startproc\n\
            ret\n\
            .cfi_restore w29\n\
            .cfi_endproc\n"
        )
        .unwrap();

        let compact = compact_unwind_section(&obj);
        let eh_frame = eh_frame_section(&obj);
        assert_eq!(u32::from_le_bytes(compact.data[12..16].try_into().unwrap()), UNWIND_ARM64_MODE_DWARF);
        assert!(!eh_frame.data.is_empty());
    }

    #[test]
    fn assemble_unpaired_saved_register_falls_back_to_eh_frame() {
        let obj = assemble_source(
            ".text\n\
            pair_target:\n\
            .cfi_startproc\n\
            sub sp, sp, #32\n\
            stp x29, x30, [sp, #16]\n\
            add x29, sp, #16\n\
            .cfi_def_cfa w29, 16\n\
            .cfi_offset w30, -8\n\
            .cfi_offset w29, -16\n\
            .cfi_offset w19, -24\n\
            ret\n\
            .cfi_endproc\n"
        )
        .unwrap();

        let compact = compact_unwind_section(&obj);
        let eh_frame = eh_frame_section(&obj);
        assert_eq!(u32::from_le_bytes(compact.data[12..16].try_into().unwrap()), UNWIND_ARM64_MODE_DWARF);
        assert!(eh_frame.relocations.len() >= 2);
    }

    #[test]
    fn assemble_subsections_via_symbols_sets_object_flag() {
        let obj = assemble_source(".text\nret\n.subsections_via_symbols\n").unwrap();
        assert_eq!(obj.flags, macho::MH_SUBSECTIONS_VIA_SYMBOLS);
    }

    #[test]
    fn assemble_build_version_sets_object_metadata() {
        let obj = assemble_source(
            ".text\nret\n.build_version macos, 11, 0 sdk_version 15, 5\n"
        )
        .unwrap();

        assert_eq!(obj.build_version.platform, macho::PLATFORM_MACOS);
        assert_eq!(obj.build_version.minos, macho::pack_version(11, 0, 0));
        assert_eq!(obj.build_version.sdk, macho::pack_version(15, 5, 0));
    }

    #[test]
    fn assemble_rejects_unsupported_build_version_platform() {
        let err = assemble_source(".build_version ios, 11, 0\n").unwrap_err();
        assert!(
            err.msg.contains("unsupported .build_version platform"),
            "got: {}",
            err
        );
    }

    #[test]
    fn assemble_rejects_unsupported_section() {
        let err = assemble_source(".section __TEXT,__foo\n.space 16\n").unwrap_err();
        assert!(err.msg.contains("unsupported section"), "got: {}", err);
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
        assert!(err.msg.contains("zero-fill"), "got: {}", err);
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
    fn assemble_got_load_reloc_uses_got_types() {
        let obj = assemble_source(
            ".global _caller\n.text\n_caller:\nadrp x0, _puts@GOTPAGE\nldr x0, [x0, _puts@GOTPAGEOFF]\nret\n"
        ).unwrap();

        let puts = obj.symbols.iter().find(|sym| sym.name == "_puts").unwrap();
        assert!(puts.undefined);
        assert!(puts.global);

        let relocs = text_relocs(&obj);
        let reloc_types: Vec<_> = relocs.iter().map(|rel| rel.reloc_type).collect();
        assert_eq!(
            reloc_types,
            vec![
                crate::macho::ARM64_RELOC_GOT_LOAD_PAGE21,
                crate::macho::ARM64_RELOC_GOT_LOAD_PAGEOFF12,
            ]
        );
        let reloc_syms: Vec<_> = relocs
            .iter()
            .map(|rel| obj.symbols[rel.symbol_idx as usize].name.as_str())
            .collect();
        assert_eq!(reloc_syms, vec!["_puts", "_puts"]);
    }

    #[test]
    fn assemble_tlvp_load_reloc_uses_tlvp_types() {
        let obj = assemble_source(
            ".global _load_tls\n.text\n_load_tls:\nadrp x0, _tls_counter@TLVPPAGE\nldr x0, [x0, _tls_counter@TLVPPAGEOFF]\nret\n"
        ).unwrap();

        let tls_counter = obj.symbols.iter().find(|sym| sym.name == "_tls_counter").unwrap();
        assert!(tls_counter.undefined);
        assert!(tls_counter.global);

        let relocs = text_relocs(&obj);
        let reloc_types: Vec<_> = relocs.iter().map(|rel| rel.reloc_type).collect();
        assert_eq!(
            reloc_types,
            vec![
                crate::macho::ARM64_RELOC_TLVP_LOAD_PAGE21,
                crate::macho::ARM64_RELOC_TLVP_LOAD_PAGEOFF12,
            ]
        );
        let reloc_syms: Vec<_> = relocs
            .iter()
            .map(|rel| obj.symbols[rel.symbol_idx as usize].name.as_str())
            .collect();
        assert_eq!(reloc_syms, vec!["_tls_counter", "_tls_counter"]);
    }

    #[test]
    fn assemble_quad_local_symbol_creates_unsigned_relocation() {
        let obj = assemble_source(".data\nfoo: .byte 1\n.quad foo\n").unwrap();
        let relocs = data_relocs(&obj);
        assert_eq!(relocs.len(), 1);
        assert_eq!(relocs[0].reloc_type, crate::macho::ARM64_RELOC_UNSIGNED);
        assert_eq!(relocs[0].length, 3);
        assert_eq!(obj.symbols[relocs[0].symbol_idx as usize].name, "foo");
        assert_eq!(&data_bytes(&obj)[1..9], &[0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn assemble_quad_got_expression_creates_pointer_to_got_relocation() {
        let obj = assemble_source(".data\n.quad _puts@GOT\n").unwrap();
        let relocs = data_relocs(&obj);
        assert_eq!(relocs.len(), 1);
        assert_eq!(relocs[0].reloc_type, crate::macho::ARM64_RELOC_POINTER_TO_GOT);
        assert_eq!(relocs[0].length, 3);
        assert!(!relocs[0].pcrel);
        assert_eq!(obj.symbols[relocs[0].symbol_idx as usize].name, "_puts");
        assert_eq!(&data_bytes(&obj)[..8], &[0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn assemble_word_got_pcrel_expression_creates_pointer_to_got_relocation() {
        let obj = assemble_source(".text\n.long _puts@GOT - .\n").unwrap();
        let relocs = text_relocs(&obj);
        assert_eq!(relocs.len(), 1);
        assert_eq!(relocs[0].reloc_type, crate::macho::ARM64_RELOC_POINTER_TO_GOT);
        assert_eq!(relocs[0].length, 2);
        assert!(relocs[0].pcrel);
        assert_eq!(obj.symbols[relocs[0].symbol_idx as usize].name, "_puts");
        assert_eq!(&text_bytes(&obj)[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn assemble_addend_relocations_precede_symbol_relocations() {
        let obj = assemble_source(
            ".text\n.globl _caller\n_caller:\nbl _puts + 4\nadrp x0, _data@PAGE + 0x24\nldr x0, [x0, _data@PAGEOFF + 0x24]\nret\n"
        ).unwrap();

        let relocs = text_relocs(&obj);
        let reloc_types: Vec<_> = relocs.iter().map(|rel| rel.reloc_type).collect();
        assert_eq!(
            reloc_types,
            vec![
                crate::macho::ARM64_RELOC_ADDEND,
                crate::macho::ARM64_RELOC_BRANCH26,
                crate::macho::ARM64_RELOC_ADDEND,
                crate::macho::ARM64_RELOC_PAGE21,
                crate::macho::ARM64_RELOC_ADDEND,
                crate::macho::ARM64_RELOC_PAGEOFF12,
            ]
        );
        assert!(!relocs[0].extern_);
        assert_eq!(relocs[0].symbol_idx, 4);
        assert!(relocs[1].extern_);
        assert_eq!(obj.symbols[relocs[1].symbol_idx as usize].name, "_puts");
        assert!(!relocs[2].extern_);
        assert_eq!(relocs[2].symbol_idx, 0x24);
        assert!(relocs[3].extern_);
        assert_eq!(obj.symbols[relocs[3].symbol_idx as usize].name, "_data");
        assert!(!relocs[4].extern_);
        assert_eq!(relocs[4].symbol_idx, 0x24);
        assert!(relocs[5].extern_);
        assert_eq!(obj.symbols[relocs[5].symbol_idx as usize].name, "_data");
    }

    #[test]
    fn assemble_quad_same_section_difference_is_constant() {
        let obj = assemble_source(".data\nfoo: .byte 1\nbar: .byte 2\n.quad bar - foo\n").unwrap();
        assert!(data_relocs(&obj).is_empty());
        assert_eq!(&data_bytes(&obj)[2..10], &1u64.to_le_bytes());
    }

    #[test]
    fn assemble_quad_external_difference_creates_subtractor_pair() {
        let obj = assemble_source(".data\n.quad _ext - _other + 4\n").unwrap();
        let relocs = data_relocs(&obj);
        assert_eq!(relocs.len(), 2);
        assert_eq!(relocs[0].reloc_type, crate::macho::ARM64_RELOC_SUBTRACTOR);
        assert_eq!(relocs[1].reloc_type, crate::macho::ARM64_RELOC_UNSIGNED);
        assert_eq!(obj.symbols[relocs[0].symbol_idx as usize].name, "_other");
        assert_eq!(obj.symbols[relocs[1].symbol_idx as usize].name, "_ext");
        assert_eq!(&data_bytes(&obj)[0..8], &4u64.to_le_bytes());
    }

    #[test]
    fn assemble_word_symbolic_expression_is_rejected() {
        let err = assemble_source(".data\n.word foo\n").unwrap_err();
        assert!(err.msg.contains("absolute value"), "got: {}", err);
    }

    #[test]
    fn assemble_forward_branch_label() {
        let obj = assemble_source(".text\nstart:\nb done\nnop\ndone:\nret\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::B { offset: 8 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_backward_branch_label() {
        let obj = assemble_source(".text\nstart:\nnop\nb start\n").unwrap();
        assert_eq!(&text_bytes(&obj)[4..8], &Inst::B { offset: -4 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_local_cbz_label() {
        let obj = assemble_source(".text\nstart:\ncbz x0, done\nret\ndone:\nret\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::Cbz { rt: X0, offset: 8, sf: true }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_local_tbz_label() {
        let obj = assemble_source(".text\ntbz x0, #5, done\nnop\ndone:\nret\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::Tbz { rt: X0, bit: 5, offset: 8, sf: true }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_adr_local_label() {
        let obj = assemble_source(".text\nadr x0, target\nret\ntarget:\nret\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::Adr { rd: X0, imm: 8 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_ldr_literal_local_label() {
        let obj = assemble_source(".text\nldr x0, target\nret\n.p2align 3\ntarget:\n.quad 42\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::LdrLit64 { rt: X0, offset: 8 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_ldrsw_literal_local_label() {
        let obj = assemble_source(".text\nldrsw x0, target\nret\ntarget:\n.word -1\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::LdrswLit { rt: X0, offset: 8 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_ldr_d_literal_local_label() {
        let obj = assemble_source(".text\nldr d0, target\nret\n.p2align 3\ntarget:\n.quad 42\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::LdrFpLit64 { rt: D0, offset: 8 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_ldr_s_literal_local_label() {
        let obj = assemble_source(".text\nldr s0, target\nret\ntarget:\n.word 42\n").unwrap();
        assert_eq!(&text_bytes(&obj)[0..4], &Inst::LdrFpLit32 { rt: S0, offset: 8 }.encode().to_le_bytes());
    }

    #[test]
    fn assemble_ldr_literal_requires_local_label() {
        let err = assemble_source(".text\nldr x0, _ext\n").unwrap_err();
        assert!(err.msg.contains("assembler-local label"), "got: {}", err);
    }

    #[test]
    fn assemble_ldr_literal_requires_same_section() {
        let err = assemble_source(".text\nldr x0, target\n.data\ntarget: .quad 42\n").unwrap_err();
        assert!(err.msg.contains("current section"), "got: {}", err);
    }

    #[test]
    fn assemble_adr_requires_local_label() {
        let err = assemble_source(".text\nadr x0, _ext\n").unwrap_err();
        assert!(err.msg.contains("assembler-local label"), "got: {}", err);
    }

    #[test]
    fn assemble_external_bl_creates_branch_relocation() {
        let obj = assemble_source(".text\nbl _puts\nret\n").unwrap();
        let reloc_name = &obj.symbols[text_relocs(&obj)[0].symbol_idx as usize].name;
        assert_eq!(reloc_name, "_puts");
        assert!(obj.symbols.iter().any(|sym| sym.name == "_puts" && sym.undefined));
    }

    #[test]
    fn assemble_external_b_creates_branch_relocation() {
        let obj = assemble_source(".text\nb _exit\n").unwrap();
        let reloc_name = &obj.symbols[text_relocs(&obj)[0].symbol_idx as usize].name;
        assert_eq!(reloc_name, "_exit");
        assert!(obj.symbols.iter().any(|sym| sym.name == "_exit" && sym.undefined));
    }

    #[test]
    fn assemble_missing_numeric_local_label_is_rejected() {
        let err = assemble_source(".text\nb 1f\n").unwrap_err();
        assert!(err.msg.contains("local symbol '.Ltmp$1$1'"), "got: {}", err);
    }

    #[test]
    fn assemble_numeric_local_labels_do_not_emit_temp_symbols() {
        let obj = assemble_source(".text\n.globl _f\n_f:\n1:\n  b 1b\n").unwrap();
        assert!(
            !obj.symbols.iter().any(|sym| sym.name.starts_with(".Ltmp$")),
            "generated numeric local labels leaked into symbol table: {:?}",
            obj.symbols
                .iter()
                .map(|sym| sym.name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn assemble_uppercase_l_labels_do_not_emit_object_symbols() {
        let obj = assemble_source(".text\n.globl _f\n_f:\nLtmp0:\n  ret\n").unwrap();
        assert!(
            !obj.symbols.iter().any(|sym| sym.name == "Ltmp0"),
            "uppercase-L temp labels leaked into symbol table: {:?}",
            obj.symbols
                .iter()
                .map(|sym| sym.name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn assemble_linker_optimization_hint_emits_payload() {
        let obj = assemble_source(
            ".text\n\
             .globl _f\n\
             _f:\n\
             Lloh0:\n\
               adrp x0, _ext@PAGE\n\
             Lloh1:\n\
               add x0, x0, _ext@PAGEOFF\n\
               ret\n\
               .loh AdrpAdd Lloh0, Lloh1\n",
        )
        .unwrap();
        assert_eq!(obj.linker_optimization_hints, vec![7, 2, 0, 4, 0, 0, 0, 0]);
    }

    #[test]
    fn assemble_page_reloc_target_at_section_base_sorts_before_section_temp() {
        let obj = assemble_source(
            ".globl _main\n\
             .text\n\
             _main:\n\
               adrp x0, msg@PAGE\n\
               add x0, x0, msg@PAGEOFF\n\
               ret\n\
             .data\n\
             msg:\n\
               .quad 0\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let msg_index = names.iter().position(|name| *name == "msg").expect("msg symbol");
        let ltmp1_index = names.iter().position(|name| *name == "ltmp1").expect("ltmp1 symbol");
        assert!(msg_index < ltmp1_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_branch19_requires_local_label() {
        let err = assemble_source(".text\nb.eq _foo\n").unwrap_err();
        assert!(err.msg.contains("assembler-local label"), "got: {}", err);
    }

    #[test]
    fn assemble_branch14_requires_local_label() {
        let err = assemble_source(".text\ntbnz x0, #33, _foo\n").unwrap_err();
        assert!(err.msg.contains("assembler-local label"), "got: {}", err);
    }

    #[test]
    fn assemble_branch_rejects_misaligned_local_target() {
        let err = assemble_source(".text\nb done\n.byte 0\ndone:\nret\n").unwrap_err();
        assert!(err.msg.contains("not 4-byte aligned"), "got: {}", err);
    }

    #[test]
    fn assemble_branch19_rejects_out_of_range_target() {
        let err = assemble_source(".text\ncbz x0, done\n.space 1048576\ndone:\nret\n").unwrap_err();
        assert!(err.msg.contains("out of range"), "got: {}", err);
    }

    #[test]
    fn assemble_branch14_rejects_out_of_range_target() {
        let err = assemble_source(".text\ntbz x0, #5, done\n.space 32768\ndone:\nret\n").unwrap_err();
        assert!(err.msg.contains("out of range"), "got: {}", err);
    }

    #[test]
    fn check_branch_offset_rejects_branch26_out_of_range() {
        let err = check_branch_offset(1i64 << 27, 26).unwrap_err();
        assert!(err.msg.contains("out of range"), "got: {}", err);
    }

    #[test]
    fn check_pcrel_offset_rejects_adr21_out_of_range() {
        let err = check_pcrel_offset(1i64 << 20, 21, "adr offset").unwrap_err();
        assert!(err.msg.contains("out of range"), "got: {}", err);
    }
}
