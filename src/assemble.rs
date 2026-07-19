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
    let src =
        fs::read_to_string(input).map_err(|e| AsmError::new(format!("{}", e)).with_path(input))?;

    let obj = assemble_source(&src).map_err(|e| e.with_source_context(input, &src))?;

    let file =
        fs::File::create(output).map_err(|e| AsmError::new(format!("{}", e)).with_path(output))?;
    let mut w = BufWriter::new(file);
    macho::write_macho(&obj, &mut w)
        .map_err(|e| AsmError::new(format!("writing output: {}", e)).with_path(output))?;

    Ok(())
}

/// Assemble source text into an ObjectFile (library API).
///
/// # Examples
///
/// ```
/// let obj = afs_as::assemble::assemble_source(
///     ".global _main\n.text\n_main:\n    ret\n",
/// )
/// .unwrap();
/// // `ret` encodes to 0xD65F03C0 (little-endian).
/// assert_eq!(obj.text_section().data, [0xC0, 0x03, 0x5F, 0xD6]);
/// assert_eq!(obj.text_section().size, 4);
/// assert!(obj.symbols.iter().any(|s| s.name == "_main" && s.global));
/// ```
pub fn assemble_source(src: &str) -> Result<ObjectFile, AsmError> {
    let stmts = parse::parse_with_locations(src).map_err(AsmError::from)?;
    assemble_located_stmts(&stmts)
}

/// Assemble pre-parsed statements into an ObjectFile.
pub fn assemble_stmts(stmts: &[Stmt]) -> Result<ObjectFile, AsmError> {
    let stmts: Vec<_> = stmts
        .iter()
        .cloned()
        .map(|stmt| LocatedStmt {
            stmt,
            line: 0,
            col: 0,
        })
        .collect();
    assemble_located_stmts(&stmts)
}

fn assemble_located_stmts(stmts: &[LocatedStmt]) -> Result<ObjectFile, AsmError> {
    let mut asm = Assembler::new();
    asm.collect_layout(stmts)?;
    asm.prepare_unwind_layout()?;
    asm.prepare_expression_state(stmts)?;
    asm.reset_for_emission();
    asm.process(stmts)?;
    asm.resolve_fixups()?;
    asm.finish()
}

/// Assemble a list of pre-encoded instructions into an ObjectFile (compiler API).
/// No parsing needed — the compiler builds Inst values directly.
///
/// This is a trusted fast path: it assumes every `Inst` is valid and will
/// **panic** on encoder precondition failures. Source-level validation lives
/// in [`assemble_source`].
///
/// # Examples
///
/// ```
/// use afs_as::assemble::assemble_instructions;
/// use afs_as::encode::Inst;
/// use afs_as::reg::X30;
///
/// let obj = assemble_instructions(&[Inst::Nop, Inst::Ret { rn: X30 }], &["_main"]);
/// assert_eq!(
///     obj.text_section().data,
///     [0x1F, 0x20, 0x03, 0xD5, 0xC0, 0x03, 0x5F, 0xD6],
/// );
/// assert!(obj.symbols.iter().any(|s| s.name == "_main" && s.global));
/// ```
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
        Self {
            path: None,
            line: None,
            col: None,
            msg,
            snippet: None,
        }
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
                writeln!(
                    f,
                    "{}:{}:{}: error: {}",
                    path.display(),
                    line,
                    col,
                    self.msg
                )?;
                if let Some(snippet) = &self.snippet {
                    writeln!(f, "{}", snippet)?;
                    write!(f, "{}^", diagnostic_caret_prefix(snippet, col))?;
                }
                Ok(())
            }
            (Some(path), _, _) => write!(f, "{}: error: {}", path.display(), self.msg),
            (None, Some(line), Some(col)) => write!(f, "{}:{}: error: {}", line, col, self.msg),
            _ => write!(f, "error: {}", self.msg),
        }
    }
}

fn diagnostic_caret_prefix(snippet: &str, col: u32) -> String {
    let mut chars = snippet.chars();
    (0..col.saturating_sub(1))
        .map(|_| match chars.next() {
            Some('\t') => '\t',
            Some(_) | None => ' ',
        })
        .collect()
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
    source_section_count: usize,

    /// Labels → (section index, offset within section).
    labels: BTreeMap<String, (usize, u64)>,
    /// Absolute symbol assignments declared via `.set` / `.equ`, in source order.
    absolute_assignments: Vec<(String, Expr)>,
    absolute_symbol_names: BTreeSet<String>,
    absolute_assignment_values: Vec<i64>,
    next_absolute_assignment: usize,
    initial_absolute_symbols: BTreeMap<String, i64>,
    absolute_symbols: BTreeMap<String, i64>,
    final_absolute_symbols: BTreeMap<String, i64>,
    common_symbols: BTreeMap<String, CommonSymbol>,
    symbol_order: BTreeMap<String, usize>,
    next_symbol_order: usize,
    section_bases: Vec<u64>,
    expected_section_layout: Vec<SectionLayoutFingerprint>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SectionLayoutFingerprint {
    segment: String,
    name: String,
    kind: SectionKind,
    align_pow2: u32,
    size: u64,
}

impl From<&Section> for SectionLayoutFingerprint {
    fn from(section: &Section) -> Self {
        Self {
            segment: section.segment.clone(),
            name: section.name.clone(),
            kind: section.kind.clone(),
            align_pow2: section.align_pow2,
            size: section.size,
        }
    }
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
const COMPACT_UNWIND_ENTRY_SIZE: u64 = 32;
const EH_FRAME_CIE_SIZE: u64 = 20;
const EH_FRAME_FDE_FIXED_SIZE: usize = 25;

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
            if self.cfa_offset < 0 {
                return Err(AsmError(
                    "compact unwind stack size must be non-negative".into(),
                ));
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
            let mut encoding = UNWIND_ARM64_MODE_FRAMELESS | ((scaled as u32) << 12);
            let mut next_low_offset = -8;
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
                        if low_offset == next_low_offset && high_offset == next_low_offset - 8 =>
                    {
                        encoding |= bit;
                        next_low_offset -= 16;
                    }
                    (Some(_), Some(_)) => return Ok(None),
                    _ => return Ok(None),
                }
            }

            for reg in self.saved_gp_offsets.keys() {
                if !matches!(*reg, 19..=28) {
                    return Ok(None);
                }
            }

            return Ok(Some(encoding));
        }

        if self.cfa_register.num() != 29 {
            return Ok(None);
        }
        if self.cfa_offset != 16 {
            return Ok(None);
        }
        if self.saved_gp_offsets.get(&29) != Some(&-16)
            || self.saved_gp_offsets.get(&30) != Some(&-8)
        {
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
            source_section_count: 1,
            labels: BTreeMap::new(),
            absolute_assignments: Vec::new(),
            absolute_symbol_names: BTreeSet::new(),
            absolute_assignment_values: Vec::new(),
            next_absolute_assignment: 0,
            initial_absolute_symbols: BTreeMap::new(),
            absolute_symbols: BTreeMap::new(),
            final_absolute_symbols: BTreeMap::new(),
            common_symbols: BTreeMap::new(),
            symbol_order: BTreeMap::from([(String::from("ltmp0"), 0)]),
            next_symbol_order: 1,
            section_bases: Vec::new(),
            expected_section_layout: Vec::new(),
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
        self.sections.truncate(self.source_section_count);
        for section in &mut self.sections {
            section.data.clear();
            section.relocations.clear();
            section.has_instructions = false;
            section.size = 0;
        }
        self.fixups.clear();
        self.next_absolute_assignment = 0;
        self.absolute_symbols
            .clone_from(&self.initial_absolute_symbols);
        self.pending_relocs.clear();
        self.pending_relocs
            .resize_with(self.sections.len(), Vec::new);
        self.active_cfi_proc = None;
        self.linker_optimization_hints.clear();
    }

    fn note_stmt_location(&mut self, line: u32, col: u32) {
        self.current_line = line;
        self.current_col = col;
    }

    fn prepare_expression_state(&mut self, stmts: &[LocatedStmt]) -> Result<(), AsmError> {
        self.section_bases = match self.section_base_addresses() {
            Ok(bases) => bases,
            Err(error) => {
                let error = match Self::section_layout_error_location(stmts) {
                    Some((line, col)) => error.with_loc_if_absent(line, col),
                    None => error,
                };
                return Err(error);
            }
        };
        self.expected_section_layout = self
            .sections
            .iter()
            .map(SectionLayoutFingerprint::from)
            .collect();
        let assignment_results = expr::resolve_absolute_assignments(
            &self.absolute_assignments,
            &self.label_values_for_expr(),
        );
        let mut first_names = BTreeMap::new();
        self.initial_absolute_symbols.clear();
        self.final_absolute_symbols.clear();
        self.absolute_assignment_values.clear();
        for ((name, _), result) in self.absolute_assignments.iter().zip(assignment_results) {
            let value = result.map_err(AsmError)?;
            if first_names.insert(name.clone(), ()).is_none() {
                self.initial_absolute_symbols.insert(name.clone(), value);
            }
            self.final_absolute_symbols.insert(name.clone(), value);
            self.absolute_assignment_values.push(value);
        }
        self.absolute_symbols
            .clone_from(&self.final_absolute_symbols);
        Ok(())
    }

    fn prepare_unwind_layout(&mut self) -> Result<(), AsmError> {
        self.source_section_count = self.sections.len();
        for section in self.unwind_layout_sections()? {
            self.note_section_temp(self.sections.len());
            self.sections.push(section);
            self.pending_relocs.push(Vec::new());
        }
        Ok(())
    }

    fn collect_layout(&mut self, stmts: &[LocatedStmt]) -> Result<(), AsmError> {
        self.section = 0;

        for stmt in stmts {
            self.collect_layout_stmt(stmt)?;
        }

        if self.active_cfi_proc.is_some() {
            return Err(AsmError(
                "unterminated .cfi_startproc before end of file".into(),
            ));
        }

        Ok(())
    }

    fn collect_layout_stmt(&mut self, stmt: &LocatedStmt) -> Result<(), AsmError> {
        match &stmt.stmt {
            Stmt::Label(name) => {
                if self.common_symbols.contains_key(name) {
                    return Err(AsmError(format!("duplicate symbol '{}'", name))
                        .with_loc_if_absent(stmt.line, stmt.col));
                }
                self.note_symbol(name);
                let offset = self.current_offset();
                if self
                    .labels
                    .insert(name.clone(), (self.section, offset))
                    .is_some()
                {
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
        Ok(())
    }

    fn section_layout_error_location(stmts: &[LocatedStmt]) -> Option<(u32, u32)> {
        let mut probe = Self::new();
        for stmt in stmts {
            probe.collect_layout_stmt(stmt).ok()?;
            let mut sections = probe.sections.clone();
            sections.extend(probe.unwind_layout_sections().ok()?);
            if Self::section_base_addresses_for(&sections).is_err() {
                return Some((stmt.line, stmt.col));
            }
        }
        None
    }

    fn process(&mut self, stmts: &[LocatedStmt]) -> Result<(), AsmError> {
        for stmt in stmts {
            self.note_stmt_location(stmt.line, stmt.col);
            match &stmt.stmt {
                Stmt::Label(_) => {}
                Stmt::Directive(dir) => self
                    .process_directive(dir)
                    .map_err(|e| e.with_loc_if_absent(stmt.line, stmt.col))?,
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
                self.absolute_symbol_names.insert(name.clone());
                self.absolute_assignments.push((name.clone(), expr.clone()));
            }
            Directive::Comm {
                name,
                size,
                align_pow2,
            } => {
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
            Directive::Align {
                power, max_skip, ..
            }
            | Directive::P2Align {
                power, max_skip, ..
            } => {
                self.collect_alignment(*power, *max_skip)?;
            }
            Directive::Byte(vals) => self.reserve_initialized_bytes(vals.len() as u64, ".byte")?,
            Directive::Short(vals) => {
                self.reserve_initialized_bytes((vals.len() as u64) * 2, ".short")?
            }
            Directive::Word(vals) => {
                self.reserve_initialized_bytes((vals.len() as u64) * 4, ".word")?
            }
            Directive::Quad(vals) => {
                self.reserve_initialized_bytes((vals.len() as u64) * 8, ".quad")?
            }
            Directive::Ascii(bytes) | Directive::Asciz(bytes) => {
                self.reserve_initialized_bytes(bytes.len() as u64, "string directive")?;
            }
            Directive::Space(n) => {
                if *n > 1024 * 1024 * 64 {
                    return Err(AsmError(format!(".space size {} too large (max 64MB)", n)));
                }
                self.sections[self.section].size = self.sections[self.section]
                    .size
                    .checked_add(*n)
                    .ok_or_else(|| AsmError(".space size overflows u64".into()))?;
            }
            Directive::Fill { repeat, size, .. } => {
                let total = (*repeat)
                    .checked_mul((*size).min(8).into())
                    .ok_or_else(|| AsmError(".fill size overflows u64".into()))?;
                self.reserve_initialized_bytes(total, ".fill")?;
            }
            Directive::Zerofill {
                segment,
                section,
                symbol,
                size,
                align_pow2,
            } => {
                self.reserve_zerofill(segment, section, symbol.as_deref(), *size, *align_pow2)?;
            }
            Directive::CfiStartProc => self.start_cfi_proc()?,
            Directive::CfiEndProc => self.finish_cfi_proc()?,
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

    fn process_directive(&mut self, dir: &Directive) -> Result<(), AsmError> {
        match dir {
            Directive::Text => self.switch_to("__TEXT", "__text")?,
            Directive::Data => self.switch_to("__DATA", "__data")?,
            Directive::Set(name, _) => self.activate_absolute_definition(name)?,
            Directive::Comm { .. }
            | Directive::Extern(_)
            | Directive::Global(_)
            | Directive::PrivateExtern(_)
            | Directive::WeakReference(_)
            | Directive::WeakDefinition(_) => {}
            Directive::Align {
                power,
                fill,
                max_skip,
            }
            | Directive::P2Align {
                power,
                fill,
                max_skip,
            } => {
                self.emit_alignment(*power, *fill, *max_skip)?;
            }
            Directive::Byte(vals) => {
                for expr in vals {
                    let bytes = self.require_sized_absolute_expr(expr, ".byte expression", 8)?;
                    self.emit_initialized_bytes(&bytes[..1], ".byte")?;
                }
            }
            Directive::Short(vals) => {
                for expr in vals {
                    let bytes = self.require_sized_absolute_expr(expr, ".short expression", 16)?;
                    self.emit_initialized_bytes(&bytes[..2], ".short")?;
                }
            }
            Directive::Word(vals) => {
                for expr in vals {
                    match self.classify_expr(expr)? {
                        ClassifiedExpr::Absolute(value) => {
                            let bytes = checked_signed_data_bytes(value, ".word expression", 32)?;
                            self.emit_initialized_bytes(&bytes[..4], ".word")?;
                        }
                        ClassifiedExpr::UnsignedAbsolute(value) => {
                            let bytes = checked_unsigned_data_bytes(value, ".word expression", 32)?;
                            self.emit_initialized_bytes(&bytes[..4], ".word")?;
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
            Directive::Fill {
                repeat,
                size,
                value,
            } => {
                self.emit_fill(*repeat, *size, *value)?;
            }
            Directive::Zerofill {
                segment,
                section,
                size,
                align_pow2,
                ..
            } => {
                self.emit_zerofill(segment, section, *size, *align_pow2)?;
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

    fn function_symbol_at(&self, section: usize, offset: u64) -> Option<String> {
        let mut exact: Vec<_> = self
            .labels
            .iter()
            .filter(|(_, (label_section, label_offset))| {
                *label_section == section && *label_offset == offset
            })
            .map(|(name, _)| name.as_str())
            .collect();

        exact.sort_unstable();

        exact
            .iter()
            .copied()
            .find(|name| {
                !is_assembler_local_symbol(name)
                    && self
                        .symbol_attrs
                        .get(*name)
                        .is_some_and(|attrs| attrs.global)
            })
            .or_else(|| {
                exact
                    .iter()
                    .copied()
                    .find(|name| !is_assembler_local_symbol(name))
            })
            .or_else(|| exact.into_iter().next())
            .map(str::to_string)
    }

    fn start_cfi_proc(&mut self) -> Result<(), AsmError> {
        if self.active_cfi_proc.is_some() {
            return Err(AsmError(
                "nested .cfi_startproc directives are not supported".into(),
            ));
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
        self.active_cfi_proc = Some(CfiProcState::new(
            self.section,
            start_offset,
            function_symbol,
        ));
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
                    return Err(AsmError(format!(
                        ".cfi_def_cfa offset {} must be non-negative",
                        offset
                    )));
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
            self.eh_frame_rows
                .push(proc.eh_frame_record(self.current_offset())?);
        }
        Ok(())
    }

    fn record_build_version(
        &mut self,
        build_version: &BuildVersionDirective,
    ) -> Result<(), AsmError> {
        if !build_version.platform.eq_ignore_ascii_case("macos") {
            return Err(AsmError(format!(
                "unsupported .build_version platform '{}' (supported: macos)",
                build_version.platform
            )));
        }
        // Apple `as` accepts repeated .build_version directives and uses the last one.
        self.build_version = Some(build_version.clone());
        Ok(())
    }

    fn record_linker_optimization_hint(&mut self, hint: &LinkerOptimizationHintDirective) {
        self.linker_optimization_hints.push(LinkerOptimizationHint {
            kind: hint.kind.clone(),
            labels: hint.labels.clone(),
        });
    }

    fn reserve_initialized_bytes(&mut self, amount: u64, context: &str) -> Result<(), AsmError> {
        if self.sections[self.section].kind.is_zerofill() {
            return Err(AsmError(format!(
                "{} is not supported in zero-fill section {},{}",
                context, self.sections[self.section].segment, self.sections[self.section].name
            )));
        }
        self.sections[self.section].size = self.sections[self.section]
            .size
            .checked_add(amount)
            .ok_or_else(|| AsmError(format!("{} size overflows u64", context)))?;
        Ok(())
    }

    fn emit_initialized_bytes(&mut self, bytes: &[u8], context: &str) -> Result<(), AsmError> {
        self.reserve_initialized_bytes(bytes.len() as u64, context)?;
        self.sections[self.section].data.extend_from_slice(bytes);
        Ok(())
    }

    fn emit_space(&mut self, amount: u64) -> Result<(), AsmError> {
        if self.sections[self.section].kind.is_zerofill() {
            self.sections[self.section].size = self.sections[self.section]
                .size
                .checked_add(amount)
                .ok_or_else(|| AsmError(".space size overflows u64".into()))?;
            return Ok(());
        }
        let section = &mut self.sections[self.section];
        let new_size = section
            .size
            .checked_add(amount)
            .ok_or_else(|| AsmError(".space size overflows u64".into()))?;
        let amount = usize::try_from(amount)
            .map_err(|_| AsmError(".space size does not fit in usize".into()))?;
        let new_len = section
            .data
            .len()
            .checked_add(amount)
            .ok_or_else(|| AsmError(".space size overflows usize".into()))?;
        section.data.resize(new_len, 0);
        section.size = new_size;
        Ok(())
    }

    fn emit_fill(&mut self, repeat: u64, size: u8, value: u64) -> Result<(), AsmError> {
        let byte_count: usize = size.min(8).into();
        let total = repeat
            .checked_mul(byte_count as u64)
            .ok_or_else(|| AsmError(".fill size overflows u64".into()))?;
        if total > 1024 * 1024 * 64 {
            return Err(AsmError(format!(
                ".fill size {} too large (max 64MB)",
                total
            )));
        }
        if byte_count == 0 {
            return Ok(());
        }
        // Darwin truncates the pattern to 32 bits before extending it to the element width.
        let pattern = u64::from(value as u32).to_le_bytes();
        for _ in 0..repeat {
            self.emit_initialized_bytes(&pattern[..byte_count], ".fill")?;
        }
        Ok(())
    }

    fn collect_alignment(&mut self, power: u32, max_skip: Option<u64>) -> Result<(), AsmError> {
        if power > 30 {
            return Err(AsmError(format!(
                "alignment power {} too large (max 30)",
                power
            )));
        }

        let current = self.current_offset();
        let aligned = checked_align_value(current, power)
            .ok_or_else(|| AsmError("alignment overflows u64".into()))?;
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
            return Err(AsmError(format!(
                "alignment power {} too large (max 30)",
                power
            )));
        }

        {
            let section = &mut self.sections[self.section];
            section.align_pow2 = section.align_pow2.max(power);
        }

        let current = self.current_offset();
        let aligned = checked_align_value(current, power)
            .ok_or_else(|| AsmError("alignment overflows u64".into()))?;
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
        if self.sections[self.section].kind.is_zerofill() {
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
        let amount_usize = usize::try_from(amount)
            .map_err(|_| AsmError(format!("{} size does not fit in usize", context)))?;
        let new_len = self.sections[self.section]
            .data
            .len()
            .checked_add(amount_usize)
            .ok_or_else(|| AsmError(format!("{} size overflows usize", context)))?;
        self.reserve_initialized_bytes(amount, context)?;
        let section = &mut self.sections[self.section];
        section.data.resize(new_len, byte);
        Ok(())
    }

    fn resolve_fixups(&mut self) -> Result<(), AsmError> {
        let fixups = std::mem::take(&mut self.fixups);
        for fixup in fixups {
            let line = fixup.line;
            let col = fixup.col;
            let result = match fixup.kind.clone() {
                FixupKind::Branch26(template) => {
                    self.resolve_branch_or_reloc(fixup, template, 26, true)
                }
                FixupKind::Branch19(template) => {
                    self.resolve_branch_or_reloc(fixup, template, 19, false)
                }
                FixupKind::Branch14(template) => {
                    self.resolve_branch_or_reloc(fixup, template, 14, false)
                }
                FixupKind::Literal19(template) => self.resolve_literal_fixup(fixup, template),
                FixupKind::Adr21(template) => self.resolve_adr_fixup(fixup, template),
                FixupKind::Page21 => {
                    self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_PAGE21, true)
                }
                FixupKind::GotLoadPage21 => {
                    self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_GOT_LOAD_PAGE21, true)
                }
                FixupKind::TlvpLoadPage21 => {
                    self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_TLVP_LOAD_PAGE21, true)
                }
                FixupKind::PageOff12 => {
                    self.resolve_page_fixup(fixup, crate::macho::ARM64_RELOC_PAGEOFF12, false)
                }
                FixupKind::GotLoadPageOff12 => self.resolve_page_fixup(
                    fixup,
                    crate::macho::ARM64_RELOC_GOT_LOAD_PAGEOFF12,
                    false,
                ),
                FixupKind::TlvpLoadPageOff12 => self.resolve_page_fixup(
                    fixup,
                    crate::macho::ARM64_RELOC_TLVP_LOAD_PAGEOFF12,
                    false,
                ),
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
            if self.should_emit_local_branch_reloc(
                &symbol,
                fixup.section,
                *target_section,
                addend,
                bits,
            ) {
                self.record_pending_reloc(
                    fixup.section,
                    fixup.offset,
                    symbol,
                    2,
                    crate::macho::ARM64_RELOC_BRANCH26,
                    true,
                );
                return Ok(());
            }
            self.ensure_same_section(fixup.section, *target_section, &symbol)?;
            let delta = (*target_offset as i64)
                .checked_sub(fixup.offset as i64)
                .and_then(|value| value.checked_add(addend))
                .ok_or_else(|| AsmError("branch offset overflows i64".into()))?;
            let resolved = self.resolve_branch_fixup(&template, delta, bits)?;
            self.patch_section_data(
                fixup.section,
                fixup.offset,
                &resolved.encode().to_le_bytes(),
                "branch fixup",
            )?;
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

    fn should_emit_local_branch_reloc(
        &self,
        symbol: &str,
        source_section: usize,
        target_section: usize,
        addend: i64,
        bits: u8,
    ) -> bool {
        bits == 26
            && addend == 0
            && self.subsections_via_symbols
            && source_section == target_section
            && self.sections[target_section].kind == SectionKind::Text
            && !is_assembler_local_symbol(symbol)
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
        let (symbol, addend) =
            self.require_relocatable_symbol(&fixup.expr, "ldr literal target")?;
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
        self.patch_section_data(
            fixup.section,
            fixup.offset,
            &resolved.encode().to_le_bytes(),
            "ldr literal fixup",
        )?;
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
        self.patch_section_data(
            fixup.section,
            fixup.offset,
            &resolved.encode().to_le_bytes(),
            "adr fixup",
        )?;
        Ok(())
    }

    fn resolve_data64_fixup(&mut self, fixup: Fixup) -> Result<(), AsmError> {
        match self.classify_expr(&fixup.expr)? {
            ClassifiedExpr::Absolute(value) => {
                self.patch_section_data(
                    fixup.section,
                    fixup.offset,
                    &(value as u64).to_le_bytes(),
                    ".quad",
                )?;
            }
            ClassifiedExpr::UnsignedAbsolute(value) => {
                self.patch_section_data(
                    fixup.section,
                    fixup.offset,
                    &value.to_le_bytes(),
                    ".quad",
                )?;
            }
            ClassifiedExpr::Relocatable { symbol, addend } => {
                self.patch_section_data(
                    fixup.section,
                    fixup.offset,
                    &(addend as u64).to_le_bytes(),
                    ".quad",
                )?;
                self.record_pending_reloc(
                    fixup.section,
                    fixup.offset,
                    symbol,
                    3,
                    crate::macho::ARM64_RELOC_UNSIGNED,
                    false,
                );
            }
            ClassifiedExpr::Difference {
                minuend,
                subtrahend,
                addend,
            } => {
                self.patch_section_data(
                    fixup.section,
                    fixup.offset,
                    &(addend as u64).to_le_bytes(),
                    ".quad",
                )?;
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
            ClassifiedExpr::PointerToGot {
                symbol,
                addend,
                pcrel,
            } => {
                self.patch_section_data(
                    fixup.section,
                    fixup.offset,
                    &(addend as u64).to_le_bytes(),
                    ".quad",
                )?;
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
            ClassifiedExpr::UnsignedAbsolute(value) => Err(AsmError(format!(
                ".word unsigned value {} is out of range; use .quad for a 64-bit bit pattern",
                value
            ))),
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

    fn require_relocatable_symbol(
        &self,
        expr: &Expr,
        context: &str,
    ) -> Result<(String, i64), AsmError> {
        match self.classify_expr(expr)? {
            ClassifiedExpr::Relocatable { symbol, addend } => Ok((symbol, addend)),
            ClassifiedExpr::Absolute(_) | ClassifiedExpr::UnsignedAbsolute(_) => Err(AsmError(
                format!("{} must resolve to a relocatable symbol", context),
            )),
            ClassifiedExpr::Difference { .. } | ClassifiedExpr::PointerToGot { .. } => {
                Err(AsmError(format!(
                    "{} must resolve to a single relocatable symbol",
                    context
                )))
            }
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
                context, section.segment, section.name
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

    fn record_common_symbol(
        &mut self,
        name: &str,
        size: u64,
        align_pow2: u8,
    ) -> Result<(), AsmError> {
        if align_pow2 > 15 {
            return Err(AsmError(format!(
                "common symbol '{}' alignment power {} too large (max 15)",
                name, align_pow2
            )));
        }
        if self.labels.contains_key(name) || self.absolute_symbol_names.contains(name) {
            return Err(AsmError(format!("duplicate symbol '{}'", name)));
        }
        if self
            .common_symbols
            .insert(name.to_string(), CommonSymbol { size, align_pow2 })
            .is_some()
        {
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
        if !self.sections[target].kind.is_zerofill() {
            return Err(AsmError(format!(
                ".zerofill requires a zero-fill section, got {},{}",
                seg, sect
            )));
        }

        let offset = {
            let section = &mut self.sections[target];
            section.align_pow2 = section.align_pow2.max(align_pow2);
            let offset = checked_align_value(section.size, align_pow2)
                .ok_or_else(|| AsmError(".zerofill alignment overflows u64".into()))?;
            section.size = offset;
            offset
        };

        if let Some(symbol) = symbol {
            if self.common_symbols.contains_key(symbol)
                || self.absolute_symbol_names.contains(symbol)
            {
                return Err(AsmError(format!("duplicate symbol '{}'", symbol)));
            }
            self.note_symbol(symbol);
            if self
                .labels
                .insert(symbol.to_string(), (target, offset))
                .is_some()
            {
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
        if !self.sections[target].kind.is_zerofill() {
            return Err(AsmError(format!(
                ".zerofill requires a zero-fill section, got {},{}",
                seg, sect
            )));
        }

        let section = &mut self.sections[target];
        section.align_pow2 = section.align_pow2.max(align_pow2);
        section.size = checked_align_value(section.size, align_pow2)
            .ok_or_else(|| AsmError(".zerofill alignment overflows u64".into()))?;
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

    fn supported_section(
        seg: &str,
        sect: &str,
    ) -> Result<(&'static str, &'static str, SectionKind), AsmError> {
        let seg_lower = seg.to_lowercase();
        let sect_lower = sect.to_lowercase();
        if seg_lower == "__text" && sect_lower == "__text" {
            Ok(("__TEXT", "__text", SectionKind::Text))
        } else if seg_lower == "__text" && sect_lower == "__cstring" {
            Ok(("__TEXT", "__cstring", SectionKind::CStringLiterals))
        } else if seg_lower == "__text" && sect_lower == "__literal16" {
            Ok(("__TEXT", "__literal16", SectionKind::Literal16))
        } else if seg_lower == "__text" && sect_lower == "__const" {
            Ok(("__TEXT", "__const", SectionKind::ConstData))
        } else if seg_lower == "__data" && sect_lower == "__data" {
            Ok(("__DATA", "__data", SectionKind::Data))
        } else if seg_lower == "__data" && sect_lower == "__const" {
            Ok(("__DATA", "__const", SectionKind::ConstData))
        } else if seg_lower == "__data" && sect_lower == "__thread_data" {
            Ok(("__DATA", "__thread_data", SectionKind::ThreadLocalData))
        } else if seg_lower == "__data" && sect_lower == "__thread_vars" {
            Ok(("__DATA", "__thread_vars", SectionKind::ThreadLocalVariables))
        } else if seg_lower == "__data" && sect_lower == "__thread_bss" {
            Ok(("__DATA", "__thread_bss", SectionKind::ThreadLocalZeroFill))
        } else if seg_lower == "__data" && sect_lower == "__bss" {
            Ok(("__DATA", "__bss", SectionKind::ZeroFill))
        } else {
            Err(AsmError(format!(
                "unsupported section {},{} (supported sections: __TEXT,__text, __TEXT,__cstring, __TEXT,__literal16, __TEXT,__const, __DATA,__data, __DATA,__const, __DATA,__thread_data, __DATA,__thread_vars, __DATA,__thread_bss, __DATA,__bss)",
                seg, sect
            )))
        }
    }

    fn ensure_same_section(
        &self,
        current_section: usize,
        target_section: usize,
        symbol: &str,
    ) -> Result<(), AsmError> {
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
            Inst::BCond { cond, .. } => Ok(Inst::BCond {
                cond: *cond,
                offset: checked,
            }),
            Inst::Cbz { rt, sf, .. } => Ok(Inst::Cbz {
                rt: *rt,
                offset: checked,
                sf: *sf,
            }),
            Inst::Cbnz { rt, sf, .. } => Ok(Inst::Cbnz {
                rt: *rt,
                offset: checked,
                sf: *sf,
            }),
            Inst::Tbz { rt, bit, sf, .. } => Ok(Inst::Tbz {
                rt: *rt,
                bit: *bit,
                offset: checked,
                sf: *sf,
            }),
            Inst::Tbnz { rt, bit, sf, .. } => Ok(Inst::Tbnz {
                rt: *rt,
                bit: *bit,
                offset: checked,
                sf: *sf,
            }),
            _ => Err(AsmError(
                "internal error: invalid branch fixup instruction".into(),
            )),
        }
    }

    fn resolve_literal_inst(&self, inst: &Inst, offset: i64) -> Result<Inst, AsmError> {
        let checked = check_branch_offset(offset, 19)?;
        match inst {
            Inst::LdrLit64 { rt, .. } => Ok(Inst::LdrLit64 {
                rt: *rt,
                offset: checked,
            }),
            Inst::LdrLit32 { rt, .. } => Ok(Inst::LdrLit32 {
                rt: *rt,
                offset: checked,
            }),
            Inst::LdrswLit { rt, .. } => Ok(Inst::LdrswLit {
                rt: *rt,
                offset: checked,
            }),
            Inst::LdrFpLit64 { rt, .. } => Ok(Inst::LdrFpLit64 {
                rt: *rt,
                offset: checked,
            }),
            Inst::LdrFpLit32 { rt, .. } => Ok(Inst::LdrFpLit32 {
                rt: *rt,
                offset: checked,
            }),
            Inst::LdrFpLit128 { rt, .. } => Ok(Inst::LdrFpLit128 {
                rt: *rt,
                offset: checked,
            }),
            _ => Err(AsmError(
                "internal error: invalid literal fixup instruction".into(),
            )),
        }
    }

    fn resolve_adr_inst(&self, inst: &Inst, offset: i64) -> Result<Inst, AsmError> {
        let checked = check_pcrel_offset(offset, 21, "adr offset")?;
        match inst {
            Inst::Adr { rd, .. } => Ok(Inst::Adr {
                rd: *rd,
                imm: checked,
            }),
            _ => Err(AsmError(
                "internal error: invalid adr fixup instruction".into(),
            )),
        }
    }

    fn section_base_addresses(&self) -> Result<Vec<u64>, AsmError> {
        Self::section_base_addresses_for(&self.sections)
    }

    fn section_base_addresses_for(sections: &[Section]) -> Result<Vec<u64>, AsmError> {
        let mut bases = vec![0u64; sections.len()];
        let mut addr = 0u64;
        for index in section_allocation_order(sections) {
            let section = &sections[index];
            addr = checked_align_value(addr, section.align_pow2)
                .ok_or_else(|| AsmError("section layout alignment overflows u64".into()))?;
            bases[index] = addr;
            addr = addr
                .checked_add(section.size)
                .ok_or_else(|| AsmError("section layout size overflows u64".into()))?;
        }
        Ok(bases)
    }

    fn symbol_values_for_expr(&self) -> BTreeMap<String, SymbolValue> {
        let mut values = self.label_values_for_expr();

        for (name, value) in &self.absolute_symbols {
            values.insert(name.clone(), SymbolValue::Absolute(*value));
        }

        values
    }

    fn label_values_for_expr(&self) -> BTreeMap<String, SymbolValue> {
        let mut values = BTreeMap::new();

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

    fn activate_absolute_definition(&mut self, name: &str) -> Result<(), AsmError> {
        let (expected_name, _) = self
            .absolute_assignments
            .get(self.next_absolute_assignment)
            .ok_or_else(|| AsmError("missing resolved absolute assignment".into()))?;
        if expected_name != name {
            return Err(AsmError(format!(
                "absolute assignment order changed between passes: expected '{}', got '{}'",
                expected_name, name
            )));
        }
        let value = *self
            .absolute_assignment_values
            .get(self.next_absolute_assignment)
            .ok_or_else(|| AsmError("missing resolved absolute assignment value".into()))?;
        self.next_absolute_assignment += 1;
        self.absolute_symbols.insert(name.to_string(), value);
        Ok(())
    }

    fn require_sized_absolute_expr(
        &self,
        expr: &Expr,
        context: &str,
        bits: u8,
    ) -> Result<[u8; 8], AsmError> {
        match self.classify_expr(expr)? {
            ClassifiedExpr::Absolute(value) => checked_signed_data_bytes(value, context, bits),
            ClassifiedExpr::UnsignedAbsolute(value) => {
                checked_unsigned_data_bytes(value, context, bits)
            }
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
            0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x7a, 0x52, 0x00, 0x01, 0x78,
            0x1e, 0x01, 0x10, 0x0c, 0x1f, 0x00,
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
            section
                .data
                .extend_from_slice(&row.start_offset.to_le_bytes());
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

    fn unwind_layout_sections(&self) -> Result<Vec<Section>, AsmError> {
        let mut sections = Vec::new();

        if !self.compact_unwind_rows.is_empty() {
            let row_count = u64::try_from(self.compact_unwind_rows.len())
                .map_err(|_| AsmError("compact unwind row count exceeds u64".into()))?;
            let mut section = Section::new("__LD", "__compact_unwind", SectionKind::CompactUnwind);
            section.align_pow2 = 3;
            section.size = row_count
                .checked_mul(COMPACT_UNWIND_ENTRY_SIZE)
                .ok_or_else(|| AsmError("compact unwind section size overflows u64".into()))?;
            sections.push(section);
        }

        if !self.eh_frame_rows.is_empty() {
            let mut size = EH_FRAME_CIE_SIZE;
            for row in &self.eh_frame_rows {
                let fde_size = EH_FRAME_FDE_FIXED_SIZE
                    .checked_add(row.instructions.len())
                    .ok_or_else(|| AsmError("eh_frame FDE size overflows usize".into()))?;
                u32::try_from(fde_size - 4)
                    .map_err(|_| AsmError("eh_frame FDE exceeds u32".into()))?;
                size = size
                    .checked_add(
                        u64::try_from(fde_size)
                            .map_err(|_| AsmError("eh_frame FDE size exceeds u64".into()))?,
                    )
                    .ok_or_else(|| AsmError("eh_frame section size overflows u64".into()))?;
            }
            let mut section = Section::new("__TEXT", "__eh_frame", SectionKind::EhFrame);
            section.align_pow2 = 3;
            section.size = size;
            sections.push(section);
        }

        Ok(sections)
    }

    fn finish(mut self) -> Result<ObjectFile, AsmError> {
        if self.active_cfi_proc.is_some() {
            return Err(AsmError(
                "unterminated .cfi_startproc before end of file".into(),
            ));
        }

        self.materialize_compact_unwind_section();
        self.materialize_eh_frame_section()?;

        let section_bases = self.section_base_addresses()?;
        let section_layout: Vec<_> = self
            .sections
            .iter()
            .map(SectionLayoutFingerprint::from)
            .collect();
        if section_bases != self.section_bases || section_layout != self.expected_section_layout {
            return Err(AsmError(
                "section layout changed between assembly passes".into(),
            ));
        }
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

        let symbol_order = self.symbol_order.clone();
        let section_base_reloc_anchor_order: BTreeMap<_, _> = self
            .pending_relocs
            .iter()
            .enumerate()
            .flat_map(|(section, relocs)| relocs.iter().map(move |reloc| (section, reloc)))
            .filter_map(|(section, reloc)| {
                let PendingRelocTarget::Symbol(symbol) = &reloc.target else {
                    return None;
                };
                let (target_section, offset) = self.labels.get(symbol)?;
                if self.sections[*target_section].kind == SectionKind::Literal16 {
                    return None;
                }
                if *offset != 0 {
                    return None;
                }

                let owner_name = self
                    .labels
                    .iter()
                    .filter(|(_, (label_section, label_offset))| {
                        *label_section == section && *label_offset <= reloc.offset as u64
                    })
                    .max_by(|(a_name, (_, a_offset)), (b_name, (_, b_offset))| {
                        a_offset
                            .cmp(b_offset)
                            .then_with(|| {
                                (!is_assembler_local_symbol(a_name))
                                    .cmp(&!is_assembler_local_symbol(b_name))
                            })
                            .then_with(|| {
                                symbol_order
                                    .get(*a_name)
                                    .copied()
                                    .unwrap_or(usize::MAX)
                                    .cmp(&symbol_order.get(*b_name).copied().unwrap_or(usize::MAX))
                            })
                    })
                    .map(|(name, _)| name.clone())?;

                if self
                    .symbol_attrs
                    .get(&owner_name)
                    .is_some_and(|attrs| attrs.global)
                {
                    return None;
                }

                let owner = symbol_order.get(&owner_name).copied()?;

                Some((symbol.clone(), owner))
            })
            .fold(
                BTreeMap::<String, usize>::new(),
                |mut acc, (symbol, owner)| {
                    acc.entry(symbol)
                        .and_modify(|existing| *existing = (*existing).min(owner))
                        .or_insert(owner);
                    acc
                },
            );
        let page_reloc_targets: BTreeSet<_> = self
            .pending_relocs
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
        let mut page_target_base_use_order = BTreeMap::new();
        let mut next_page_target_base_order = 0usize;
        for relocs in &self.pending_relocs {
            for reloc in relocs {
                if reloc.reloc_type != macho::ARM64_RELOC_PAGE21
                    && reloc.reloc_type != macho::ARM64_RELOC_PAGEOFF12
                {
                    continue;
                }
                let PendingRelocTarget::Symbol(symbol) = &reloc.target else {
                    continue;
                };
                let Some((_target_section, _offset)) = self.labels.get(symbol) else {
                    continue;
                };
                if symbol.starts_with("ltmp")
                    || is_hidden_local_object_symbol(symbol)
                    || is_compiler_local_pool_symbol(symbol)
                {
                    continue;
                }
                page_target_base_use_order
                    .entry(symbol.clone())
                    .or_insert_with(|| {
                        let order = next_page_target_base_order;
                        next_page_target_base_order += 1;
                        order
                    });
            }
        }
        let section_page_target_at_base: BTreeSet<_> = self
            .sections
            .iter()
            .enumerate()
            .filter_map(|(section, _)| {
                self.labels
                    .iter()
                    .any(|(name, (label_section, offset))| {
                        *label_section == section
                            && *offset == 0
                            && page_reloc_targets.contains(name)
                            && !name.starts_with("ltmp")
                            && !is_hidden_local_object_symbol(name)
                    })
                    .then_some(section)
            })
            .collect();
        let section_temp_trailing_order: BTreeMap<usize, usize> = self
            .sections
            .iter()
            .enumerate()
            .filter_map(|(section, _)| {
                if !section_page_target_at_base.contains(&section)
                    || !matches!(
                        self.sections[section].kind,
                        SectionKind::CStringLiterals
                            | SectionKind::ConstData
                            | SectionKind::Data
                            | SectionKind::ZeroFill
                    )
                {
                    return None;
                }
                let anchor = self
                    .labels
                    .iter()
                    .filter(|(name, (label_section, _offset))| {
                        *label_section == section
                            && page_reloc_targets.contains(*name)
                            && !name.starts_with("ltmp")
                            && !is_hidden_local_object_symbol(name)
                            && !is_compiler_local_pool_symbol(name)
                    })
                    .filter_map(|(name, _)| symbol_order.get(name).copied())
                    .max()?;
                Some((section, anchor))
            })
            .collect();
        symbols.sort_by(|a, b| {
            let rank_a = symbol_class_rank(a);
            let rank_b = symbol_class_rank(b);
            rank_a.cmp(&rank_b).then_with(|| match rank_a {
                0 => {
                    let a_is_primary_text_temp = a.name == "ltmp0";
                    let b_is_primary_text_temp = b.name == "ltmp0";
                    let a_anchor = section_base_reloc_anchor_order
                        .get(&a.name)
                        .copied()
                        .unwrap_or(usize::MAX);
                    let b_anchor = section_base_reloc_anchor_order
                        .get(&b.name)
                        .copied()
                        .unwrap_or(usize::MAX);
                    let a_base_order = symbol_order.get(&a.name).copied().unwrap_or(usize::MAX);
                    let b_base_order = symbol_order.get(&b.name).copied().unwrap_or(usize::MAX);
                    let a_order = if let Some(section) = a
                        .name
                        .strip_prefix("ltmp")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        section_temp_trailing_order
                            .get(&section)
                            .copied()
                            .unwrap_or(a_base_order)
                    } else {
                        a_base_order.min(a_anchor)
                    };
                    let b_order = if let Some(section) = b
                        .name
                        .strip_prefix("ltmp")
                        .and_then(|s| s.parse::<usize>().ok())
                    {
                        section_temp_trailing_order
                            .get(&section)
                            .copied()
                            .unwrap_or(b_base_order)
                    } else {
                        b_base_order.min(b_anchor)
                    };
                    a_is_primary_text_temp
                        .cmp(&b_is_primary_text_temp)
                        .reverse()
                        .then_with(|| {
                            let a_is_section_temp = a.name.starts_with("ltmp");
                            let b_is_section_temp = b.name.starts_with("ltmp");
                            let same_section_index = (a.section == b.section && a.section > 0)
                                .then(|| (a.section - 1) as usize);
                            let a_section_kind = (a.section > 0)
                                .then(|| &self.sections[(a.section - 1) as usize].kind);
                            let a_non_temp_text_local = !a_is_section_temp
                                && a.section > 0
                                && self.sections[(a.section - 1) as usize].kind
                                    == SectionKind::Text;
                            let b_non_temp_text_local = !b_is_section_temp
                                && b.section > 0
                                && self.sections[(b.section - 1) as usize].kind
                                    == SectionKind::Text;
                            let a_page_target_base = !a_is_section_temp
                                && page_target_base_use_order.contains_key(&a.name);
                            let b_page_target_base = !b_is_section_temp
                                && page_target_base_use_order.contains_key(&b.name);
                            let a_local_bucket = if a_non_temp_text_local {
                                0u8
                            } else if a_page_target_base {
                                1
                            } else {
                                2
                            };
                            let b_local_bucket = if b_non_temp_text_local {
                                0u8
                            } else if b_page_target_base {
                                1
                            } else {
                                2
                            };
                            let bucket_cmp = a_local_bucket.cmp(&b_local_bucket);
                            if bucket_cmp != std::cmp::Ordering::Equal {
                                return bucket_cmp;
                            }
                            if a_local_bucket == 1 {
                                let page_target_cmp = page_target_base_use_order[&a.name]
                                    .cmp(&page_target_base_use_order[&b.name]);
                                if page_target_cmp != std::cmp::Ordering::Equal {
                                    return page_target_cmp;
                                }
                            }
                            if a.section == b.section
                                && a.value == b.value
                                && a.section > 0
                                && a_is_section_temp != b_is_section_temp
                            {
                                let temp_cmp = match a_section_kind {
                                    Some(
                                        SectionKind::Literal16
                                        | SectionKind::ThreadLocalData
                                        | SectionKind::ThreadLocalZeroFill,
                                    ) => b_is_section_temp.cmp(&a_is_section_temp),
                                    _ if same_section_index.is_some_and(|section| {
                                        section_temp_trailing_order.contains_key(&section)
                                    }) =>
                                    {
                                        a_is_section_temp.cmp(&b_is_section_temp)
                                    }
                                    _ => std::cmp::Ordering::Equal,
                                };
                                if temp_cmp != std::cmp::Ordering::Equal {
                                    return temp_cmp;
                                }
                            }
                            let a_is_trailing_temp = a_is_section_temp
                                && a.section > 0
                                && section_temp_trailing_order
                                    .contains_key(&((a.section - 1) as usize));
                            let b_is_trailing_temp = b_is_section_temp
                                && b.section > 0
                                && section_temp_trailing_order
                                    .contains_key(&((b.section - 1) as usize));

                            a_order.cmp(&b_order).then_with(|| {
                                let trailing_temp_cmp = a_is_trailing_temp.cmp(&b_is_trailing_temp);
                                if trailing_temp_cmp != std::cmp::Ordering::Equal {
                                    return trailing_temp_cmp;
                                }
                                symbol_order
                                    .get(&a.name)
                                    .copied()
                                    .unwrap_or(usize::MAX)
                                    .cmp(&symbol_order.get(&b.name).copied().unwrap_or(usize::MAX))
                                    .then_with(|| a.name.cmp(&b.name))
                            })
                        })
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
                        .ok_or_else(|| {
                            AsmError(format!("missing relocation symbol '{}'", symbol))
                        })? as u32,
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
                    AsmError(format!(
                        ".loh {} references undefined label '{}'",
                        hint.kind, label
                    ))
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

fn checked_align_value(value: u64, power: u32) -> Option<u64> {
    let alignment = 1u64.checked_shl(power)?;
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
}

fn checked_signed_data_bytes(value: i64, context: &str, bits: u8) -> Result<[u8; 8], AsmError> {
    if !parse::signed_data_value_fits(value, bits) {
        return Err(AsmError(format!(
            "{} value {} is out of range for {}-bit data",
            context, value, bits
        )));
    }
    Ok(value.to_le_bytes())
}

fn checked_unsigned_data_bytes(value: u64, context: &str, bits: u8) -> Result<[u8; 8], AsmError> {
    if !parse::unsigned_data_value_fits(value, bits) {
        return Err(AsmError(format!(
            "{} value {} is out of range for {}-bit data",
            context, value, bits
        )));
    }
    Ok(value.to_le_bytes())
}

fn section_allocation_order(sections: &[Section]) -> Vec<usize> {
    let mut order: Vec<_> = (0..sections.len()).collect();
    order.sort_by_key(|&index| sections[index].kind.is_zerofill());
    order
}

fn check_branch_offset(offset: i64, bits: u8) -> Result<i32, AsmError> {
    if offset % 4 != 0 {
        return Err(AsmError(format!(
            "branch offset {} is not 4-byte aligned",
            offset
        )));
    }

    let scaled = offset / 4;
    let min = -(1i64 << (bits - 1));
    let max = (1i64 << (bits - 1)) - 1;
    if scaled < min || scaled > max {
        return Err(AsmError(format!(
            "branch offset {} is out of range for {}-bit immediate",
            offset, bits
        )));
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
        "AdrpLdr" => Some(2),
        "AdrpLdrGotLdr" => Some(4),
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

fn is_compiler_local_pool_symbol(name: &str) -> bool {
    name.starts_with("lCPI")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macho::write_macho;

    fn parse_symtab_info(data: &[u8]) -> (usize, usize, usize, usize) {
        let ncmds = u32::from_le_bytes(data[16..20].try_into().expect("ncmds")) as usize;
        let mut offset = 32usize;
        for _ in 0..ncmds {
            let cmd = u32::from_le_bytes(data[offset..offset + 4].try_into().expect("cmd"));
            let cmdsize =
                u32::from_le_bytes(data[offset + 4..offset + 8].try_into().expect("cmdsize"))
                    as usize;
            if cmd == 0x2 {
                let symoff =
                    u32::from_le_bytes(data[offset + 8..offset + 12].try_into().expect("symoff"))
                        as usize;
                let nsyms =
                    u32::from_le_bytes(data[offset + 12..offset + 16].try_into().expect("nsyms"))
                        as usize;
                let stroff =
                    u32::from_le_bytes(data[offset + 16..offset + 20].try_into().expect("stroff"))
                        as usize;
                let strsize =
                    u32::from_le_bytes(data[offset + 20..offset + 24].try_into().expect("strsize"))
                        as usize;
                return (symoff, nsyms, stroff, strsize);
            }
            offset += cmdsize;
        }
        panic!("missing symtab");
    }

    fn symbol_name_at(data: &[u8], stroff: usize, strx: u32) -> String {
        let start = stroff + strx as usize;
        let end = data[start..]
            .iter()
            .position(|&b| b == 0)
            .map(|len| start + len)
            .expect("nul terminator");
        String::from_utf8(data[start..end].to_vec()).expect("utf8 symbol name")
    }

    fn file_symbol_names(data: &[u8]) -> Vec<String> {
        let (symoff, nsyms, stroff, _) = parse_symtab_info(data);
        let mut names = Vec::with_capacity(nsyms);
        for index in 0..nsyms {
            let base = symoff + index * 16;
            let strx = u32::from_le_bytes(data[base..base + 4].try_into().expect("strx"));
            names.push(symbol_name_at(data, stroff, strx));
        }
        names
    }
    use crate::reg::*;

    fn text_bytes(obj: &ObjectFile) -> &[u8] {
        &obj.text_section().data
    }

    fn data_bytes(obj: &ObjectFile) -> &[u8] {
        &obj.section("__DATA", "__data")
            .expect("missing __DATA,__data")
            .data
    }

    fn text_relocs(obj: &ObjectFile) -> &[Relocation] {
        &obj.text_section().relocations
    }

    fn data_relocs(obj: &ObjectFile) -> &[Relocation] {
        &obj.section("__DATA", "__data")
            .expect("missing __DATA,__data")
            .relocations
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
    fn assemble_register_shifts() {
        let obj = assemble_source(
            ".text\n\
             lsl x0, x1, x2\n\
             lsr x0, x1, x2\n\
             asr x0, x1, x2\n\
             lsl w3, w4, w5\n\
             lsr w3, w4, w5\n\
             asr w3, w4, w5\n",
        )
        .unwrap();
        assert_eq!(
            text_bytes(&obj),
            [
                0x20, 0x20, 0xC2, 0x9A, 0x20, 0x24, 0xC2, 0x9A, 0x20, 0x28, 0xC2, 0x9A, 0x83, 0x20,
                0xC5, 0x1A, 0x83, 0x24, 0xC5, 0x1A, 0x83, 0x28, 0xC5, 0x1A,
            ]
        );
    }

    #[test]
    fn assemble_immediate_shift_boundaries() {
        let obj = assemble_source(
            ".text\n\
             lsl w0, w1, #0\n\
             lsl w0, w1, #31\n\
             lsl x0, x1, #0\n\
             lsl x0, x1, #63\n\
             lsr w0, w1, #0\n\
             lsr w0, w1, #31\n\
             lsr x0, x1, #0\n\
             lsr x0, x1, #63\n\
             asr w0, w1, #0\n\
             asr w0, w1, #31\n\
             asr x0, x1, #0\n\
             asr x0, x1, #63\n",
        )
        .unwrap();
        let expected: Vec<u8> = [
            0x53007C20u32,
            0x53010020,
            0xD340FC20,
            0xD3410020,
            0x53007C20,
            0x531F7C20,
            0xD340FC20,
            0xD37FFC20,
            0x13007C20,
            0x131F7C20,
            0x9340FC20,
            0x937FFC20,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        assert_eq!(text_bytes(&obj), expected);
    }

    #[test]
    fn reject_invalid_immediate_shifts_before_encoding() {
        for mnemonic in ["lsl", "lsr", "asr"] {
            for operands in [
                "w0, w1, #-1",
                "w0, w1, #32",
                "x0, x1, #64",
                "x0, x1, #255",
                "x0, x1, #256",
                "sp, x1, #0",
                "x0, sp, #0",
                "w0, x1, #0",
                "x0, w1, #0",
            ] {
                let source = format!(".text\n{mnemonic} {operands}\n");
                assert!(assemble_source(&source).is_err(), "accepted {source:?}");
            }
        }
    }

    #[test]
    fn assemble_compiler_emitted_extensions_and_fcvt() {
        let obj = assemble_source(
            ".text\n\
             sxtw x0, w1\n\
             sxtb w2, w3\n\
             sxtb x4, w5\n\
             sxth w2, w3\n\
             sxth x2, w3\n\
             uxtw x6, w7\n\
             fcvt d8, s9\n\
             fcvt s10, d11\n\
             .data\n\
             .quad 0xbff0000000000000\n",
        )
        .unwrap();
        assert_eq!(
            text_bytes(&obj),
            [
                0x20, 0x7C, 0x40, 0x93, 0x62, 0x1C, 0x00, 0x13, 0xA4, 0x1C, 0x40, 0x93, 0x62, 0x3C,
                0x00, 0x13, 0x62, 0x3C, 0x40, 0x93, 0xE6, 0x7C, 0x40, 0xD3, 0x28, 0xC1, 0x22, 0x1E,
                0x6A, 0x41, 0x62, 0x1E,
            ]
        );
        assert_eq!(data_bytes(&obj), 0xBFF0000000000000u64.to_le_bytes());
    }

    #[test]
    fn assemble_unsigned_quad_bit_patterns_in_context() {
        let obj = assemble_source(
            ".data\n\
             bits: .quad 0x8000000000000000, 0xffffffffffffffff // hexadecimal\n\
             decimal: .quad 18446744073709551615 ; u64 max\n",
        )
        .unwrap();
        let expected = [
            0x8000000000000000u64.to_le_bytes(),
            u64::MAX.to_le_bytes(),
            u64::MAX.to_le_bytes(),
        ]
        .concat();
        assert_eq!(data_bytes(&obj), expected);
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
    fn assemble_sp_register_arithmetic_preserves_sp() {
        let obj = assemble_source(
            ".text\n\
             add x0, sp, x1\n\
             add sp, x1, x2\n\
             sub x3, sp, x4\n\
             sub sp, x5, x6\n\
             adds x7, sp, x8\n\
             subs x9, sp, x10\n\
             cmp sp, x11\n\
             cmn sp, x12\n\
             add x13, sp, x14, lsl #4\n\
             sub sp, x15, x16, lsl #2\n\
             add w0, wsp, w1\n\
             add wsp, w1, w2\n\
             sub w3, wsp, w4\n\
             sub wsp, w5, w6\n\
             adds w7, wsp, w8\n\
             subs w9, wsp, w10\n\
             cmp wsp, w11\n\
             cmn wsp, w12\n\
             add w13, wsp, w14, lsl #4\n\
             sub wsp, w15, w16, lsl #2\n\
             add w17, wsp, w18, uxtw #3\n\
             sub wsp, w19, w20, sxtw #4\n\
             sub sp, sp, x16\n\
             add sp, sp, x16\n\
             sub wsp, wsp, w16\n\
             add wsp, wsp, w16\n",
        )
        .unwrap();
        assert_eq!(
            text_bytes(&obj),
            [
                0xE0, 0x63, 0x21, 0x8B, 0x3F, 0x60, 0x22, 0x8B, 0xE3, 0x63, 0x24, 0xCB, 0xBF, 0x60,
                0x26, 0xCB, 0xE7, 0x63, 0x28, 0xAB, 0xE9, 0x63, 0x2A, 0xEB, 0xFF, 0x63, 0x2B, 0xEB,
                0xFF, 0x63, 0x2C, 0xAB, 0xED, 0x73, 0x2E, 0x8B, 0xFF, 0x69, 0x30, 0xCB, 0xE0, 0x43,
                0x21, 0x0B, 0x3F, 0x40, 0x22, 0x0B, 0xE3, 0x43, 0x24, 0x4B, 0xBF, 0x40, 0x26, 0x4B,
                0xE7, 0x43, 0x28, 0x2B, 0xE9, 0x43, 0x2A, 0x6B, 0xFF, 0x43, 0x2B, 0x6B, 0xFF, 0x43,
                0x2C, 0x2B, 0xED, 0x53, 0x2E, 0x0B, 0xFF, 0x49, 0x30, 0x4B, 0xF1, 0x4F, 0x32, 0x0B,
                0x7F, 0xD2, 0x34, 0x4B, 0xFF, 0x63, 0x30, 0xCB, 0xFF, 0x63, 0x30, 0x8B, 0xFF, 0x43,
                0x30, 0x4B, 0xFF, 0x43, 0x30, 0x0B,
            ]
        );
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
            bl _puts\n",
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
        let obj =
            assemble_source(".text\n.global _start\n_start:\nnop\nfoo:\nadd x0, x1, x2\nret\n")
                .unwrap();
        let foo = obj.symbols.iter().find(|s| s.name == "foo").unwrap();
        assert_eq!(foo.value, 4); // after the nop
        assert!(!foo.global);
    }

    #[test]
    fn assemble_instructions_api() {
        let insts = vec![Inst::Nop, Inst::Ret { rn: X30 }];
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
        assert_eq!(
            obj.symbols
                .iter()
                .find(|sym| sym.name == "msg")
                .unwrap()
                .value,
            0
        );
    }

    #[test]
    fn assemble_extern_declaration_does_not_emit_symbol_by_itself() {
        let obj = assemble_source(".extern _puts\n.text\nret\n").unwrap();
        assert!(!obj.symbols.iter().any(|sym| sym.name == "_puts"));
    }

    #[test]
    fn assemble_comm_emits_common_symbol() {
        let obj = assemble_source(".comm _common, 24, 3\n.text\nret\n").unwrap();
        let common = obj
            .symbols
            .iter()
            .find(|sym| sym.name == "_common")
            .unwrap();
        assert!(common.global);
        assert!(common.undefined);
        assert!(common.common);
        assert_eq!(common.common_align_pow2, 3);
        assert_eq!(common.value, 24);
        assert_eq!(
            text_bytes(&obj),
            Inst::Ret { rn: X30 }.encode().to_le_bytes()
        );
    }

    #[test]
    fn assemble_zerofill_reserves_bss_without_switching_sections() {
        let obj =
            assemble_source(".text\nret\n.zerofill __DATA,__bss,_scratch,16,4\nret\n").unwrap();
        let bss = obj.section("__DATA", "__bss").unwrap();
        let scratch = obj
            .symbols
            .iter()
            .find(|sym| sym.name == "_scratch")
            .unwrap();

        assert_eq!(
            text_bytes(&obj),
            [
                Inst::Ret { rn: X30 }.encode().to_le_bytes(),
                Inst::Ret { rn: X30 }.encode().to_le_bytes(),
            ]
            .concat()
        );
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
        let obj =
            assemble_source(".text\nret\n.zerofill __DATA,__bss,_scratch,16,4\n.data\n.byte 1\n")
                .unwrap();

        let section_names: Vec<_> = obj
            .sections
            .iter()
            .map(|section| (section.segment.as_str(), section.name.as_str()))
            .collect();
        assert_eq!(
            section_names,
            vec![
                ("__TEXT", "__text"),
                ("__DATA", "__bss"),
                ("__DATA", "__data")
            ]
        );

        let scratch = obj
            .symbols
            .iter()
            .find(|sym| sym.name == "_scratch")
            .unwrap();
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
        assert!(
            err.msg
                .contains("unsupported directive '.unknown_directive'"),
            "got: {}",
            err
        );
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
            .cfi_endproc\n",
        )
        .unwrap();
        let compact = compact_unwind_section(&obj);
        assert_eq!(compact.align_pow2, 3);
        assert_eq!(compact.data.len(), 32);
        assert_eq!(
            compact.data,
            [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x10,
                0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
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
    fn assemble_frameless_saved_pair_cfi_emits_compact_unwind() {
        let obj = assemble_source(
            ".text\n\
            frameless_pair_target:\n\
            .cfi_startproc\n\
            sub sp, sp, #32\n\
            .cfi_def_cfa_offset 32\n\
            .cfi_offset w27, -8\n\
            .cfi_offset w28, -16\n\
            add sp, sp, #32\n\
            ret\n\
            .cfi_endproc\n",
        )
        .unwrap();
        let compact = compact_unwind_section(&obj);
        assert_eq!(
            u32::from_le_bytes(compact.data[12..16].try_into().unwrap()),
            UNWIND_ARM64_MODE_FRAMELESS | (2 << 12) | UNWIND_ARM64_FRAME_X27_X28_PAIR
        );
        assert!(obj.section("__TEXT", "__eh_frame").is_none());
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
            .cfi_endproc\n",
        )
        .unwrap();
        let compact = compact_unwind_section(&obj);
        assert_eq!(
            compact.data,
            [
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x03, 0x00,
                0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ]
        );
    }

    #[test]
    fn assemble_cfi_endproc_requires_active_proc() {
        let err = assemble_source(".cfi_endproc\n").unwrap_err();
        assert!(
            err.msg
                .contains(".cfi_endproc requires an active .cfi_startproc"),
            "got: {}",
            err
        );
    }

    #[test]
    fn assemble_cfi_directive_requires_active_proc() {
        let err = assemble_source(".cfi_def_cfa_offset 16\n").unwrap_err();
        assert!(
            err.msg
                .contains("CFI directives require an active .cfi_startproc"),
            "got: {}",
            err
        );
    }

    #[test]
    fn assemble_unterminated_cfi_proc_is_rejected() {
        let err =
            assemble_source(".text\nunterminated_target:\n.cfi_startproc\nret\n").unwrap_err();
        assert!(
            err.msg.contains("unterminated .cfi_startproc"),
            "got: {}",
            err
        );
    }

    #[test]
    fn assemble_cfi_restore_falls_back_to_eh_frame() {
        let obj = assemble_source(
            ".text\n\
            restore_target:\n\
            .cfi_startproc\n\
            ret\n\
            .cfi_restore w29\n\
            .cfi_endproc\n",
        )
        .unwrap();

        let compact = compact_unwind_section(&obj);
        let eh_frame = eh_frame_section(&obj);
        assert_eq!(
            u32::from_le_bytes(compact.data[12..16].try_into().unwrap()),
            UNWIND_ARM64_MODE_DWARF
        );
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
            .cfi_endproc\n",
        )
        .unwrap();

        let compact = compact_unwind_section(&obj);
        let eh_frame = eh_frame_section(&obj);
        assert_eq!(
            u32::from_le_bytes(compact.data[12..16].try_into().unwrap()),
            UNWIND_ARM64_MODE_DWARF
        );
        assert!(eh_frame.relocations.len() >= 2);
    }

    #[test]
    fn assemble_subsections_via_symbols_sets_object_flag() {
        let obj = assemble_source(".text\nret\n.subsections_via_symbols\n").unwrap();
        assert_eq!(obj.flags, macho::MH_SUBSECTIONS_VIA_SYMBOLS);
    }

    #[test]
    fn assemble_build_version_sets_object_metadata() {
        let obj =
            assemble_source(".text\nret\n.build_version macos, 11, 0 sdk_version 15, 5\n").unwrap();

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
            ".section __TEXT,__cstring\n\
             msg: .asciz \"hello\"\n\
             .section __TEXT,__literal16\n\
             .p2align 4\n\
             lit:\n\
             .byte 1\n\
             .space 15\n\
             .section __TEXT,__const\n\
             value: .quad 42\n",
        )
        .unwrap();

        let cstring = obj.section("__TEXT", "__cstring").unwrap();
        let literal16 = obj.section("__TEXT", "__literal16").unwrap();
        let const_data = obj.section("__TEXT", "__const").unwrap();
        assert_eq!(cstring.data, b"hello\0");
        assert_eq!(literal16.kind, SectionKind::Literal16);
        assert_eq!(literal16.align_pow2, 4);
        assert_eq!(literal16.data.len(), 16);
        assert_eq!(const_data.data, 42u64.to_le_bytes());
        assert_eq!(
            obj.symbols
                .iter()
                .find(|sym| sym.name == "msg")
                .unwrap()
                .value,
            0
        );
        assert_eq!(
            obj.symbols
                .iter()
                .find(|sym| sym.name == "lit")
                .unwrap()
                .value,
            16
        );
        assert_eq!(
            obj.symbols
                .iter()
                .find(|sym| sym.name == "value")
                .unwrap()
                .value,
            32
        );
    }

    #[test]
    fn assemble_supported_data_const_section() {
        let obj = assemble_source(
            ".section __DATA,__const\n\
             .p2align 3\n\
             value: .quad 42\n",
        )
        .unwrap();

        let const_data = obj.section("__DATA", "__const").unwrap();
        assert_eq!(const_data.kind, SectionKind::ConstData);
        assert_eq!(const_data.align_pow2, 3);
        assert_eq!(const_data.data, 42u64.to_le_bytes());
        assert_eq!(
            obj.symbols
                .iter()
                .find(|sym| sym.name == "value")
                .unwrap()
                .value,
            0
        );
    }

    #[test]
    fn assemble_supported_thread_local_sections() {
        let obj = assemble_source(
            ".section __DATA,__thread_data\n\
             .p2align 2\n\
             _tls_value$tlv$init:\n\
             .long 5\n\
             .section __DATA,__thread_vars\n\
             .globl _tls_value\n\
             _tls_value:\n\
             .quad __tlv_bootstrap\n\
             .quad 0\n\
             .quad _tls_value$tlv$init\n",
        )
        .unwrap();

        let thread_data = obj.section("__DATA", "__thread_data").unwrap();
        let thread_vars = obj.section("__DATA", "__thread_vars").unwrap();
        assert_eq!(thread_data.data, 5u32.to_le_bytes());
        assert_eq!(thread_data.align_pow2, 2);
        assert_eq!(thread_vars.data.len(), 24);
        assert_eq!(thread_vars.relocations.len(), 2);
        assert_eq!(
            obj.symbols[thread_vars.relocations[0].symbol_idx as usize].name,
            "__tlv_bootstrap"
        );
        assert_eq!(
            obj.symbols[thread_vars.relocations[1].symbol_idx as usize].name,
            "_tls_value$tlv$init"
        );
    }

    #[test]
    fn assemble_tbss_thread_local_zerofill() {
        let obj = assemble_source(
            ".tbss _tls_counter$tlv$init, 4, 2\n\
             .section __DATA,__thread_vars\n\
             .globl _tls_counter\n\
             _tls_counter:\n\
             .quad __tlv_bootstrap\n\
             .quad 0\n\
             .quad _tls_counter$tlv$init\n",
        )
        .unwrap();

        let thread_bss = obj.section("__DATA", "__thread_bss").unwrap();
        let thread_vars = obj.section("__DATA", "__thread_vars").unwrap();
        assert!(thread_bss.data.is_empty());
        assert_eq!(thread_bss.size, 4);
        assert_eq!(thread_bss.align_pow2, 2);
        assert_eq!(thread_vars.relocations.len(), 2);
        assert_eq!(
            obj.symbols[thread_vars.relocations[1].symbol_idx as usize].name,
            "_tls_counter$tlv$init"
        );
        assert_eq!(
            obj.symbols[thread_vars.relocations[1].symbol_idx as usize].name,
            "_tls_counter$tlv$init"
        );
        let thread_bss_index = obj
            .sections
            .iter()
            .position(|section| section.segment == "__DATA" && section.name == "__thread_bss")
            .unwrap();
        let tls_init = obj
            .symbols
            .iter()
            .find(|sym| sym.name == "_tls_counter$tlv$init")
            .unwrap();
        assert_eq!(tls_init.section, (thread_bss_index + 1) as u8);
        assert!(!tls_init.undefined);
    }

    #[test]
    fn assemble_bss_is_zero_fill() {
        let obj =
            assemble_source(".section __DATA,__bss\n.p2align 4\nscratch:\n.space 16\n").unwrap();
        let bss = obj.section("__DATA", "__bss").unwrap();
        assert!(bss.data.is_empty());
        assert_eq!(bss.size, 16);
        assert_eq!(bss.align_pow2, 4);
        let scratch = obj
            .symbols
            .iter()
            .find(|sym| sym.name == "scratch")
            .unwrap();
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

        let reloc_syms: Vec<_> = text_relocs(&obj)
            .iter()
            .map(|rel| obj.symbols[rel.symbol_idx as usize].name.as_str())
            .collect();
        assert_eq!(reloc_syms, vec!["second", "second"]);
    }

    #[test]
    fn assemble_page_reloc_creates_undefined_external_symbol() {
        let obj = assemble_source(
            ".global _main\n.text\n_main:\nadrp x0, _foo@PAGE\nadd x0, x0, _foo@PAGEOFF\nret\n",
        )
        .unwrap();

        let foo = obj.symbols.iter().find(|sym| sym.name == "_foo").unwrap();
        assert!(foo.undefined);
        assert!(foo.global);

        let reloc_syms: Vec<_> = text_relocs(&obj)
            .iter()
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

        let tls_counter = obj
            .symbols
            .iter()
            .find(|sym| sym.name == "_tls_counter")
            .unwrap();
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
        assert_eq!(
            relocs[0].reloc_type,
            crate::macho::ARM64_RELOC_POINTER_TO_GOT
        );
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
        assert_eq!(
            relocs[0].reloc_type,
            crate::macho::ARM64_RELOC_POINTER_TO_GOT
        );
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
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::B { offset: 8 }.encode().to_le_bytes()
        );
    }

    #[test]
    fn assemble_backward_branch_label() {
        let obj = assemble_source(".text\nstart:\nnop\nb start\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[4..8],
            &Inst::B { offset: -4 }.encode().to_le_bytes()
        );
    }

    #[test]
    fn assemble_local_cbz_label() {
        let obj = assemble_source(".text\nstart:\ncbz x0, done\nret\ndone:\nret\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::Cbz {
                rt: X0,
                offset: 8,
                sf: true
            }
            .encode()
            .to_le_bytes()
        );
    }

    #[test]
    fn assemble_local_tbz_label() {
        let obj = assemble_source(".text\ntbz x0, #5, done\nnop\ndone:\nret\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::Tbz {
                rt: X0,
                bit: 5,
                offset: 8,
                sf: true
            }
            .encode()
            .to_le_bytes()
        );
    }

    #[test]
    fn assemble_adr_local_label() {
        let obj = assemble_source(".text\nadr x0, target\nret\ntarget:\nret\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::Adr { rd: X0, imm: 8 }.encode().to_le_bytes()
        );
    }

    #[test]
    fn assemble_ldr_literal_local_label() {
        let obj =
            assemble_source(".text\nldr x0, target\nret\n.p2align 3\ntarget:\n.quad 42\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::LdrLit64 { rt: X0, offset: 8 }.encode().to_le_bytes()
        );
    }

    #[test]
    fn assemble_ldrsw_literal_local_label() {
        let obj = assemble_source(".text\nldrsw x0, target\nret\ntarget:\n.word -1\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::LdrswLit { rt: X0, offset: 8 }.encode().to_le_bytes()
        );
    }

    #[test]
    fn assemble_ldr_d_literal_local_label() {
        let obj =
            assemble_source(".text\nldr d0, target\nret\n.p2align 3\ntarget:\n.quad 42\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::LdrFpLit64 { rt: D0, offset: 8 }
                .encode()
                .to_le_bytes()
        );
    }

    #[test]
    fn assemble_ldr_s_literal_local_label() {
        let obj = assemble_source(".text\nldr s0, target\nret\ntarget:\n.word 42\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::LdrFpLit32 { rt: S0, offset: 8 }
                .encode()
                .to_le_bytes()
        );
    }

    #[test]
    fn assemble_ldr_q_literal_local_label() {
        let obj =
            assemble_source(".text\nldr q0, target\nret\n.p2align 4\ntarget:\n.zero 16\n").unwrap();
        assert_eq!(
            &text_bytes(&obj)[0..4],
            &Inst::LdrFpLit128 {
                rt: FpReg::new(0),
                offset: 16
            }
            .encode()
            .to_le_bytes()
        );
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
        assert!(obj
            .symbols
            .iter()
            .any(|sym| sym.name == "_puts" && sym.undefined));
    }

    #[test]
    fn assemble_external_b_creates_branch_relocation() {
        let obj = assemble_source(".text\nb _exit\n").unwrap();
        let reloc_name = &obj.symbols[text_relocs(&obj)[0].symbol_idx as usize].name;
        assert_eq!(reloc_name, "_exit");
        assert!(obj
            .symbols
            .iter()
            .any(|sym| sym.name == "_exit" && sym.undefined));
    }

    #[test]
    fn assemble_local_bl_with_subsections_creates_branch_relocation() {
        let obj = assemble_source(
            ".text\n\
             .subsections_via_symbols\n\
             .globl _entry\n\
             _entry:\n\
             bl _helper\n\
             ret\n\
             .p2align 2\n\
             _helper:\n\
             ret\n",
        )
        .unwrap();
        let text_relocs = text_relocs(&obj);
        assert_eq!(text_relocs.len(), 1);
        assert_eq!(
            text_relocs[0].reloc_type,
            crate::macho::ARM64_RELOC_BRANCH26
        );
        let helper = &obj.symbols[text_relocs[0].symbol_idx as usize];
        assert_eq!(helper.name, "_helper");
        assert!(!helper.undefined);
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
    fn assemble_linker_optimization_hint_emits_adrp_ldr_payload() {
        let obj = assemble_source(
            ".text\n\
             .globl _f\n\
             _f:\n\
             Lloh0:\n\
               adrp x8, _g_counter@PAGE\n\
             Lloh1:\n\
               ldr w8, [x8, _g_counter@PAGEOFF]\n\
               ret\n\
               .loh AdrpLdr Lloh0, Lloh1\n\
             .data\n\
             _g_counter:\n\
               .long 7\n",
        )
        .unwrap();
        assert_eq!(obj.linker_optimization_hints, vec![2, 2, 0, 4, 0, 0, 0, 0]);
    }

    #[test]
    fn assemble_linker_optimization_hint_emits_adrp_ldr_got_ldr_payload() {
        let obj = assemble_source(
            ".text\n\
             .globl _f\n\
             _f:\n\
             Lloh0:\n\
               adrp x8, _ext_value@GOTPAGE\n\
             Lloh1:\n\
               ldr x8, [x8, _ext_value@GOTPAGEOFF]\n\
             Lloh2:\n\
               ldr w8, [x8]\n\
               ret\n\
               .loh AdrpLdrGotLdr Lloh0, Lloh1, Lloh2\n",
        )
        .unwrap();
        assert_eq!(obj.linker_optimization_hints, vec![4, 3, 0, 4, 8, 0, 0, 0]);
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
        let msg_index = names
            .iter()
            .position(|name| *name == "msg")
            .expect("msg symbol");
        let ltmp1_index = names
            .iter()
            .position(|name| *name == "ltmp1")
            .expect("ltmp1 symbol");
        assert!(msg_index < ltmp1_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_global_page_target_does_not_jump_ahead_of_later_local_text_symbol() {
        let obj = assemble_source(
            ".globl _main\n\
             .text\n\
             _main:\n\
               adrp x0, msg@PAGE\n\
               add x0, x0, msg@PAGEOFF\n\
               ret\n\
             _local_after_main:\n\
               ret\n\
             .section __TEXT,__cstring,cstring_literals\n\
             msg:\n\
               .asciz \"x\"\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let local_index = names
            .iter()
            .position(|name| *name == "_local_after_main")
            .expect("local symbol");
        let msg_index = names
            .iter()
            .position(|name| *name == "msg")
            .expect("msg symbol");
        assert!(local_index < msg_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_weak_definition_keeps_source_order_before_later_cstring_label() {
        let src = ".globl _main\n\
                   .weak_definition _local_optional\n\
                   .text\n\
                   _main:\n\
                     adrp x0, msg@PAGE\n\
                     add x0, x0, msg@PAGEOFF\n\
                     ret\n\
                   _local_optional:\n\
                     ret\n\
                   .section __TEXT,__cstring,cstring_literals\n\
                   msg:\n\
                     .asciz \"x\"\n";
        let stmts = parse::parse_with_locations(src).unwrap();
        let mut asm = Assembler::new();
        asm.collect_layout(&stmts).unwrap();
        assert!(
            asm.symbol_order["_local_optional"] < asm.symbol_order["msg"],
            "symbol_order: {:?}",
            asm.symbol_order
        );

        let obj = assemble_located_stmts(&stmts).unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let local_index = names
            .iter()
            .position(|name| *name == "_local_optional")
            .expect("local symbol");
        let msg_index = names
            .iter()
            .position(|name| *name == "msg")
            .expect("msg symbol");
        assert!(local_index < msg_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_mixed_link_fixture_keeps_local_text_before_cstring_symbol() {
        let src = include_str!("../tests/corpus/mixed_link_runtime_main.s");
        let stmts = parse::parse_with_locations(src).unwrap();
        let mut asm = Assembler::new();
        asm.collect_layout(&stmts).unwrap();
        assert!(
            asm.symbol_order["_local_optional"] < asm.symbol_order["msg"],
            "symbol_order: {:?}",
            asm.symbol_order
        );

        let obj = assemble_located_stmts(&stmts).unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let local_index = names
            .iter()
            .position(|name| *name == "_local_optional")
            .expect("local symbol");
        let msg_index = names
            .iter()
            .position(|name| *name == "msg")
            .expect("msg symbol");
        let ltmp1_index = names
            .iter()
            .position(|name| *name == "ltmp1")
            .expect("cstring section temp");
        let scratch_index = names
            .iter()
            .position(|name| *name == "_scratch_buf")
            .expect("bss symbol");
        let ltmp2_index = names
            .iter()
            .position(|name| *name == "ltmp2")
            .expect("bss section temp");
        let ltmp3_index = names
            .iter()
            .position(|name| *name == "ltmp3")
            .expect("thread data section temp");
        let tls_value_init_index = names
            .iter()
            .position(|name| *name == "_tls_value$tlv$init")
            .expect("thread data init");
        let ltmp4_index = names
            .iter()
            .position(|name| *name == "ltmp4")
            .expect("thread bss section temp");
        let tls_counter_init_index = names
            .iter()
            .position(|name| *name == "_tls_counter$tlv$init")
            .expect("thread bss init");
        assert!(local_index < msg_index, "symbols: {:?}", names);
        assert!(scratch_index < ltmp1_index, "symbols: {:?}", names);
        assert!(msg_index < ltmp1_index, "symbols: {:?}", names);
        assert!(scratch_index < ltmp2_index, "symbols: {:?}", names);
        assert!(ltmp3_index < tls_value_init_index, "symbols: {:?}", names);
        assert!(ltmp4_index < tls_counter_init_index, "symbols: {:?}", names);

        let mut buf = Vec::new();
        write_macho(&obj, &mut buf).unwrap();
        let written_names = file_symbol_names(&buf);
        assert_eq!(written_names, names, "written symbols: {:?}", written_names);
    }

    #[test]
    fn finish_rejects_generated_section_size_drift() {
        let stmts = parse::parse_with_locations(
            ".text\n\
             _f:\n\
             .cfi_startproc\n\
             ret\n\
             .cfi_restore w29\n\
             .cfi_endproc\n",
        )
        .unwrap();
        let mut asm = Assembler::new();
        asm.collect_layout(&stmts).unwrap();
        asm.prepare_unwind_layout().unwrap();
        asm.prepare_expression_state(&stmts).unwrap();
        asm.reset_for_emission();
        asm.process(&stmts).unwrap();
        asm.resolve_fixups().unwrap();

        asm.eh_frame_rows[0].instructions.push(0);
        let error = asm
            .finish()
            .expect_err("generated section size drift unexpectedly passed");
        assert_eq!(error.msg, "section layout changed between assembly passes");
    }

    #[test]
    fn assemble_section_base_reloc_target_sorts_ahead_of_later_section_temps() {
        let obj = assemble_source(
            ".build_version macos, 11, 0 sdk_version 15, 5\n\
             .subsections_via_symbols\n\
             .globl _stress_1\n\
             .text\n\
             .p2align 2\n\
             _stress_1:\n\
               adrp x11, cstr0_1@PAGE\n\
               add x11, x11, cstr0_1@PAGEOFF\n\
               adrp x13, data0_1@PAGE\n\
               add x13, x13, data0_1@PAGEOFF\n\
               ret\n\
             .section __TEXT,__cstring,cstring_literals\n\
             cstr0_1:\n\
               .asciz \"stress-1-a\"\n\
             cstr1_1:\n\
               .asciz \"stress-1-b\"\n\
             .section __TEXT,__const\n\
             const0_1:\n\
               .quad data0_1\n\
             .data\n\
             .p2align 3\n\
             data0_1:\n\
               .quad cstr0_1\n\
             .zerofill __DATA,__bss,_scratch_1,32,4\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let data0_index = names
            .iter()
            .position(|name| *name == "data0_1")
            .expect("data0_1 symbol");
        let ltmp1_index = names
            .iter()
            .position(|name| *name == "ltmp1")
            .expect("ltmp1 symbol");
        assert!(data0_index < ltmp1_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_tls_init_symbol_stays_after_thread_data_section_temp() {
        let obj = assemble_source(
            ".text\n\
             .globl _read_tls_plus_one\n\
             _read_tls_plus_one:\n\
               adrp x0, _tls_value@TLVPPAGE\n\
               ldr x0, [x0, _tls_value@TLVPPAGEOFF]\n\
               ret\n\
             .section __DATA,__thread_data,thread_local_regular\n\
             .p2align 2\n\
             _tls_value$tlv$init:\n\
               .long 5\n\
             .section __DATA,__thread_vars,thread_local_variables\n\
             .globl _tls_value\n\
             _tls_value:\n\
               .quad __tlv_bootstrap\n\
               .quad 0\n\
               .quad _tls_value$tlv$init\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let ltmp1_index = names
            .iter()
            .position(|name| *name == "ltmp1")
            .expect("ltmp1 symbol");
        let tls_init_index = names
            .iter()
            .position(|name| *name == "_tls_value$tlv$init")
            .expect("tls init symbol");
        assert!(ltmp1_index < tls_init_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_const_reloc_target_anchors_before_later_const_symbols() {
        let obj = assemble_source(
            ".build_version macos, 11, 0 sdk_version 15, 5\n\
             .subsections_via_symbols\n\
             .globl _fuzz_1\n\
             .text\n\
             _fuzz_1:\n\
               ret\n\
             .section __TEXT,__const\n\
             .p2align 3\n\
             const0_1:\n\
               .quad data0_1\n\
             const1_1:\n\
               .quad _ext_1\n\
             const2_1:\n\
               .quad _other_1 - _ext_1 + 12\n\
             .data\n\
             .p2align 3\n\
             data0_1:\n\
               .quad 0\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let const0_index = names
            .iter()
            .position(|name| *name == "const0_1")
            .expect("const0_1 symbol");
        let data0_index = names
            .iter()
            .position(|name| *name == "data0_1")
            .expect("data0_1 symbol");
        let const1_index = names
            .iter()
            .position(|name| *name == "const1_1")
            .expect("const1_1 symbol");
        assert!(const0_index < data0_index, "symbols: {:?}", names);
        assert!(data0_index < const1_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_const_section_temp_sits_between_base_and_later_const_labels() {
        let obj = assemble_source(
            ".text\n\
             _f:\n\
               adrp x0, const0@PAGE\n\
               add x0, x0, const0@PAGEOFF\n\
               ret\n\
             .section __TEXT,__const\n\
             const0:\n\
               .quad 1\n\
             const1:\n\
               .quad 2\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let const0_index = names.iter().position(|name| *name == "const0").unwrap();
        let const1_index = names.iter().position(|name| *name == "const1").unwrap();
        let ltmp1_index = names.iter().position(|name| *name == "ltmp1").unwrap();
        assert!(const0_index < ltmp1_index, "symbols: {:?}", names);
        assert!(ltmp1_index < const1_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_literal16_section_temp_sits_between_base_and_later_literal_labels() {
        let obj = assemble_source(
            ".text\n\
             _f:\n\
               adrp x0, lit0@PAGE\n\
               ldr q0, [x0, lit0@PAGEOFF]\n\
               ret\n\
             .section __TEXT,__literal16\n\
             .p2align 4\n\
             lit0:\n\
               .byte 1\n\
             .space 15\n\
             lit1:\n\
               .byte 2\n\
             .space 15\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let ltmp1_index = names.iter().position(|name| *name == "ltmp1").unwrap();
        let lit0_index = names.iter().position(|name| *name == "lit0").unwrap();
        let lit1_index = names.iter().position(|name| *name == "lit1").unwrap();
        assert!(lit0_index < ltmp1_index, "symbols: {:?}", names);
        assert!(ltmp1_index < lit1_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_literal16_compiler_pool_temp_stays_before_lcpi_symbols() {
        let obj = assemble_source(
            ".text\n\
             _f:\n\
               adrp x0, lCPI0_0@PAGE\n\
               ldr q0, [x0, lCPI0_0@PAGEOFF]\n\
               ret\n\
             .section __TEXT,__literal16\n\
             .p2align 4\n\
             lCPI0_0:\n\
               .byte 1\n\
             .space 15\n\
             lCPI1_0:\n\
               .byte 2\n\
             .space 15\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let ltmp1_index = names.iter().position(|name| *name == "ltmp1").unwrap();
        let lcpi0_index = names.iter().position(|name| *name == "lCPI0_0").unwrap();
        let lcpi1_index = names.iter().position(|name| *name == "lCPI1_0").unwrap();
        assert!(ltmp1_index < lcpi0_index, "symbols: {:?}", names);
        assert!(lcpi0_index < lcpi1_index, "symbols: {:?}", names);
    }

    #[test]
    fn assemble_cstring_section_temp_sits_between_base_and_later_cstring_labels() {
        let obj = assemble_source(
            ".build_version macos, 11, 0 sdk_version 15, 5\n\
             .subsections_via_symbols\n\
             .globl _fuzz_1\n\
             .text\n\
             _fuzz_1:\n\
               adrp x16, cstr0_1@PAGE\n\
               add x16, x16, cstr0_1@PAGEOFF\n\
               adrp x17, _ext_1@GOTPAGE\n\
               ldr x17, [x17, _ext_1@GOTPAGEOFF]\n\
               ret\n\
             .section __TEXT,__cstring,cstring_literals\n\
             cstr0_1:\n\
               .asciz \"hello\"\n\
             cstr1_1:\n\
               .asciz \"world\"\n\
             .section __TEXT,__const\n\
             .p2align 3\n\
             const0_1:\n\
               .quad data0_1\n\
             const1_1:\n\
               .quad _ext_1\n\
             .data\n\
             .p2align 3\n\
             data0_1:\n\
               .quad cstr0_1\n",
        )
        .unwrap();
        let names: Vec<_> = obj.symbols.iter().map(|sym| sym.name.as_str()).collect();
        let ltmp1_index = names.iter().position(|name| *name == "ltmp1").unwrap();
        let cstr0_index = names.iter().position(|name| *name == "cstr0_1").unwrap();
        let cstr1_index = names.iter().position(|name| *name == "cstr1_1").unwrap();
        assert!(cstr0_index < ltmp1_index, "symbols: {:?}", names);
        assert!(ltmp1_index < cstr1_index, "symbols: {:?}", names);
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
        let err =
            assemble_source(".text\ntbz x0, #5, done\n.space 32768\ndone:\nret\n").unwrap_err();
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
