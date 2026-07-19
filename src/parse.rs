//! ARM64 assembly parser.
//!
//! Parses tokenized assembly into structured statements: instructions (as `Inst`),
//! labels, and directives. Resolves instruction aliases (cmp, mov, tst, etc.)
//! to their canonical forms.

use crate::encode::{AddrExtend, BarrierOpt, Inst, RegExtend, RegShift};
use crate::expr::{self, Expr, SymbolModifier};
use crate::lex::{LexError, Lexer, Tok, Token};
use crate::reg::*;

use std::collections::BTreeMap;
use std::fmt;

/// A parsed assembly statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Label(String),
    Instruction(Inst),
    /// Instruction with a label reference that needs relocation.
    InstructionWithReloc(Inst, LabelRef),
    Directive(Directive),
}

/// A label reference in an instruction that needs relocation.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRef {
    pub symbol: String,
    pub kind: RelocKind,
    pub addend: i64,
}

/// What kind of relocation is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocKind {
    /// ADRP — page-relative (ARM64_RELOC_PAGE21)
    Page21,
    /// ADRP — page-relative to GOT slot page (ARM64_RELOC_GOT_LOAD_PAGE21)
    GotLoadPage21,
    /// ADRP — page-relative to TLVP slot page (ARM64_RELOC_TLVP_LOAD_PAGE21)
    TlvpLoadPage21,
    /// ADD/LDR — page offset (ARM64_RELOC_PAGEOFF12)
    PageOff12,
    /// LDR — page offset to GOT slot (ARM64_RELOC_GOT_LOAD_PAGEOFF12)
    GotLoadPageOff12,
    /// LDR — page offset to TLVP slot (ARM64_RELOC_TLVP_LOAD_PAGEOFF12)
    TlvpLoadPageOff12,
    /// B/BL — branch (ARM64_RELOC_BRANCH26)
    Branch26,
    /// B.cond / CBZ / CBNZ — assembler-resolved 19-bit branch immediate
    Branch19,
    /// TBZ / TBNZ — assembler-resolved 14-bit branch immediate
    Branch14,
    /// LDR literal — assembler-resolved 19-bit PC-relative load
    Literal19,
    /// ADR — assembler-resolved 21-bit PC-relative address
    Adr21,
}

/// Assembly directives.
#[derive(Debug, Clone, PartialEq)]
pub enum Directive {
    Text,
    Data,
    Comm {
        name: String,
        size: u64,
        align_pow2: u8,
    },
    Extern(String),
    Global(String),
    PrivateExtern(String),
    WeakReference(String),
    WeakDefinition(String),
    Set(String, Expr),
    Align {
        power: u32,
        fill: Option<u8>,
        max_skip: Option<u64>,
    },
    P2Align {
        power: u32,
        fill: Option<u8>,
        max_skip: Option<u64>,
    },
    Byte(Vec<Expr>),
    Short(Vec<Expr>),
    Word(Vec<Expr>),
    Quad(Vec<Expr>),
    Ascii(Vec<u8>),
    Asciz(Vec<u8>),
    Space(u64),
    Fill {
        repeat: u64,
        size: u8,
        value: u64,
    },
    Zerofill {
        segment: String,
        section: String,
        symbol: Option<String>,
        size: u64,
        align_pow2: u32,
    },
    CfiStartProc,
    CfiEndProc,
    CfiDefCfa {
        register: GpReg,
        offset: i64,
    },
    CfiDefCfaOffset(i64),
    CfiDefCfaRegister(GpReg),
    CfiOffset {
        register: GpReg,
        offset: i64,
    },
    CfiRestore(GpReg),
    CfiAdjustCfaOffset(i64),
    Section(String, String),
    SubsectionsViaSymbols,
    BuildVersion(BuildVersionDirective),
    LinkerOptimizationHint(LinkerOptimizationHintDirective),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionTriple {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildVersionDirective {
    pub platform: String,
    pub minos: VersionTriple,
    pub sdk: Option<VersionTriple>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkerOptimizationHintDirective {
    pub kind: String,
    pub labels: Vec<String>,
}

fn linker_optimization_hint_label_count(kind: &str) -> Option<usize> {
    match kind {
        "AdrpLdrGotLdr" => Some(3),
        "AdrpAdd" | "AdrpLdr" | "AdrpLdrGot" => Some(2),
        _ => None,
    }
}

/// Parse error with source location.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

fn allowed_section_attrs(seg: &str, sect: &str) -> Option<&'static [&'static str]> {
    let seg = seg.to_ascii_lowercase();
    let sect = sect.to_ascii_lowercase();
    match (seg.as_str(), sect.as_str()) {
        ("__text", "__text") => Some(&["regular", "pure_instructions"]),
        ("__text", "__cstring") => Some(&["regular", "cstring_literals"]),
        ("__text", "__literal16") => Some(&["regular", "16byte_literals"]),
        ("__text", "__const") => Some(&["regular"]),
        ("__data", "__data") => Some(&["regular"]),
        ("__data", "__const") => Some(&["regular"]),
        // Apple `as` accepts either thread-local attr spelling here and
        // canonicalizes based on the section name.
        ("__data", "__thread_data") => {
            Some(&["regular", "thread_local_regular", "thread_local_variables"])
        }
        ("__data", "__thread_vars") => {
            Some(&["regular", "thread_local_regular", "thread_local_variables"])
        }
        _ => None,
    }
}

fn validate_section_attrs(seg: &str, sect: &str, attrs: &[String]) -> Result<(), String> {
    if attrs.is_empty() {
        return Ok(());
    }
    let Some(allowed) = allowed_section_attrs(seg, sect) else {
        return Err(format!(
            "section {},{} does not support explicit attributes",
            seg, sect
        ));
    };
    let unsupported: Vec<_> = attrs
        .iter()
        .filter(|attr| {
            !allowed
                .iter()
                .any(|allowed_attr| attr.eq_ignore_ascii_case(allowed_attr))
        })
        .cloned()
        .collect();
    if unsupported.is_empty() {
        if seg.eq_ignore_ascii_case("__TEXT")
            && sect.eq_ignore_ascii_case("__text")
            && attrs
                .iter()
                .any(|attr| attr.eq_ignore_ascii_case("pure_instructions"))
            && !attrs
                .iter()
                .any(|attr| attr.eq_ignore_ascii_case("regular"))
        {
            return Err(format!(
                "section {},{} requires 'regular' when using 'pure_instructions'",
                seg, sect
            ));
        }

        return Ok(());
    }

    Err(format!(
        "unsupported section attributes for {},{}: {} (supported attrs: {})",
        seg,
        sect,
        unsupported.join(", "),
        allowed.join(", ")
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocatedStmt {
    pub stmt: Stmt,
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: error: {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError {
            line: e.line,
            col: e.col,
            msg: e.msg,
        }
    }
}

/// Parse assembly source text into a list of statements.
pub fn parse(src: &str) -> Result<Vec<Stmt>, ParseError> {
    Ok(parse_with_locations(src)?
        .into_iter()
        .map(|stmt| stmt.stmt)
        .collect())
}

/// Parse assembly source text into statements with source locations.
pub fn parse_with_locations(src: &str) -> Result<Vec<LocatedStmt>, ParseError> {
    let tokens = Lexer::tokenize(src)?;
    let previews = scan_absolute_assignments(&tokens);
    let assignments: Vec<_> = previews
        .iter()
        .map(|preview| (preview.name.clone(), preview.expr.clone()))
        .collect();
    let resolved = expr::resolve_absolute_assignments(&assignments, &BTreeMap::new());

    let mut p = Parser::new(&tokens);
    let mut first_assignments = BTreeMap::new();
    for (preview, value) in previews.iter().zip(resolved) {
        let value = value.map_err(|error| {
            let origin = &previews[error.assignment_index()];
            LocatedAbsoluteAssignmentError {
                error,
                line: origin.line,
                col: origin.col,
            }
        });
        let first_value = value.as_ref().ok().copied();
        p.absolute_assignment_values
            .insert((preview.line, preview.col), value);
        if first_assignments.insert(preview.name.clone(), ()).is_none() {
            if let Some(value) = first_value {
                p.absolute_symbols.insert(preview.name.clone(), value);
            }
        }
    }
    p.parse_program()
}

struct AbsoluteAssignmentPreview {
    name: String,
    expr: Expr,
    line: u32,
    col: u32,
}

#[derive(Clone)]
struct LocatedAbsoluteAssignmentError {
    error: expr::AbsoluteAssignmentError,
    line: u32,
    col: u32,
}

const MAX_EXPRESSION_DEPTH: usize = 256;

struct ParsedExpr {
    expr: Expr,
    depth: usize,
}

impl ParsedExpr {
    fn leaf(expr: Expr) -> Self {
        Self { expr, depth: 0 }
    }
}

fn scan_absolute_assignments(tokens: &[Token]) -> Vec<AbsoluteAssignmentPreview> {
    let mut assignments = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(&token.kind, Tok::Ident(name) if name == ".set" || name == ".equ") {
            continue;
        }
        if index > 0 && !matches!(tokens[index - 1].kind, Tok::Newline | Tok::Colon) {
            continue;
        }

        let mut parser = Parser::new(tokens);
        parser.pos = index + 1;
        let parsed = (|| {
            let name = parser.expect_ident()?;
            parser.expect(&Tok::Comma)?;
            let expr = parser.parse_absolute_assignment_expr()?;
            if !parser.at_end_of_stmt() {
                return Err(parser.err("invalid absolute assignment".into()));
            }
            Ok((name, expr))
        })();
        if let Ok((name, expr)) = parsed {
            assignments.push(AbsoluteAssignmentPreview {
                name,
                expr,
                line: token.line,
                col: token.col,
            });
        }
    }
    assignments
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    absolute_symbols: BTreeMap<String, i64>,
    absolute_assignment_values: BTreeMap<(u32, u32), Result<i64, LocatedAbsoluteAssignmentError>>,
    numeric_labels: BTreeMap<u32, u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumericLabelDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddSubModifier {
    Shift(RegShift, u8),
    Extend(RegExtend, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GpRegKind {
    Reg,
    Sp,
    Zr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GpOperandMeta {
    is_64bit: bool,
    kind: GpRegKind,
    start: usize,
}

impl GpOperandMeta {
    fn new(is_64bit: bool, kind: GpRegKind, start: usize) -> Self {
        Self {
            is_64bit,
            kind,
            start,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FpMemWidth {
    B8,
    H16,
    S32,
    D64,
    Q128,
}

impl FpMemWidth {
    fn scale(self) -> u8 {
        match self {
            FpMemWidth::B8 => 0,
            FpMemWidth::H16 => 1,
            FpMemWidth::S32 => 2,
            FpMemWidth::D64 => 3,
            FpMemWidth::Q128 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SimdLaneWidth {
    B8,
    H16,
    S32,
    D64,
}

impl SimdLaneWidth {
    fn max_index(self) -> u8 {
        match self {
            SimdLaneWidth::B8 => 15,
            SimdLaneWidth::H16 => 7,
            SimdLaneWidth::S32 => 3,
            SimdLaneWidth::D64 => 1,
        }
    }
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            pos: 0,
            absolute_symbols: BTreeMap::new(),
            absolute_assignment_values: BTreeMap::new(),
            numeric_labels: BTreeMap::new(),
        }
    }

    fn peek(&self) -> &Tok {
        if self.pos < self.tokens.len() {
            &self.tokens[self.pos].kind
        } else {
            &Tok::Eof
        }
    }

    fn token_at(&self, pos: usize) -> &Token {
        &self.tokens[pos.min(self.tokens.len() - 1)]
    }

    fn cur(&self) -> &Token {
        self.token_at(self.pos)
    }

    fn advance(&mut self) -> &Token {
        let t = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.advance();
                Ok(s)
            }
            other => Err(self.err(format!("expected identifier, got {}", other))),
        }
    }

    fn parse_section_attr(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Tok::Ident(attr) => {
                self.advance();
                Ok(attr)
            }
            Tok::Integer(value) => {
                let start = self.cur().clone();
                self.advance();
                let mut attr = value.to_string();
                let width = u32::try_from(attr.len()).unwrap_or(u32::MAX);
                let next_is_adjacent = self.cur().line == start.line
                    && self.cur().col == start.col.saturating_add(width);
                if next_is_adjacent {
                    if let Tok::Ident(suffix) = self.peek().clone() {
                        self.advance();
                        attr.push_str(&suffix);
                    }
                }
                Ok(attr)
            }
            other => Err(self.err(format!("expected section attribute, got {}", other))),
        }
    }

    fn expect(&mut self, kind: &Tok) -> Result<(), ParseError> {
        if self.peek() == kind {
            self.advance();
            Ok(())
        } else {
            Err(self.err(format!("expected {}, got {}", kind, self.peek())))
        }
    }

    fn eat(&mut self, kind: &Tok) -> bool {
        if self.peek() == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn err(&self, msg: String) -> ParseError {
        let t = self.cur();
        ParseError {
            line: t.line,
            col: t.col,
            msg,
        }
    }

    fn err_at(&self, pos: usize, msg: String) -> ParseError {
        let t = &self.tokens[pos.min(self.tokens.len() - 1)];
        ParseError {
            line: t.line,
            col: t.col,
            msg,
        }
    }

    fn expression_depth_error(&self, pos: usize) -> ParseError {
        self.err_at(
            pos,
            format!("expression exceeds maximum depth of {MAX_EXPRESSION_DEPTH}"),
        )
    }

    fn at_end_of_stmt(&self) -> bool {
        matches!(self.peek(), Tok::Newline | Tok::Eof)
    }

    fn skip_newlines(&mut self) {
        while self.peek() == &Tok::Newline {
            self.advance();
        }
    }

    fn parse_program(&mut self) -> Result<Vec<LocatedStmt>, ParseError> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while self.peek() != &Tok::Eof {
            self.parse_line(&mut stmts)?;
            if !self.at_end_of_stmt() {
                return Err(self.err(format!("unexpected trailing token: {}", self.peek())));
            }
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn parse_line(&mut self, stmts: &mut Vec<LocatedStmt>) -> Result<(), ParseError> {
        // A line can be: label, directive, instruction, or empty.
        match self.peek().clone() {
            Tok::Ident(ref name) if name.starts_with('.') => {
                // Could be directive or local label.
                let start = self.cur().clone();
                let name = name.clone();
                self.advance();
                if self.eat(&Tok::Colon) {
                    // Local label (.Lxxx:)
                    stmts.push(LocatedStmt {
                        stmt: Stmt::Label(name),
                        line: start.line,
                        col: start.col,
                    });
                    if !self.at_end_of_stmt() {
                        self.parse_line(stmts)?;
                    }
                } else {
                    // Directive
                    stmts.push(LocatedStmt {
                        stmt: self.parse_directive(&name, start.line, start.col)?,
                        line: start.line,
                        col: start.col,
                    });
                }
            }
            Tok::Ident(_) => {
                let start = self.cur().clone();
                let name = if let Tok::Ident(s) = self.peek().clone() {
                    s
                } else {
                    unreachable!()
                };
                self.advance();
                if self.eat(&Tok::Colon) {
                    // Label
                    stmts.push(LocatedStmt {
                        stmt: Stmt::Label(name),
                        line: start.line,
                        col: start.col,
                    });
                    // There might be an instruction on the same line.
                    if !self.at_end_of_stmt() {
                        self.parse_line(stmts)?;
                    }
                } else {
                    // Instruction mnemonic — might have a condition suffix.
                    let mnemonic = self.resolve_mnemonic(&name)?;
                    stmts.push(LocatedStmt {
                        stmt: self.parse_instruction(&mnemonic)?,
                        line: start.line,
                        col: start.col,
                    });
                }
            }
            Tok::Integer(_) if self.numeric_label_definition_number().is_some() => {
                let start = self.cur().clone();
                let number = self.numeric_label_definition_number().unwrap();
                self.advance();
                self.expect(&Tok::Colon)?;
                stmts.push(LocatedStmt {
                    stmt: Stmt::Label(self.define_numeric_label(number)),
                    line: start.line,
                    col: start.col,
                });
                if !self.at_end_of_stmt() {
                    self.parse_line(stmts)?;
                }
            }
            Tok::Newline | Tok::Eof => {}
            _ => return Err(self.err(format!("unexpected token: {}", self.peek()))),
        }
        Ok(())
    }

    /// Handle conditional branch mnemonics: "b" followed by ".eq", ".ne", etc.
    fn resolve_mnemonic(&mut self, name: &str) -> Result<String, ParseError> {
        let lower = name.to_lowercase();
        if let Some(cond_name) = lower.strip_prefix("b.") {
            if parse_condition(cond_name).is_some() {
                return Ok(format!("b.{}", cond_name));
            }
        }
        if lower == "b" {
            // Check for .cond suffix (e.g., B.EQ, b.ne)
            if let Tok::Ident(ref cond) = self.peek().clone() {
                if let Some(cond_name) = cond.strip_prefix('.') {
                    if parse_condition(cond_name).is_none() {
                        return Ok(lower);
                    }
                    let full = format!("b.{}", cond_name.to_lowercase());
                    self.advance();
                    return Ok(full);
                }
            }
        }
        Ok(lower)
    }

    fn parse_directive(&mut self, name: &str, line: u32, col: u32) -> Result<Stmt, ParseError> {
        let dir = match name {
            ".text" => Directive::Text,
            ".data" => Directive::Data,
            ".cstring" => Directive::Section("__TEXT".into(), "__cstring".into()),
            ".comm" => {
                let sym = self.expect_ident()?;
                self.expect(&Tok::Comma)?;
                let size = self.parse_unsigned_const_expr::<u64>("common size expression")?;
                let align_pow2 = if self.eat(&Tok::Comma) {
                    self.parse_unsigned_const_expr::<u8>("common alignment expression")?
                } else {
                    0
                };
                Directive::Comm {
                    name: sym,
                    size,
                    align_pow2,
                }
            }
            ".extern" => {
                let sym = self.expect_ident()?;
                Directive::Extern(sym)
            }
            ".global" | ".globl" => {
                let sym = self.expect_ident()?;
                Directive::Global(sym)
            }
            ".private_extern" => {
                let sym = self.expect_ident()?;
                Directive::PrivateExtern(sym)
            }
            ".weak_reference" => {
                let sym = self.expect_ident()?;
                Directive::WeakReference(sym)
            }
            ".weak_definition" => {
                let sym = self.expect_ident()?;
                Directive::WeakDefinition(sym)
            }
            ".set" | ".equ" => {
                let sym = self.expect_ident()?;
                self.expect(&Tok::Comma)?;
                let expr = self.parse_absolute_assignment_expr()?;
                match self.absolute_assignment_values.get(&(line, col)).cloned() {
                    Some(Ok(value)) => {
                        self.absolute_symbols.insert(sym.clone(), value);
                    }
                    Some(Err(error)) if error.error.may_resolve_with_labels() => {
                        self.absolute_symbols.remove(&sym);
                    }
                    Some(Err(error)) => {
                        return Err(ParseError {
                            line: error.line,
                            col: error.col,
                            msg: error.error.to_string(),
                        });
                    }
                    None => {
                        if let Ok(value) = expr::eval_with_symbols(&expr, &self.absolute_symbols) {
                            self.absolute_symbols.insert(sym.clone(), value);
                        } else {
                            self.absolute_symbols.remove(&sym);
                        }
                    }
                }
                Directive::Set(sym, expr)
            }
            ".align" => {
                let (power, fill, max_skip) = self.parse_alignment_directive_args()?;
                Directive::Align {
                    power,
                    fill,
                    max_skip,
                }
            }
            ".p2align" => {
                let (power, fill, max_skip) = self.parse_alignment_directive_args()?;
                Directive::P2Align {
                    power,
                    fill,
                    max_skip,
                }
            }
            ".byte" => Directive::Byte(self.parse_data_expr_list(name, 8)?),
            ".short" => Directive::Short(self.parse_data_expr_list(name, 16)?),
            ".word" | ".long" => Directive::Word(self.parse_data_expr_list(name, 32)?),
            ".quad" => Directive::Quad(self.parse_quad_expr_list()?),
            ".ascii" => {
                if let Tok::StringLit(s) = self.peek().clone() {
                    self.advance();
                    Directive::Ascii(s)
                } else {
                    return Err(self.err("expected string after .ascii".into()));
                }
            }
            ".asciz" | ".string" => {
                if let Tok::StringLit(s) = self.peek().clone() {
                    self.advance();
                    let mut bytes = s;
                    bytes.push(0); // null terminator
                    Directive::Asciz(bytes)
                } else {
                    return Err(self.err("expected string after .asciz".into()));
                }
            }
            ".space" | ".skip" => {
                let n = self.parse_unsigned_const_expr::<u64>("space expression")?;
                Directive::Space(n)
            }
            ".zero" => {
                let n = self.parse_unsigned_const_expr::<u64>("zero expression")?;
                Directive::Space(n)
            }
            ".fill" => {
                let repeat = self.parse_unsigned_const_expr::<u64>("fill repeat expression")?;
                self.expect(&Tok::Comma)?;
                let size = self.parse_apple_fill_size()?;
                let value = if self.eat(&Tok::Comma) {
                    self.parse_apple_fill_pattern()?
                } else {
                    0
                };
                Directive::Fill {
                    repeat,
                    size,
                    value,
                }
            }
            ".zerofill" => {
                let segment = self.expect_ident()?;
                self.expect(&Tok::Comma)?;
                let section = self.expect_ident()?;
                self.expect(&Tok::Comma)?;
                let symbol = if matches!(self.peek(), Tok::Ident(_)) {
                    Some(self.expect_ident()?)
                } else {
                    None
                };
                self.expect(&Tok::Comma)?;
                let size = self.parse_unsigned_const_expr::<u64>("zerofill size expression")?;
                let align_pow2 = if self.eat(&Tok::Comma) {
                    self.parse_unsigned_const_expr::<u32>("zerofill alignment expression")?
                } else {
                    0
                };
                Directive::Zerofill {
                    segment,
                    section,
                    symbol,
                    size,
                    align_pow2,
                }
            }
            ".tbss" => {
                let symbol = self.expect_ident()?;
                self.expect(&Tok::Comma)?;
                let size = self.parse_unsigned_const_expr::<u64>("tbss size expression")?;
                self.expect(&Tok::Comma)?;
                let align_pow2 =
                    self.parse_unsigned_const_expr::<u32>("tbss alignment expression")?;
                Directive::Zerofill {
                    segment: "__DATA".into(),
                    section: "__thread_bss".into(),
                    symbol: Some(symbol),
                    size,
                    align_pow2,
                }
            }
            ".cfi_startproc" => Directive::CfiStartProc,
            ".cfi_endproc" => Directive::CfiEndProc,
            ".cfi_def_cfa" => {
                let register = self.parse_cfi_register()?;
                self.expect(&Tok::Comma)?;
                let offset = self.parse_const_expr("CFA offset")?;
                Directive::CfiDefCfa { register, offset }
            }
            ".cfi_def_cfa_offset" => {
                Directive::CfiDefCfaOffset(self.parse_const_expr("CFA offset")?)
            }
            ".cfi_def_cfa_register" => Directive::CfiDefCfaRegister(self.parse_cfi_register()?),
            ".cfi_offset" => {
                let register = self.parse_cfi_register()?;
                self.expect(&Tok::Comma)?;
                let offset = self.parse_const_expr("CFI offset")?;
                Directive::CfiOffset { register, offset }
            }
            ".cfi_restore" => Directive::CfiRestore(self.parse_cfi_register()?),
            ".cfi_adjust_cfa_offset" => {
                Directive::CfiAdjustCfaOffset(self.parse_const_expr("CFA adjustment")?)
            }
            ".section" => {
                let seg = self.expect_ident()?;
                self.expect(&Tok::Comma)?;
                let sect = self.expect_ident()?;
                let mut attrs = Vec::new();
                while self.eat(&Tok::Comma) {
                    attrs.push(self.parse_section_attr()?);
                }
                if let Err(msg) = validate_section_attrs(&seg, &sect, &attrs) {
                    return Err(self.err(msg));
                }
                Directive::Section(seg, sect)
            }
            ".subsections_via_symbols" => Directive::SubsectionsViaSymbols,
            ".build_version" => {
                let platform = self.expect_ident()?.to_ascii_lowercase();
                self.expect(&Tok::Comma)?;
                let minos = self.parse_version_triple("build version minimum OS")?;
                let sdk = if self.at_end_of_stmt() {
                    None
                } else {
                    let keyword_start = self.pos;
                    let keyword = self.peek().clone();
                    let Tok::Ident(keyword) = &keyword else {
                        return Err(self.err_at(
                            keyword_start,
                            format!("expected sdk_version after .build_version, got {}", keyword),
                        ));
                    };
                    if !keyword.eq_ignore_ascii_case("sdk_version") {
                        return Err(self.err_at(
                            keyword_start,
                            format!("expected sdk_version after .build_version, got {}", keyword),
                        ));
                    }
                    self.advance();
                    Some(self.parse_version_triple("build version SDK")?)
                };
                if !self.at_end_of_stmt() {
                    return Err(self.err("unexpected tokens after .build_version".into()));
                }
                Directive::BuildVersion(BuildVersionDirective {
                    platform,
                    minos,
                    sdk,
                })
            }
            ".loh" => {
                let kind = self.expect_ident()?;
                let Some(expected) = linker_optimization_hint_label_count(&kind) else {
                    return Err(self.err(format!(
                        "unsupported .loh kind '{}' (supported: AdrpAdd, AdrpLdr, AdrpLdrGot, AdrpLdrGotLdr)",
                        kind
                    )));
                };
                let mut labels = Vec::new();
                if !self.at_end_of_stmt() {
                    labels.push(self.expect_ident()?);
                    while !self.at_end_of_stmt() {
                        self.expect(&Tok::Comma)?;
                        labels.push(self.expect_ident()?);
                    }
                }
                if labels.len() != expected {
                    return Err(self.err(format!(
                        ".loh {} expects {} label{}, got {}",
                        kind,
                        expected,
                        if expected == 1 { "" } else { "s" },
                        labels.len()
                    )));
                }
                Directive::LinkerOptimizationHint(LinkerOptimizationHintDirective { kind, labels })
            }
            _ if name.starts_with(".cfi_") => {
                return Err(ParseError {
                    line,
                    col,
                    msg: format!(
                    "unsupported CFI directive '{}' (supported: .cfi_startproc, .cfi_endproc, .cfi_def_cfa, .cfi_def_cfa_offset, .cfi_def_cfa_register, .cfi_offset, .cfi_restore, .cfi_adjust_cfa_offset)",
                    name
                )});
            }
            _ => {
                return Err(ParseError {
                    line,
                    col,
                    msg: format!("unsupported directive '{}'", name),
                });
            }
        };
        Ok(Stmt::Directive(dir))
    }

    fn parse_data_expr_list(&mut self, directive: &str, bits: u8) -> Result<Vec<Expr>, ParseError> {
        let mut values = Vec::new();
        loop {
            let start = self.pos;
            let expr =
                self.parse_expr_with_standalone_unsigned("standalone data-directive bit patterns")?;
            self.validate_data_expr(&expr, directive, bits, start)?;
            values.push(expr);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        Ok(values)
    }

    fn validate_data_expr(
        &self,
        expr: &Expr,
        directive: &str,
        bits: u8,
        start: usize,
    ) -> Result<(), ParseError> {
        let value = match expr {
            Expr::Unsigned(value) if unsigned_data_value_fits(*value, bits) => return Ok(()),
            Expr::Unsigned(value) => value.to_string(),
            _ => match expr::eval_pure(expr) {
                Ok(value) if signed_data_value_fits(value, bits) => return Ok(()),
                Ok(value) => value.to_string(),
                Err(_) => return Ok(()),
            },
        };
        Err(self.err_at(
            start,
            format!(
                "{} expression value {} is out of range for {}-bit data",
                directive, value, bits
            ),
        ))
    }

    fn parse_quad_expr_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut vals = vec![self.parse_quad_expr()?];
        while self.eat(&Tok::Comma) {
            vals.push(self.parse_quad_expr()?);
        }
        Ok(vals)
    }

    fn parse_alignment_directive_args(
        &mut self,
    ) -> Result<(u32, Option<u8>, Option<u64>), ParseError> {
        let power = self.parse_unsigned_const_expr::<u32>("alignment expression")?;
        let mut fill = None;
        let mut max_skip = None;

        if self.eat(&Tok::Comma) {
            if !self.at_end_of_stmt() && self.peek() != &Tok::Comma {
                fill = Some(self.parse_truncated_byte_const_expr("alignment fill expression")?);
            }
            if self.eat(&Tok::Comma) {
                max_skip =
                    Some(self.parse_unsigned_const_expr::<u64>("alignment max-skip expression")?);
            }
        }

        Ok((power, fill, max_skip))
    }

    fn parse_cfi_register(&mut self) -> Result<GpReg, ParseError> {
        let (register, _, _) = self.parse_gp_reg_with_size_kind()?;
        Ok(register)
    }

    fn parse_version_triple(&mut self, context: &str) -> Result<VersionTriple, ParseError> {
        let major = self.parse_version_component(context)?;
        self.expect(&Tok::Comma)?;
        let minor = self.parse_version_component(context)?;
        let patch = if self.eat(&Tok::Comma) {
            self.parse_version_component(context)?
        } else {
            0
        };
        Ok(VersionTriple {
            major,
            minor,
            patch,
        })
    }

    fn parse_version_component(&mut self, context: &str) -> Result<u32, ParseError> {
        match self.peek().clone() {
            Tok::Integer(value) if value >= 0 => {
                self.advance();
                u32::try_from(value).map_err(|_| {
                    self.err(format!(
                        "{} component {} does not fit in u32",
                        context, value
                    ))
                })
            }
            Tok::Integer(value) => Err(self.err(format!(
                "{} component must be non-negative, got {}",
                context, value
            ))),
            other => Err(self.err(format!("expected integer for {}, got {}", context, other))),
        }
    }

    fn parse_const_expr(&mut self, context: &str) -> Result<i64, ParseError> {
        let expr = self.parse_expr()?;
        expr::eval_with_symbols(&expr, &self.absolute_symbols).map_err(|err| {
            self.err(format!(
                "{} must be a pure constant expression: {}",
                context, err
            ))
        })
    }

    fn parse_unsigned_const_expr<T>(&mut self, context: &str) -> Result<T, ParseError>
    where
        T: TryFrom<i64>,
    {
        let start = self.pos;
        let value = self.parse_const_expr(context)?;
        T::try_from(value).map_err(|_| {
            self.err_at(
                start,
                format!(
                    "{} value {} does not fit in {}",
                    context,
                    value,
                    std::any::type_name::<T>()
                ),
            )
        })
    }

    fn parse_truncated_byte_const_expr(&mut self, context: &str) -> Result<u8, ParseError> {
        Ok(self.parse_bit_pattern_const_expr(context)?.to_le_bytes()[0])
    }

    fn parse_apple_fill_pattern(&mut self) -> Result<u64, ParseError> {
        let bytes = self
            .parse_bit_pattern_const_expr("fill value expression")?
            .to_le_bytes();
        // Apple fill patterns are truncated to 32 bits, then zero-extended to the element width.
        Ok(u64::from(u32::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3],
        ])))
    }

    fn parse_apple_fill_size(&mut self) -> Result<u8, ParseError> {
        let start = self.pos;
        let expr = self.parse_expr_with_standalone_unsigned("standalone fill-size values")?;
        let value = match expr {
            Expr::Unsigned(value) => value,
            expr => {
                let value =
                    expr::eval_with_symbols(&expr, &self.absolute_symbols).map_err(|err| {
                        self.err(format!(
                            "fill size expression must be a pure constant expression: {}",
                            err
                        ))
                    })?;
                u64::try_from(value).map_err(|_| {
                    self.err_at(
                        start,
                        format!("fill size expression value {} does not fit in u64", value),
                    )
                })?
            }
        };
        Ok(u8::try_from(value.min(8)).expect("normalized fill size fits in u8"))
    }

    fn parse_bit_pattern_const_expr(&mut self, context: &str) -> Result<u64, ParseError> {
        let expr = self.parse_expr_with_standalone_unsigned("standalone bit-pattern values")?;
        match expr {
            Expr::Unsigned(value) => Ok(value),
            expr => expr::eval_with_symbols(&expr, &self.absolute_symbols)
                .map(|value| u64::from_le_bytes(value.to_le_bytes()))
                .map_err(|err| {
                    self.err(format!(
                        "{} must be a pure constant expression: {}",
                        context, err
                    ))
                }),
        }
    }

    fn starts_const_expr(&self) -> bool {
        if self.numeric_label_ref_at(self.pos).is_some() {
            return false;
        }
        self.starts_absolute_symbol_expr()
            || matches!(
                self.peek(),
                Tok::Integer(_) | Tok::UnsignedInteger(_) | Tok::Minus | Tok::LParen
            )
    }

    fn starts_immediate_expr(&self) -> bool {
        self.peek() == &Tok::Hash || self.starts_const_expr()
    }

    fn parse_immediate_const_expr(&mut self, context: &str) -> Result<i64, ParseError> {
        self.eat(&Tok::Hash);
        self.parse_const_expr(context)
    }

    fn parse_u16_immediate(&mut self, context: &str) -> Result<u16, ParseError> {
        let start = self.pos;
        let value = self.parse_immediate_const_expr(context)?;
        u16::try_from(value).map_err(|_| {
            self.err_at(
                start,
                format!("{} must be in the range 0..=65535, got {}", context, value),
            )
        })
    }

    fn parse_signed_scaled_immediate(
        &mut self,
        context: &str,
        bits: u8,
        scale: u8,
    ) -> Result<i64, ParseError> {
        let start = self.pos;
        let value = self.parse_immediate_const_expr(context)?;
        let alignment = 1i64 << scale;
        if value % alignment != 0 {
            return Err(self.err_at(
                start,
                format!("{} {} is not {}-byte aligned", context, value, alignment),
            ));
        }

        let scaled = value / alignment;
        let min = -(1i64 << (bits - 1));
        let max = (1i64 << (bits - 1)) - 1;
        if !(min..=max).contains(&scaled) {
            return Err(self.err_at(
                start,
                format!(
                    "{} {} is out of range for a signed {}-bit immediate with scale {}",
                    context, value, bits, alignment
                ),
            ));
        }

        Ok(value)
    }

    fn parse_i32_signed_scaled_immediate(
        &mut self,
        context: &str,
        bits: u8,
        scale: u8,
    ) -> Result<i32, ParseError> {
        let value = self.parse_signed_scaled_immediate(context, bits, scale)?;
        i32::try_from(value).map_err(|_| {
            self.err(format!(
                "{} {} does not fit the assembler's offset representation",
                context, value
            ))
        })
    }

    fn parse_i16_signed_scaled_immediate(
        &mut self,
        context: &str,
        bits: u8,
        scale: u8,
    ) -> Result<i16, ParseError> {
        let start = self.pos;
        let value = self.parse_immediate_const_expr(context)?;
        self.validate_i16_signed_scaled_immediate(value, context, bits, scale, start)
    }

    fn validate_i16_signed_scaled_immediate(
        &self,
        value: i64,
        context: &str,
        bits: u8,
        scale: u8,
        start: usize,
    ) -> Result<i16, ParseError> {
        let alignment = 1i64 << scale;
        if value % alignment != 0 {
            return Err(self.err_at(
                start,
                format!("{} {} is not {}-byte aligned", context, value, alignment),
            ));
        }

        let scaled = value / alignment;
        let min = -(1i64 << (bits - 1));
        let max = (1i64 << (bits - 1)) - 1;
        if !(min..=max).contains(&scaled) {
            return Err(self.err_at(
                start,
                format!(
                    "{} {} is out of range for a signed {}-bit immediate with scale {}",
                    context, value, bits, alignment
                ),
            ));
        }

        i16::try_from(value).map_err(|_| {
            self.err_at(
                start,
                format!(
                    "{} {} does not fit the assembler's offset representation",
                    context, value
                ),
            )
        })
    }

    fn parse_logical_immediate_value(&mut self, sf: bool) -> Result<u64, ParseError> {
        let start = self.pos;
        let imm = self.parse_immediate_const_expr("logical immediate")?;
        let raw = if sf {
            imm as u64
        } else {
            let min = -i64::from(u32::MAX);
            let max = i64::from(u32::MAX);
            if !(min..=max).contains(&imm) {
                return Err(self.err_at(
                    start,
                    format!(
                        "32-bit logical immediate must be in the range {}..={}, got {}",
                        min, max, imm
                    ),
                ));
            }
            (imm as u32) as u64
        };
        if logical_immediate_encodable(raw, if sf { 64 } else { 32 }) {
            Ok(raw)
        } else {
            Err(self.err(format!(
                "immediate {:#x} is not encodable as a logical immediate",
                raw
            )))
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        Ok(self.parse_add_sub_expr(false, 0)?.expr)
    }

    fn parse_quad_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_expr_with_standalone_unsigned("standalone .quad values")
    }

    fn parse_absolute_assignment_expr(&mut self) -> Result<Expr, ParseError> {
        let expr =
            self.parse_expr_with_standalone_unsigned("standalone absolute assignment values")?;
        Ok(match expr {
            Expr::Unsigned(value) => Expr::Int(i64::from_le_bytes(value.to_le_bytes())),
            expr => expr,
        })
    }

    fn parse_expr_with_standalone_unsigned(
        &mut self,
        restriction: &str,
    ) -> Result<Expr, ParseError> {
        let start = self.pos;
        let expr = self.parse_add_sub_expr(true, 0)?.expr;
        if contains_wide_unsigned_literal(&expr) && !matches!(expr, Expr::Unsigned(_)) {
            let wide_literal = (start..self.pos)
                .find(|&pos| matches!(&self.tokens[pos].kind, Tok::UnsignedInteger(_)))
                .unwrap_or(start);
            return Err(self.err_at(
                wide_literal,
                format!(
                    "unsigned integer literals above i64::MAX must be {}",
                    restriction
                ),
            ));
        }
        Ok(expr)
    }

    fn parse_add_sub_expr(
        &mut self,
        allow_wide_unsigned: bool,
        nesting: usize,
    ) -> Result<ParsedExpr, ParseError> {
        let mut parsed = self.parse_unary_expr(allow_wide_unsigned, nesting)?;
        loop {
            let operator_pos = self.pos;
            let is_add = match self.peek() {
                Tok::Plus => true,
                Tok::Minus => false,
                _ => break,
            };
            self.advance();

            let rhs = self.parse_unary_expr(allow_wide_unsigned, nesting)?;
            let depth = parsed.depth.max(rhs.depth) + 1;
            if depth > MAX_EXPRESSION_DEPTH {
                return Err(self.expression_depth_error(operator_pos));
            }

            let expr = if is_add {
                Expr::Add(Box::new(parsed.expr), Box::new(rhs.expr))
            } else {
                Expr::Sub(Box::new(parsed.expr), Box::new(rhs.expr))
            };
            parsed = ParsedExpr { expr, depth };
        }
        Ok(parsed)
    }

    fn parse_unary_expr(
        &mut self,
        allow_wide_unsigned: bool,
        nesting: usize,
    ) -> Result<ParsedExpr, ParseError> {
        let first_minus = self.pos;
        let mut minus_count = 0;
        while self.peek() == &Tok::Minus {
            if minus_count == MAX_EXPRESSION_DEPTH {
                return Err(self.expression_depth_error(self.pos));
            }
            self.advance();
            minus_count += 1;
        }

        let signed_min_literal = minus_count > 0
            && matches!(self.peek(), Tok::UnsignedInteger(value) if *value == 1_u64 << 63);
        let parsed = if signed_min_literal {
            self.advance();
            ParsedExpr::leaf(Expr::Int(i64::MIN))
        } else {
            self.parse_primary_expr(allow_wide_unsigned, nesting)?
        };
        if parsed.depth > MAX_EXPRESSION_DEPTH - minus_count {
            let offending_minus = first_minus + (MAX_EXPRESSION_DEPTH - parsed.depth);
            return Err(self.expression_depth_error(offending_minus));
        }

        let depth = parsed.depth + minus_count;
        let mut expr = parsed.expr;
        let wrapping_minuses = minus_count - usize::from(signed_min_literal);
        for _ in 0..wrapping_minuses {
            expr = Expr::UnaryMinus(Box::new(expr));
        }
        Ok(ParsedExpr { expr, depth })
    }

    fn parse_primary_expr(
        &mut self,
        allow_wide_unsigned: bool,
        nesting: usize,
    ) -> Result<ParsedExpr, ParseError> {
        if let Some(symbol) = self.parse_numeric_label_ref()? {
            return Ok(ParsedExpr::leaf(Expr::Symbol(symbol)));
        }
        match self.peek().clone() {
            Tok::Integer(value) => {
                self.advance();
                Ok(ParsedExpr::leaf(Expr::Int(value)))
            }
            Tok::UnsignedInteger(value) if allow_wide_unsigned => {
                self.advance();
                Ok(ParsedExpr::leaf(Expr::Unsigned(value)))
            }
            Tok::UnsignedInteger(value) => Err(self.err(format!(
                "unsigned integer {} exceeds the i64 range for this context",
                value
            ))),
            Tok::Ident(symbol) => {
                self.advance();
                if self.eat(&Tok::At) {
                    let modifier = self.expect_ident()?;
                    let upper = modifier.to_ascii_uppercase();
                    match upper.as_str() {
                        "GOT" => Ok(ParsedExpr::leaf(Expr::ModifiedSymbol {
                            symbol,
                            modifier: SymbolModifier::Got,
                        })),
                        _ => Err(self.err(format!(
                            "unsupported relocation modifier '@{}' in expression",
                            modifier
                        ))),
                    }
                } else {
                    Ok(ParsedExpr::leaf(Expr::Symbol(symbol)))
                }
            }
            Tok::Dot => {
                self.advance();
                Ok(ParsedExpr::leaf(Expr::CurrentLocation))
            }
            Tok::LParen => {
                let open_paren = self.pos;
                if nesting == MAX_EXPRESSION_DEPTH {
                    return Err(self.expression_depth_error(open_paren));
                }
                self.advance();
                let expr = self.parse_add_sub_expr(allow_wide_unsigned, nesting + 1)?;
                self.expect(&Tok::RParen)?;
                Ok(expr)
            }
            other => Err(self.err(format!("expected expression, got {}", other))),
        }
    }

    fn numeric_label_definition_number(&self) -> Option<u32> {
        match self.peek() {
            Tok::Integer(value)
                if *value >= 0 && matches!(&self.token_at(self.pos + 1).kind, Tok::Colon) =>
            {
                u32::try_from(*value).ok()
            }
            _ => None,
        }
    }

    fn numeric_label_ref_at(&self, pos: usize) -> Option<(u32, NumericLabelDirection)> {
        let int_tok = self.token_at(pos);
        let value = match &int_tok.kind {
            Tok::Integer(value) if *value >= 0 => u32::try_from(*value).ok()?,
            _ => return None,
        };
        let suffix_tok = self.token_at(pos + 1);
        if suffix_tok.line != int_tok.line {
            return None;
        }
        if suffix_tok.col != int_tok.col + decimal_width(value) {
            return None;
        }
        match &suffix_tok.kind {
            Tok::Ident(suffix) if suffix.eq_ignore_ascii_case("f") => {
                Some((value, NumericLabelDirection::Forward))
            }
            Tok::Ident(suffix) if suffix.eq_ignore_ascii_case("b") => {
                Some((value, NumericLabelDirection::Backward))
            }
            _ => None,
        }
    }

    fn parse_numeric_label_ref(&mut self) -> Result<Option<String>, ParseError> {
        let Some((number, direction)) = self.numeric_label_ref_at(self.pos) else {
            return Ok(None);
        };
        self.advance();
        self.advance();
        Ok(Some(self.resolve_numeric_label_ref(number, direction)?))
    }

    fn parse_label_reference(&mut self) -> Result<String, ParseError> {
        if let Some(symbol) = self.parse_numeric_label_ref()? {
            Ok(symbol)
        } else {
            self.expect_ident()
        }
    }

    fn parse_branch_label_reference(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        let symbol = self.parse_label_reference()?;
        if is_canonical_register_name(&symbol) {
            Err(self.err_at(
                start,
                format!("expected label, got architectural register '{}'", symbol),
            ))
        } else {
            Ok(symbol)
        }
    }

    fn parse_symbol_reloc_modifier(
        &mut self,
        default: Option<RelocKind>,
        allowed: &[(&str, RelocKind)],
        context: &str,
    ) -> Result<RelocKind, ParseError> {
        if !self.eat(&Tok::At) {
            return default
                .ok_or_else(|| self.err(format!("{} requires a relocation modifier", context)));
        }

        let modifier = self.expect_ident()?;
        let upper = modifier.to_ascii_uppercase();
        for (name, kind) in allowed {
            if upper == *name {
                return Ok(*kind);
            }
        }

        Err(self.err(format!(
            "unsupported relocation modifier '@{}' for {}",
            modifier, context
        )))
    }

    fn parse_optional_symbol_addend(&mut self) -> Result<i64, ParseError> {
        if self.eat(&Tok::Plus) {
            self.parse_const_expr("symbol addend")
        } else if self.eat(&Tok::Minus) {
            Ok(-self.parse_const_expr("symbol addend")?)
        } else {
            Ok(0)
        }
    }

    fn starts_non_register_symbol_reference(&self) -> bool {
        if self.numeric_label_ref_at(self.pos).is_some() {
            return true;
        }
        if self.starts_absolute_symbol_expr() {
            return false;
        }
        match self.peek() {
            Tok::Ident(name) => {
                let has_reloc_modifier = matches!(
                    self.tokens.get(self.pos + 1).map(|token| &token.kind),
                    Some(Tok::At)
                );
                !is_canonical_register_name(name)
                    && (!looks_like_gp_register_name(name) || has_reloc_modifier)
            }
            _ => false,
        }
    }

    fn starts_non_register_literal_reference(&self) -> bool {
        if self.numeric_label_ref_at(self.pos).is_some() {
            return true;
        }
        if self.starts_absolute_symbol_expr() {
            return false;
        }
        match self.peek() {
            Tok::Ident(name) => !is_canonical_register_name(name),
            _ => false,
        }
    }

    fn define_numeric_label(&mut self, number: u32) -> String {
        let ordinal = self.numeric_labels.entry(number).or_insert(0);
        *ordinal += 1;
        numeric_label_symbol(number, *ordinal)
    }

    fn resolve_numeric_label_ref(
        &self,
        number: u32,
        direction: NumericLabelDirection,
    ) -> Result<String, ParseError> {
        let current = self.numeric_labels.get(&number).copied().unwrap_or(0);
        match direction {
            NumericLabelDirection::Forward => Ok(numeric_label_symbol(number, current + 1)),
            NumericLabelDirection::Backward if current > 0 => {
                Ok(numeric_label_symbol(number, current))
            }
            NumericLabelDirection::Backward => Err(self.err(format!(
                "numeric label '{}' has no previous definition",
                number
            ))),
        }
    }

    fn parse_instruction(&mut self, mnemonic: &str) -> Result<Stmt, ParseError> {
        // ADRP and ADD-with-label return Stmt directly (may carry relocation info).
        if mnemonic == "adrp" {
            return self.parse_adrp();
        }
        if mnemonic == "add" {
            return self.parse_add_sub_stmt(false, false);
        }
        if mnemonic == "b" {
            return self.parse_b();
        }
        if mnemonic == "bl" {
            return self.parse_bl();
        }
        if mnemonic == "cbz" {
            return self.parse_cbz(false);
        }
        if mnemonic == "cbnz" {
            return self.parse_cbz(true);
        }
        if mnemonic == "tbz" {
            return self.parse_tbz(false);
        }
        if mnemonic == "tbnz" {
            return self.parse_tbz(true);
        }
        if mnemonic == "adr" {
            return self.parse_adr();
        }
        if mnemonic == "ldr" {
            return self.parse_ldr_str(true);
        }
        if mnemonic == "str" {
            return self.parse_ldr_str(false);
        }
        if mnemonic == "ldur" {
            return self.parse_ldur_stur(true);
        }
        if mnemonic == "stur" {
            return self.parse_ldur_stur(false);
        }
        if mnemonic == "ldrsw" {
            return self.parse_ldrsw();
        }
        if mnemonic == "ldaprb" {
            return self.parse_ldapr_narrow("ldaprb");
        }
        if mnemonic == "ldaprh" {
            return self.parse_ldapr_narrow("ldaprh");
        }
        if mnemonic == "ldapr" {
            return self.parse_ldapr();
        }
        if mnemonic == "stlrb" {
            return self.parse_stlr_narrow("stlrb");
        }
        if mnemonic == "stlrh" {
            return self.parse_stlr_narrow("stlrh");
        }
        if mnemonic == "stlr" {
            return self.parse_stlr();
        }
        if mnemonic == "ldaddalb" {
            return self.parse_atomic_rmw_narrow("ldaddalb");
        }
        if mnemonic == "ldaddalh" {
            return self.parse_atomic_rmw_narrow("ldaddalh");
        }
        if mnemonic == "ldaddal" {
            return self.parse_ldaddal();
        }
        if mnemonic == "ldumaxalb" {
            return self.parse_atomic_rmw_narrow("ldumaxalb");
        }
        if mnemonic == "ldumaxalh" {
            return self.parse_atomic_rmw_narrow("ldumaxalh");
        }
        if mnemonic == "ldumaxal" {
            return self.parse_atomic_rmw("ldumaxal");
        }
        if mnemonic == "ldsmaxalb" {
            return self.parse_atomic_rmw_narrow("ldsmaxalb");
        }
        if mnemonic == "ldsmaxalh" {
            return self.parse_atomic_rmw_narrow("ldsmaxalh");
        }
        if mnemonic == "ldsmaxal" {
            return self.parse_ldsmaxal();
        }
        if mnemonic == "lduminalb" {
            return self.parse_atomic_rmw_narrow("lduminalb");
        }
        if mnemonic == "lduminalh" {
            return self.parse_atomic_rmw_narrow("lduminalh");
        }
        if mnemonic == "lduminal" {
            return self.parse_atomic_rmw("lduminal");
        }
        if mnemonic == "ldsminalb" {
            return self.parse_atomic_rmw_narrow("ldsminalb");
        }
        if mnemonic == "ldsminalh" {
            return self.parse_atomic_rmw_narrow("ldsminalh");
        }
        if mnemonic == "ldsminal" {
            return self.parse_ldsminal();
        }
        if mnemonic == "ldclralb" {
            return self.parse_atomic_rmw_narrow("ldclralb");
        }
        if mnemonic == "ldclralh" {
            return self.parse_atomic_rmw_narrow("ldclralh");
        }
        if mnemonic == "ldclral" {
            return self.parse_ldclral();
        }
        if mnemonic == "ldeoralb" {
            return self.parse_atomic_rmw_narrow("ldeoralb");
        }
        if mnemonic == "ldeoralh" {
            return self.parse_atomic_rmw_narrow("ldeoralh");
        }
        if mnemonic == "ldeoral" {
            return self.parse_ldeoral();
        }
        if mnemonic == "ldsetalb" {
            return self.parse_atomic_rmw_narrow("ldsetalb");
        }
        if mnemonic == "ldsetalh" {
            return self.parse_atomic_rmw_narrow("ldsetalh");
        }
        if mnemonic == "ldsetal" {
            return self.parse_ldsetal();
        }
        if mnemonic == "swpalb" {
            return self.parse_atomic_rmw_narrow("swpalb");
        }
        if mnemonic == "swpalh" {
            return self.parse_atomic_rmw_narrow("swpalh");
        }
        if mnemonic == "swpal" {
            return self.parse_swpal();
        }
        if mnemonic == "casalb" {
            return self.parse_atomic_rmw_narrow("casalb");
        }
        if mnemonic == "casalh" {
            return self.parse_atomic_rmw_narrow("casalh");
        }
        if mnemonic == "casal" {
            return self.parse_casal();
        }
        if let Some(cond) = mnemonic.strip_prefix("b.") {
            return self.parse_bcond(cond);
        }

        // All other instructions return Inst, wrapped as Stmt::Instruction.
        let inst = match mnemonic {
            // ---- Data processing (register + immediate) ----
            "sub" => self.parse_add_sub(true, false),
            "adds" => self.parse_add_sub(false, true),
            "subs" => self.parse_add_sub(true, true),
            "cmp" => self.parse_cmp(),
            "cmn" => self.parse_cmn(),
            "mul" => self.parse_3reg("mul"),
            "umull" => self.parse_umull(),
            "madd" => self.parse_madd(),
            "msub" => self.parse_madd_sub("msub"),
            "sdiv" => self.parse_3reg("sdiv"),
            "udiv" => self.parse_3reg("udiv"),

            // Logic
            "and" => self.parse_logic("and"),
            "and.16b" => self.parse_simd_logic_16b("and.16b"),
            "bic.16b" => self.parse_simd_logic_16b("bic.16b"),
            "bif.16b" => self.parse_simd_logic_16b("bif.16b"),
            "bit.16b" => self.parse_simd_logic_16b("bit.16b"),
            "bsl.16b" => self.parse_simd_logic_16b("bsl.16b"),
            "orr" => self.parse_logic("orr"),
            "orr.16b" => self.parse_simd_logic_16b("orr.16b"),
            "eor" => self.parse_logic("eor"),
            "eor.16b" => self.parse_simd_logic_16b("eor.16b"),
            "ands" => self.parse_logic("ands"),
            "neg" => self.parse_neg(),
            "mvn" => self.parse_mvn(),
            "tst" => self.parse_tst(),

            // Move
            "csel" => self.parse_csel(),
            "csinc" => self.parse_csinc(),
            "csinv" => self.parse_csinv(),
            "csneg" => self.parse_csneg(),
            "ccmp" => self.parse_ccmp(),
            "ccmn" => self.parse_ccmn(),
            "cset" => self.parse_cset(),
            "csetm" => self.parse_csetm(),
            "cinc" => self.parse_cinc(),
            "cinv" => self.parse_cinv(),
            "cneg" => self.parse_cneg(),
            "mov" => self.parse_mov(),
            "mov.s" | "mov.d" | "mov.h" | "mov.b" => self.parse_simd_lane_insert(mnemonic),
            "umov.h" | "umov.b" => self.parse_simd_lane_extract_gp(mnemonic),
            "smov.h" | "smov.b" => self.parse_simd_lane_extract_gp_signed(mnemonic),
            "ld1.s" | "ld1.d" | "ld1.h" | "ld1.b" => self.parse_simd_lane_load(mnemonic),
            "mov.8b" | "mov.16b" | "mov.4s" | "mov.2d" => self.parse_simd_mov(mnemonic),
            "dup.16b" | "dup.8h" | "dup.4s" | "dup.2d" => self.parse_simd_dup(mnemonic),
            "tbl.16b" => self.parse_simd_table_lookup("tbl.16b"),
            "tbx.16b" => self.parse_simd_table_lookup("tbx.16b"),
            "ext.16b" => self.parse_simd_ext_16b(),
            "rev64.4s" => self.parse_simd_rev64_4s(),
            "zip1.4s" | "zip2.4s" | "uzp1.4s" | "uzp2.4s" | "trn1.4s" | "trn2.4s" => {
                self.parse_simd_shuffle_4s(mnemonic)
            }
            "zip1.2d" | "zip2.2d" | "uzp1.2d" | "uzp2.2d" | "trn1.2d" | "trn2.2d" => {
                self.parse_simd_shuffle_2d(mnemonic)
            }
            "movz" => self.parse_mov_wide("movz"),
            "movk" => self.parse_mov_wide("movk"),
            "movn" => self.parse_mov_wide("movn"),

            // Shifts
            "lsl" => self.parse_shift("lsl"),
            "lsr" => self.parse_shift("lsr"),
            "asr" => self.parse_shift("asr"),
            "sxtw" => self.parse_extend_alias("sxtw"),
            "sxtb" => self.parse_extend_alias("sxtb"),
            "sxth" => self.parse_extend_alias("sxth"),
            "uxtw" => self.parse_extend_alias("uxtw"),
            "ubfiz" => self.parse_bitfield_alias("ubfiz"),
            "bfi" => self.parse_bitfield_alias("bfi"),
            "bfxil" => self.parse_bitfield_alias("bfxil"),

            // Branches
            "ret" => self.parse_ret(),
            "br" => {
                let rn = self.parse_x_reg("br")?;
                Ok(Inst::Br { rn })
            }
            "blr" => {
                let rn = self.parse_x_reg("blr")?;
                Ok(Inst::Blr { rn })
            }

            // Load/store
            "ldrb" => self.parse_ldstb_h("ldrb"),
            "ldrsb" => self.parse_ldst_signed_b_h("ldrsb"),
            "ldrh" => self.parse_ldstb_h("ldrh"),
            "ldrsh" => self.parse_ldst_signed_b_h("ldrsh"),
            "strb" => self.parse_ldstb_h("strb"),
            "strh" => self.parse_ldstb_h("strh"),
            "stp" => self.parse_ldp_stp(false),
            "ldp" => self.parse_ldp_stp(true),

            // FP arithmetic (double)
            "fadd" => self.parse_fp_arith("fadd"),
            "addp.16b" | "smaxp.16b" | "sminp.16b" | "umaxp.16b" | "uminp.16b" => {
                self.parse_simd_int_arith_16b(mnemonic)
            }
            "addp.2d" => self.parse_simd_int_arith_2d(mnemonic),
            "addp.8h" | "smaxp.8h" | "sminp.8h" | "umaxp.8h" | "uminp.8h" => {
                self.parse_simd_int_arith_8h(mnemonic)
            }
            "add.4s" | "addp.4s" | "sub.4s" | "smax.4s" | "smaxp.4s" | "smin.4s" | "sminp.4s"
            | "umax.4s" | "umaxp.4s" | "umin.4s" | "uminp.4s" => {
                self.parse_simd_int_arith_4s(mnemonic)
            }
            "addv.16b" | "umaxv.16b" | "smaxv.16b" | "uminv.16b" | "sminv.16b" => {
                self.parse_simd_reduce_16b(mnemonic)
            }
            "addv.8h" | "umaxv.8h" | "smaxv.8h" | "uminv.8h" | "sminv.8h" => {
                self.parse_simd_reduce_8h(mnemonic)
            }
            "addv.4s" | "faddp.2s" | "fmaxv.4s" | "fminv.4s" | "fmaxnmv.4s" | "fminnmv.4s"
            | "smaxv.4s" | "umaxv.4s" | "sminv.4s" | "uminv.4s" => {
                self.parse_simd_reduce_4s(mnemonic)
            }
            "cmeq.4s" | "cmhs.4s" | "cmhi.4s" | "cmge.4s" | "cmgt.4s" | "fcmeq.4s" | "fcmge.4s"
            | "fcmgt.4s" => self.parse_simd_compare_4s(mnemonic),
            "fcmeq.2d" | "fcmge.2d" | "fcmgt.2d" | "fcmle.2d" | "fcmlt.2d" => {
                self.parse_simd_compare_2d(mnemonic)
            }
            "fadd.4s" | "faddp.4s" | "fmaxp.4s" | "fminp.4s" | "fmaxnmp.4s" | "fminnmp.4s"
            | "fmla.4s" | "fmls.4s" | "fsub.4s" | "fmul.4s" | "fdiv.4s" | "fmax.4s" | "fmin.4s"
            | "fmaxnm.4s" | "fminnm.4s" | "frecps.4s" | "frsqrts.4s" => {
                self.parse_simd_fp_arith_4s(mnemonic)
            }
            "fadd.2d" | "fsub.2d" | "fmul.2d" | "fdiv.2d" | "fabd.2d" | "fmla.2d" | "fmls.2d"
            | "fmax.2d" | "fmin.2d" | "fmaxnm.2d" | "fminnm.2d" | "frecps.2d" | "frsqrts.2d" => {
                self.parse_simd_fp_arith_2d(mnemonic)
            }
            "faddp.2d" | "fmaxp.2d" | "fminp.2d" | "fmaxnmp.2d" | "fminnmp.2d" => {
                self.parse_fpairwise_or_reduce_2d(mnemonic)
            }
            "fabs.4s" | "fneg.4s" | "fsqrt.4s" | "scvtf.4s" | "ucvtf.4s" | "fcvtzs.4s"
            | "fcvtzu.4s" | "frecpe.4s" | "frsqrte.4s" | "frintn.4s" | "frintm.4s"
            | "frintp.4s" | "frintz.4s" | "frinta.4s" | "frinti.4s" => {
                self.parse_simd_fp_unary_4s(mnemonic)
            }
            "fabs.2d" | "fneg.2d" | "fsqrt.2d" | "scvtf.2d" | "ucvtf.2d" | "fcvtzs.2d"
            | "fcvtzu.2d" | "frecpe.2d" | "frsqrte.2d" | "frintn.2d" | "frintm.2d"
            | "frintp.2d" | "frintz.2d" | "frinta.2d" | "frinti.2d" => {
                self.parse_simd_fp_unary_2d(mnemonic)
            }
            "fsub" => self.parse_fp_arith("fsub"),
            "fmul" => self.parse_fp_arith("fmul"),
            "fdiv" => self.parse_fp_arith("fdiv"),
            "fneg" => self.parse_fp_unary("fneg"),
            "fabs" => self.parse_fp_unary("fabs"),
            "fsqrt" => self.parse_fp_unary("fsqrt"),
            "fcmp" => self.parse_fcmp(),
            "fcsel" => self.parse_fcsel(),
            "fmadd" => self.parse_fmadd(),

            // FP conversion
            "fcvt" => self.parse_fcvt(),
            "fcvtzs" => self.parse_fcvtzs(),
            "scvtf" => self.parse_scvtf(),
            "fmov" => self.parse_fmov(),

            // System
            "svc" => {
                let imm = self.parse_u16_immediate("svc immediate")?;
                Ok(Inst::Svc { imm16: imm })
            }
            "nop" => Ok(Inst::Nop),
            "yield" => Ok(Inst::Yield),
            "wfe" => Ok(Inst::Wfe),
            "wfi" => Ok(Inst::Wfi),
            "sev" => Ok(Inst::Sev),
            "sevl" => Ok(Inst::Sevl),
            "dmb" => Ok(Inst::Dmb {
                option: self.parse_barrier_option("dmb option")?,
            }),
            "dsb" => Ok(Inst::Dsb {
                option: self.parse_barrier_option("dsb option")?,
            }),
            "isb" => {
                let option = if self.at_end_of_stmt() {
                    BarrierOpt::Sy
                } else {
                    self.parse_barrier_option("isb option")?
                };
                Ok(Inst::Isb { option })
            }
            "brk" => {
                let imm = self.parse_u16_immediate("brk immediate")?;
                Ok(Inst::Brk { imm16: imm })
            }

            _ => Err(self.err(format!("unknown mnemonic: {}", mnemonic))),
        }?;
        Ok(Stmt::Instruction(inst))
    }

    // ---- Register parsing helpers ----

    fn parse_gp_reg_with_required_width(
        &mut self,
        required_64bit: bool,
        context: &str,
    ) -> Result<GpReg, ParseError> {
        let start = self.pos;
        let (reg, is_64bit) = self.parse_gp_data_reg_with_size(context)?;
        if is_64bit != required_64bit {
            let register = if required_64bit {
                "an x-register"
            } else {
                "a w-register"
            };
            return Err(self.err_at(start, format!("{} requires {}", context, register)));
        }
        Ok(reg)
    }

    fn parse_x_reg(&mut self, context: &str) -> Result<GpReg, ParseError> {
        self.parse_gp_reg_with_required_width(true, context)
    }

    fn parse_w_reg(&mut self, context: &str) -> Result<GpReg, ParseError> {
        self.parse_gp_reg_with_required_width(false, context)
    }

    fn parse_memory_base_reg(&mut self, context: &str) -> Result<GpReg, ParseError> {
        let start = self.pos;
        let (reg, is_64bit, kind) = self.parse_gp_reg_with_size_kind()?;
        if !is_64bit || kind == GpRegKind::Zr {
            return Err(self.err_at(
                start,
                format!("{} requires an x-register or sp base", context),
            ));
        }
        Ok(reg)
    }

    fn parse_gp_data_reg_with_size(&mut self, context: &str) -> Result<(GpReg, bool), ParseError> {
        let start = self.pos;
        let (reg, is_64bit, kind) = self.parse_gp_reg_with_size_kind()?;
        if kind == GpRegKind::Sp {
            return Err(self.err_at(
                start,
                format!("{} does not allow sp as a data register", context),
            ));
        }
        Ok((reg, is_64bit))
    }

    fn parse_gp_reg_matching_width(
        &mut self,
        expected_64bit: bool,
        context: &str,
    ) -> Result<GpReg, ParseError> {
        let start = self.pos;
        let (reg, is_64bit) = self.parse_gp_data_reg_with_size(context)?;
        if is_64bit != expected_64bit {
            return Err(self.err_at(
                start,
                format!("{} requires registers of the same width", context),
            ));
        }
        Ok(reg)
    }

    fn parse_gp_reg_with_size_kind(&mut self) -> Result<(GpReg, bool, GpRegKind), ParseError> {
        let start = self.pos;
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        self.gp_reg_with_size_kind_from_name(&name, &lower, start)
    }

    fn parse_add_sub_gp_reg_with_size_kind(
        &mut self,
    ) -> Result<(GpReg, bool, GpRegKind), ParseError> {
        self.parse_gp_reg_with_size_kind()
    }

    fn gp_reg_with_size_kind_from_name(
        &self,
        name: &str,
        lower: &str,
        start: usize,
    ) -> Result<(GpReg, bool, GpRegKind), ParseError> {
        if let Some(register) = classify_gp_reg_name(lower) {
            return Ok(register);
        }

        if lower.starts_with('x') || lower.starts_with('w') {
            Err(self.err_at(start, format!("bad register '{}'", name)))
        } else {
            Err(self.err_at(start, format!("expected GP register, got '{}'", name)))
        }
    }

    fn parse_lane_gp_reg(
        &mut self,
        width: SimdLaneWidth,
        context: &str,
    ) -> Result<GpReg, ParseError> {
        let start = self.pos;
        let (reg, is_64bit, kind) = self.parse_gp_reg_with_size_kind()?;
        if kind == GpRegKind::Sp {
            return Err(self.err_at(start, format!("{} does not allow SP", context)));
        }
        let needs_64bit = matches!(width, SimdLaneWidth::D64);
        if needs_64bit != is_64bit {
            let width_name = if needs_64bit {
                "x-register"
            } else {
                "w-register"
            };
            return Err(self.err_at(start, format!("{} requires a {}", context, width_name)));
        }
        Ok(reg)
    }

    fn parse_fp_reg_with_size(&mut self) -> Result<(FpReg, bool), ParseError> {
        let start = self.pos;
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        if lower.starts_with('d') {
            let reg = parse_fp_reg_name(&lower)
                .ok_or_else(|| self.err_at(start, format!("bad FP register '{}'", name)))?;
            Ok((reg, true)) // double
        } else if lower.starts_with('s') {
            let reg = parse_fp_reg_name(&lower)
                .ok_or_else(|| self.err_at(start, format!("bad FP register '{}'", name)))?;
            Ok((reg, false)) // single
        } else {
            Err(self.err_at(start, format!("expected FP register, got '{}'", name)))
        }
    }

    fn parse_fp_reg_matching_width(
        &mut self,
        expected_double: bool,
        context: &str,
    ) -> Result<FpReg, ParseError> {
        let start = self.pos;
        let (reg, is_double) = self.parse_fp_reg_with_size()?;
        if is_double != expected_double {
            return Err(self.err_at(
                start,
                format!("{} requires registers of the same width", context),
            ));
        }
        Ok(reg)
    }

    fn parse_fp_mem_reg_with_width(&mut self) -> Result<(FpReg, FpMemWidth), ParseError> {
        let start = self.pos;
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        let width = if lower.starts_with('b') {
            FpMemWidth::B8
        } else if lower.starts_with('h') {
            FpMemWidth::H16
        } else if lower.starts_with('d') {
            FpMemWidth::D64
        } else if lower.starts_with('s') {
            FpMemWidth::S32
        } else if lower.starts_with('q') {
            FpMemWidth::Q128
        } else {
            return Err(self.err_at(start, format!("expected FP/SIMD register, got '{}'", name)));
        };
        let num = parse_canonical_register_number(&lower[1..])
            .ok_or_else(|| self.err_at(start, format!("bad FP/SIMD register '{}'", name)))?;
        if num > 31 {
            return Err(self.err_at(start, format!("bad FP/SIMD register '{}'", name)));
        }
        let reg = FpReg::new(num);
        Ok((reg, width))
    }

    fn parse_atomic_data_reg(&mut self, context: &str) -> Result<(GpReg, bool), ParseError> {
        let start = self.pos;
        let (reg, sf, kind) = self.parse_gp_reg_with_size_kind()?;
        if kind == GpRegKind::Sp {
            return Err(self.err_at(
                start,
                format!("{} does not allow SP as a data register", context),
            ));
        }
        Ok((reg, sf))
    }

    fn parse_atomic_wreg(&mut self, context: &str) -> Result<GpReg, ParseError> {
        let start = self.pos;
        let (reg, sf) = self.parse_atomic_data_reg(context)?;
        if sf {
            return Err(self.err_at(start, format!("{} requires a w-register", context)));
        }
        Ok(reg)
    }

    fn parse_atomic_base_reg(&mut self, context: &str) -> Result<GpReg, ParseError> {
        self.expect(&Tok::LBracket)?;
        let start = self.pos;
        let (rn, sf, kind) = self.parse_gp_reg_with_size_kind()?;
        if !sf || kind == GpRegKind::Zr {
            return Err(self.err_at(
                start,
                format!("{} expects an [Xn] or [sp] base register", context),
            ));
        }
        if !self.eat(&Tok::RBracket) {
            return Err(self.err(format!("{} expects a simple [Xn] memory operand", context)));
        }
        if self.eat(&Tok::Bang) {
            return Err(self.err(format!("{} does not support pre-index addressing", context)));
        }
        if self.eat(&Tok::Comma) {
            return Err(self.err(format!(
                "{} does not support post-index or offset addressing",
                context
            )));
        }
        Ok(rn)
    }

    // ---- Instruction-specific parsers ----

    fn parse_ldapr(&mut self) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_atomic_data_reg("ldapr")?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_atomic_base_reg("ldapr")?;
        Ok(Stmt::Instruction(if sf {
            Inst::Ldapr64 { rt, rn }
        } else {
            Inst::Ldapr32 { rt, rn }
        }))
    }

    fn parse_ldapr_narrow(&mut self, mnemonic: &str) -> Result<Stmt, ParseError> {
        let rt = self.parse_atomic_wreg(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_atomic_base_reg(mnemonic)?;
        Ok(Stmt::Instruction(match mnemonic {
            "ldaprb" => Inst::Ldaprb { rt, rn },
            "ldaprh" => Inst::Ldaprh { rt, rn },
            _ => unreachable!(),
        }))
    }

    fn parse_stlr(&mut self) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_atomic_data_reg("stlr")?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_atomic_base_reg("stlr")?;
        Ok(Stmt::Instruction(if sf {
            Inst::Stlr64 { rt, rn }
        } else {
            Inst::Stlr32 { rt, rn }
        }))
    }

    fn parse_stlr_narrow(&mut self, mnemonic: &str) -> Result<Stmt, ParseError> {
        let rt = self.parse_atomic_wreg(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_atomic_base_reg(mnemonic)?;
        Ok(Stmt::Instruction(match mnemonic {
            "stlrb" => Inst::Stlrb { rt, rn },
            "stlrh" => Inst::Stlrh { rt, rn },
            _ => unreachable!(),
        }))
    }

    fn parse_atomic_rmw(&mut self, mnemonic: &str) -> Result<Stmt, ParseError> {
        let (rs, sf) = self.parse_atomic_data_reg(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rt_start = self.pos;
        let (rt, rt_sf) = self.parse_atomic_data_reg(mnemonic)?;
        if sf != rt_sf {
            return Err(self.err_at(
                rt_start,
                format!(
                    "{} requires source and destination registers of the same width",
                    mnemonic
                ),
            ));
        }
        self.expect(&Tok::Comma)?;
        let rn = self.parse_atomic_base_reg(mnemonic)?;
        Ok(Stmt::Instruction(match (mnemonic, sf) {
            ("ldaddal", true) => Inst::Ldaddal64 { rs, rt, rn },
            ("ldaddal", false) => Inst::Ldaddal32 { rs, rt, rn },
            ("ldumaxal", true) => Inst::Ldumaxal64 { rs, rt, rn },
            ("ldumaxal", false) => Inst::Ldumaxal32 { rs, rt, rn },
            ("ldsmaxal", true) => Inst::Ldsmaxal64 { rs, rt, rn },
            ("ldsmaxal", false) => Inst::Ldsmaxal32 { rs, rt, rn },
            ("lduminal", true) => Inst::Lduminal64 { rs, rt, rn },
            ("lduminal", false) => Inst::Lduminal32 { rs, rt, rn },
            ("ldsminal", true) => Inst::Ldsminal64 { rs, rt, rn },
            ("ldsminal", false) => Inst::Ldsminal32 { rs, rt, rn },
            ("ldclral", true) => Inst::Ldclral64 { rs, rt, rn },
            ("ldclral", false) => Inst::Ldclral32 { rs, rt, rn },
            ("ldeoral", true) => Inst::Ldeoral64 { rs, rt, rn },
            ("ldeoral", false) => Inst::Ldeoral32 { rs, rt, rn },
            ("ldsetal", true) => Inst::Ldsetal64 { rs, rt, rn },
            ("ldsetal", false) => Inst::Ldsetal32 { rs, rt, rn },
            ("swpal", true) => Inst::Swpal64 { rs, rt, rn },
            ("swpal", false) => Inst::Swpal32 { rs, rt, rn },
            ("casal", true) => Inst::Casal64 { rs, rt, rn },
            ("casal", false) => Inst::Casal32 { rs, rt, rn },
            _ => unreachable!(),
        }))
    }

    fn parse_atomic_rmw_narrow(&mut self, mnemonic: &str) -> Result<Stmt, ParseError> {
        let rs = self.parse_atomic_wreg(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rt = self.parse_atomic_wreg(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_atomic_base_reg(mnemonic)?;
        Ok(Stmt::Instruction(match mnemonic {
            "ldaddalb" => Inst::Ldaddalb { rs, rt, rn },
            "ldaddalh" => Inst::Ldaddalh { rs, rt, rn },
            "ldumaxalb" => Inst::Ldumaxalb { rs, rt, rn },
            "ldumaxalh" => Inst::Ldumaxalh { rs, rt, rn },
            "ldsmaxalb" => Inst::Ldsmaxalb { rs, rt, rn },
            "ldsmaxalh" => Inst::Ldsmaxalh { rs, rt, rn },
            "lduminalb" => Inst::Lduminalb { rs, rt, rn },
            "lduminalh" => Inst::Lduminalh { rs, rt, rn },
            "ldsminalb" => Inst::Ldsminalb { rs, rt, rn },
            "ldsminalh" => Inst::Ldsminalh { rs, rt, rn },
            "ldclralb" => Inst::Ldclralb { rs, rt, rn },
            "ldclralh" => Inst::Ldclralh { rs, rt, rn },
            "ldeoralb" => Inst::Ldeoralb { rs, rt, rn },
            "ldeoralh" => Inst::Ldeoralh { rs, rt, rn },
            "ldsetalb" => Inst::Ldsetalb { rs, rt, rn },
            "ldsetalh" => Inst::Ldsetalh { rs, rt, rn },
            "swpalb" => Inst::Swpalb { rs, rt, rn },
            "swpalh" => Inst::Swpalh { rs, rt, rn },
            "casalb" => Inst::Casalb { rs, rt, rn },
            "casalh" => Inst::Casalh { rs, rt, rn },
            _ => unreachable!(),
        }))
    }

    fn parse_ldaddal(&mut self) -> Result<Stmt, ParseError> {
        self.parse_atomic_rmw("ldaddal")
    }

    fn parse_ldsmaxal(&mut self) -> Result<Stmt, ParseError> {
        self.parse_atomic_rmw("ldsmaxal")
    }

    fn parse_ldsminal(&mut self) -> Result<Stmt, ParseError> {
        self.parse_atomic_rmw("ldsminal")
    }

    fn parse_ldclral(&mut self) -> Result<Stmt, ParseError> {
        self.parse_atomic_rmw("ldclral")
    }

    fn parse_ldeoral(&mut self) -> Result<Stmt, ParseError> {
        self.parse_atomic_rmw("ldeoral")
    }

    fn parse_ldsetal(&mut self) -> Result<Stmt, ParseError> {
        self.parse_atomic_rmw("ldsetal")
    }

    fn parse_swpal(&mut self) -> Result<Stmt, ParseError> {
        self.parse_atomic_rmw("swpal")
    }

    fn parse_casal(&mut self) -> Result<Stmt, ParseError> {
        self.parse_atomic_rmw("casal")
    }

    fn parse_add_sub(&mut self, is_sub: bool, sets_flags: bool) -> Result<Inst, ParseError> {
        let rd_start = self.pos;
        let rd = self.parse_add_sub_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        let rn_start = self.pos;
        let rn = self.parse_add_sub_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        self.parse_add_sub_operand(rd, rn, rd_start, rn_start, is_sub, sets_flags)
    }

    /// ADD that can also handle label@PAGEOFF references (for ADRP+ADD pairs).
    fn parse_add_sub_stmt(&mut self, is_sub: bool, sets_flags: bool) -> Result<Stmt, ParseError> {
        let rd_start = self.pos;
        let rd_operand = self.parse_add_sub_gp_reg_with_size_kind()?;
        let (rd, sf, rd_kind) = rd_operand;
        self.expect(&Tok::Comma)?;
        let rn_start = self.pos;
        let rn_operand = self.parse_add_sub_gp_reg_with_size_kind()?;
        let (rn, rn_is_64bit, rn_kind) = rn_operand;
        self.expect(&Tok::Comma)?;

        // Check for label@PAGEOFF (identifier or numeric local reference followed by @).
        if self.starts_non_register_symbol_reference() {
            self.validate_add_sub_immediate_registers(
                GpOperandMeta::new(sf, rd_kind, rd_start),
                GpOperandMeta::new(rn_is_64bit, rn_kind, rn_start),
                sets_flags,
            )?;
            let label = self.parse_label_reference()?;
            let kind = self.parse_symbol_reloc_modifier(
                Some(RelocKind::PageOff12),
                &[("PAGEOFF", RelocKind::PageOff12)],
                "add/sub symbol operand",
            )?;
            let addend = self.parse_optional_symbol_addend()?;
            let inst = Inst::AddImm {
                rd,
                rn,
                imm12: 0,
                shift: false,
                sf,
            };
            return Ok(Stmt::InstructionWithReloc(
                inst,
                LabelRef {
                    symbol: label,
                    kind,
                    addend,
                },
            ));
        }

        // Normal add/sub (immediate or register).
        let inst = self.parse_add_sub_operand(
            rd_operand, rn_operand, rd_start, rn_start, is_sub, sets_flags,
        )?;
        Ok(Stmt::Instruction(inst))
    }

    /// Parse the third operand of add/sub (immediate or register).
    fn parse_add_sub_operand(
        &mut self,
        rd: (GpReg, bool, GpRegKind),
        rn: (GpReg, bool, GpRegKind),
        rd_start: usize,
        rn_start: usize,
        is_sub: bool,
        sets_flags: bool,
    ) -> Result<Inst, ParseError> {
        let (rd, sf, rd_kind) = rd;
        let (rn, rn_is_64bit, rn_kind) = rn;
        let rd_meta = GpOperandMeta::new(sf, rd_kind, rd_start);
        let rn_meta = GpOperandMeta::new(rn_is_64bit, rn_kind, rn_start);
        if self.starts_immediate_expr() {
            self.validate_add_sub_immediate_registers(rd_meta, rn_meta, sets_flags)?;
            let (imm, shift, negate_operation) =
                self.parse_add_sub_immediate("add/sub immediate")?;
            let is_sub = is_sub ^ negate_operation;
            Ok(match (is_sub, sets_flags) {
                (false, false) => Inst::AddImm {
                    rd,
                    rn,
                    imm12: imm,
                    shift,
                    sf,
                },
                (true, false) => Inst::SubImm {
                    rd,
                    rn,
                    imm12: imm,
                    shift,
                    sf,
                },
                (false, true) => Inst::AddsImm {
                    rd,
                    rn,
                    imm12: imm,
                    shift,
                    sf,
                },
                (true, true) => Inst::SubsImm {
                    rd,
                    rn,
                    imm12: imm,
                    shift,
                    sf,
                },
            })
        } else {
            let rm_start = self.pos;
            let (rm, rm_is_64bit, rm_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
            let rm_meta = GpOperandMeta::new(rm_is_64bit, rm_kind, rm_start);
            let modifier = self.parse_optional_add_sub_modifier(sf, sets_flags, rm_meta)?;
            let modifier = self.normalize_add_sub_register_modifier(
                rd_meta, rn_meta, rm_meta, sets_flags, modifier,
            )?;
            Ok(match (is_sub, sets_flags, modifier) {
                (false, false, None) => Inst::AddReg { rd, rn, rm, sf },
                (true, false, None) => Inst::SubReg { rd, rn, rm, sf },
                (false, true, None) => Inst::AddsReg { rd, rn, rm, sf },
                (true, true, None) => Inst::SubsReg { rd, rn, rm, sf },
                (false, false, Some(AddSubModifier::Shift(shift, amount))) => Inst::AddShiftReg {
                    rd,
                    rn,
                    rm,
                    shift,
                    amount,
                    sf,
                },
                (true, false, Some(AddSubModifier::Shift(shift, amount))) => Inst::SubShiftReg {
                    rd,
                    rn,
                    rm,
                    shift,
                    amount,
                    sf,
                },
                (false, true, Some(AddSubModifier::Shift(shift, amount))) => Inst::AddsShiftReg {
                    rd,
                    rn,
                    rm,
                    shift,
                    amount,
                    sf,
                },
                (true, true, Some(AddSubModifier::Shift(shift, amount))) => Inst::SubsShiftReg {
                    rd,
                    rn,
                    rm,
                    shift,
                    amount,
                    sf,
                },
                (false, false, Some(AddSubModifier::Extend(extend, amount))) => Inst::AddExtReg {
                    rd,
                    rn,
                    rm,
                    extend,
                    amount,
                    sf,
                },
                (true, false, Some(AddSubModifier::Extend(extend, amount))) => Inst::SubExtReg {
                    rd,
                    rn,
                    rm,
                    extend,
                    amount,
                    sf,
                },
                (false, true, Some(AddSubModifier::Extend(extend, amount))) => Inst::AddsExtReg {
                    rd,
                    rn,
                    rm,
                    extend,
                    amount,
                    sf,
                },
                (true, true, Some(AddSubModifier::Extend(extend, amount))) => Inst::SubsExtReg {
                    rd,
                    rn,
                    rm,
                    extend,
                    amount,
                    sf,
                },
            })
        }
    }

    fn parse_cmp(&mut self) -> Result<Inst, ParseError> {
        let rn_start = self.pos;
        let (rn, sf, rn_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            self.validate_add_sub_immediate_registers(
                GpOperandMeta::new(sf, GpRegKind::Zr, rn_start),
                GpOperandMeta::new(sf, rn_kind, rn_start),
                true,
            )?;
            let (imm, shift, negate_operation) = self.parse_add_sub_immediate("cmp immediate")?;
            if negate_operation {
                Ok(Inst::AddsImm {
                    rd: XZR,
                    rn,
                    imm12: imm,
                    shift,
                    sf,
                })
            } else {
                Ok(Inst::SubsImm {
                    rd: XZR,
                    rn,
                    imm12: imm,
                    shift,
                    sf,
                })
            }
        } else {
            let rm_start = self.pos;
            let (rm, rm_is_64bit, rm_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
            let rn_meta = GpOperandMeta::new(sf, rn_kind, rn_start);
            let rm_meta = GpOperandMeta::new(rm_is_64bit, rm_kind, rm_start);
            let modifier = self.parse_optional_add_sub_modifier(sf, true, rm_meta)?;
            let modifier = self.normalize_add_sub_register_modifier(
                GpOperandMeta::new(sf, GpRegKind::Zr, rn_start),
                rn_meta,
                rm_meta,
                true,
                modifier,
            )?;
            if let Some(modifier) = modifier {
                Ok(match modifier {
                    AddSubModifier::Shift(shift, amount) => Inst::SubsShiftReg {
                        rd: XZR,
                        rn,
                        rm,
                        shift,
                        amount,
                        sf,
                    },
                    AddSubModifier::Extend(extend, amount) => Inst::SubsExtReg {
                        rd: XZR,
                        rn,
                        rm,
                        extend,
                        amount,
                        sf,
                    },
                })
            } else {
                Ok(Inst::SubsReg {
                    rd: XZR,
                    rn,
                    rm,
                    sf,
                })
            }
        }
    }

    fn parse_cmn(&mut self) -> Result<Inst, ParseError> {
        let rn_start = self.pos;
        let (rn, sf, rn_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        let rm_start = self.pos;
        let (rm, rm_is_64bit, rm_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
        let rn_meta = GpOperandMeta::new(sf, rn_kind, rn_start);
        let rm_meta = GpOperandMeta::new(rm_is_64bit, rm_kind, rm_start);
        let modifier = self.parse_optional_add_sub_modifier(sf, true, rm_meta)?;
        let modifier = self.normalize_add_sub_register_modifier(
            GpOperandMeta::new(sf, GpRegKind::Zr, rn_start),
            rn_meta,
            rm_meta,
            true,
            modifier,
        )?;
        if let Some(modifier) = modifier {
            Ok(match modifier {
                AddSubModifier::Shift(shift, amount) => Inst::AddsShiftReg {
                    rd: XZR,
                    rn,
                    rm,
                    shift,
                    amount,
                    sf,
                },
                AddSubModifier::Extend(extend, amount) => Inst::AddsExtReg {
                    rd: XZR,
                    rn,
                    rm,
                    extend,
                    amount,
                    sf,
                },
            })
        } else {
            Ok(Inst::AddsReg {
                rd: XZR,
                rn,
                rm,
                sf,
            })
        }
    }

    fn parse_tst(&mut self) -> Result<Inst, ParseError> {
        let (rn, sf) = self.parse_gp_data_reg_with_size("tst")?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let imm = self.parse_logical_immediate_value(sf)?;
            Ok(Inst::AndsImm {
                rd: XZR,
                rn,
                imm,
                sf,
            })
        } else {
            let rm = self.parse_gp_reg_matching_width(sf, "tst")?;
            Ok(Inst::AndsReg {
                rd: XZR,
                rn,
                rm,
                sf,
            })
        }
    }

    fn parse_neg(&mut self) -> Result<Inst, ParseError> {
        let rd_start = self.pos;
        let (rd, sf, rd_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        let rm_start = self.pos;
        let (rm, rm_is_64bit, rm_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
        if rd_kind == GpRegKind::Sp || rm_kind == GpRegKind::Sp {
            let start = if rd_kind == GpRegKind::Sp {
                rd_start
            } else {
                rm_start
            };
            return Err(self.err_at(start, "neg does not allow sp operands".into()));
        }
        let modifier = self.parse_optional_add_sub_modifier(
            sf,
            false,
            GpOperandMeta::new(rm_is_64bit, rm_kind, rm_start),
        )?;
        if !matches!(modifier, Some(AddSubModifier::Extend(..))) && rm_is_64bit != sf {
            return Err(self.err_at(rm_start, "neg requires registers of the same width".into()));
        }
        self.validate_add_sub_extended_base_reg(
            GpOperandMeta::new(sf, GpRegKind::Zr, rm_start),
            modifier,
        )?;
        if let Some(modifier) = modifier {
            Ok(match modifier {
                AddSubModifier::Shift(shift, amount) => Inst::SubShiftReg {
                    rd,
                    rn: XZR,
                    rm,
                    shift,
                    amount,
                    sf,
                },
                AddSubModifier::Extend(extend, amount) => Inst::SubExtReg {
                    rd,
                    rn: XZR,
                    rm,
                    extend,
                    amount,
                    sf,
                },
            })
        } else {
            Ok(Inst::SubReg {
                rd,
                rn: XZR,
                rm,
                sf,
            })
        }
    }

    fn parse_mvn(&mut self) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size("mvn")?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_gp_reg_matching_width(sf, "mvn")?;
        Ok(Inst::OrnReg {
            rd,
            rn: XZR,
            rm,
            sf,
        })
    }

    fn parse_cond_select(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let cond_name = self.expect_ident()?;
        let cond = parse_condition(&cond_name)
            .ok_or_else(|| self.err(format!("unknown condition: {}", cond_name)))?;
        Ok(match mnemonic {
            "csel" => Inst::Csel {
                rd,
                rn,
                rm,
                cond,
                sf,
            },
            "csinc" => Inst::Csinc {
                rd,
                rn,
                rm,
                cond,
                sf,
            },
            "csinv" => Inst::Csinv {
                rd,
                rn,
                rm,
                cond,
                sf,
            },
            "csneg" => Inst::Csneg {
                rd,
                rn,
                rm,
                cond,
                sf,
            },
            _ => unreachable!("unsupported conditional select mnemonic"),
        })
    }

    fn parse_cond_select_set_alias(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let cond_name = self.expect_ident()?;
        let cond = parse_condition(&cond_name)
            .ok_or_else(|| self.err(format!("unknown condition: {}", cond_name)))?;
        let cond = invert_condition(cond);
        Ok(match mnemonic {
            "cset" => Inst::Csinc {
                rd,
                rn: XZR,
                rm: XZR,
                cond,
                sf,
            },
            "csetm" => Inst::Csinv {
                rd,
                rn: XZR,
                rm: XZR,
                cond,
                sf,
            },
            _ => unreachable!("unsupported conditional set alias"),
        })
    }

    fn parse_cond_select_unary_alias(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let cond_name = self.expect_ident()?;
        let cond = parse_condition(&cond_name)
            .ok_or_else(|| self.err(format!("unknown condition: {}", cond_name)))?;
        let cond = invert_condition(cond);
        Ok(match mnemonic {
            "cinc" => Inst::Csinc {
                rd,
                rn,
                rm: rn,
                cond,
                sf,
            },
            "cinv" => Inst::Csinv {
                rd,
                rn,
                rm: rn,
                cond,
                sf,
            },
            "cneg" => Inst::Csneg {
                rd,
                rn,
                rm: rn,
                cond,
                sf,
            },
            _ => unreachable!("unsupported conditional unary alias"),
        })
    }

    fn parse_csel(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select("csel")
    }

    fn parse_csinc(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select("csinc")
    }

    fn parse_csinv(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select("csinv")
    }

    fn parse_csneg(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select("csneg")
    }

    fn parse_cond_compare_imm(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rn, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let imm5 = self.parse_immediate_const_expr("conditional compare immediate")?;
        if !(0..=31).contains(&imm5) {
            return Err(self.err(format!(
                "conditional compare immediate must be in 0..=31, got {}",
                imm5
            )));
        }
        self.expect(&Tok::Comma)?;
        let nzcv = self.parse_immediate_const_expr("conditional compare nzcv")?;
        if !(0..=15).contains(&nzcv) {
            return Err(self.err(format!(
                "conditional compare nzcv must be in 0..=15, got {}",
                nzcv
            )));
        }
        self.expect(&Tok::Comma)?;
        let cond_name = self.expect_ident()?;
        let cond = parse_condition(&cond_name)
            .ok_or_else(|| self.err(format!("unknown condition: {}", cond_name)))?;
        Ok(match mnemonic {
            "ccmp" => Inst::CcmpImm {
                rn,
                imm5: imm5 as u8,
                nzcv: nzcv as u8,
                cond,
                sf,
            },
            "ccmn" => Inst::CcmnImm {
                rn,
                imm5: imm5 as u8,
                nzcv: nzcv as u8,
                cond,
                sf,
            },
            _ => unreachable!("unsupported conditional compare mnemonic"),
        })
    }

    fn parse_ccmp(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_compare_imm("ccmp")
    }

    fn parse_ccmn(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_compare_imm("ccmn")
    }

    fn parse_cset(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select_set_alias("cset")
    }

    fn parse_csetm(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select_set_alias("csetm")
    }

    fn parse_cinc(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select_unary_alias("cinc")
    }

    fn parse_cinv(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select_unary_alias("cinv")
    }

    fn parse_cneg(&mut self) -> Result<Inst, ParseError> {
        self.parse_cond_select_unary_alias("cneg")
    }

    fn parse_3reg(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        Ok(match mnemonic {
            "mul" => Inst::Mul { rd, rn, rm, sf },
            "sdiv" => Inst::Sdiv { rd, rn, rm, sf },
            "udiv" => Inst::Udiv { rd, rn, rm, sf },
            _ => unreachable!(),
        })
    }

    fn parse_madd_sub(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let ra = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        Ok(match mnemonic {
            "madd" => Inst::Madd { rd, rn, rm, ra, sf },
            "msub" => Inst::Msub { rd, rn, rm, ra, sf },
            _ => unreachable!(),
        })
    }

    fn parse_madd(&mut self) -> Result<Inst, ParseError> {
        self.parse_madd_sub("madd")
    }

    fn parse_umull(&mut self) -> Result<Inst, ParseError> {
        let rd_start = self.pos;
        let (rd, rd_is_64bit) = self.parse_gp_data_reg_with_size("umull")?;
        if !rd_is_64bit {
            return Err(self.err_at(rd_start, "umull destination must be an X register".into()));
        }
        self.expect(&Tok::Comma)?;
        let rn_start = self.pos;
        let (rn, rn_is_64bit) = self.parse_gp_data_reg_with_size("umull")?;
        if rn_is_64bit {
            return Err(self.err_at(rn_start, "umull sources must be W registers".into()));
        }
        self.expect(&Tok::Comma)?;
        let rm_start = self.pos;
        let (rm, rm_is_64bit) = self.parse_gp_data_reg_with_size("umull")?;
        if rm_is_64bit {
            return Err(self.err_at(rm_start, "umull sources must be W registers".into()));
        }
        Ok(Inst::Umull { rd, rn, rm })
    }

    fn parse_logic(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let imm = self.parse_logical_immediate_value(sf)?;
            Ok(match mnemonic {
                "and" => Inst::AndImm { rd, rn, imm, sf },
                "orr" => Inst::OrrImm { rd, rn, imm, sf },
                "eor" => Inst::EorImm { rd, rn, imm, sf },
                "ands" => Inst::AndsImm { rd, rn, imm, sf },
                _ => unreachable!(),
            })
        } else {
            let rm = self.parse_gp_reg_matching_width(sf, mnemonic)?;
            Ok(match mnemonic {
                "and" => Inst::AndReg { rd, rn, rm, sf },
                "orr" => Inst::OrrReg { rd, rn, rm, sf },
                "eor" => Inst::EorReg { rd, rn, rm, sf },
                "ands" => Inst::AndsReg { rd, rn, rm, sf },
                _ => unreachable!(),
            })
        }
    }

    fn parse_mov(&mut self) -> Result<Inst, ParseError> {
        if self.peek_is_scalar_fp_reg() {
            return self.parse_simd_lane_extract();
        }
        let rd_start = self.pos;
        let (rd, sf, rd_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            if rd_kind == GpRegKind::Sp {
                return Err(self.err_at(
                    rd_start,
                    "mov immediate does not allow sp as a destination".into(),
                ));
            }
            let imm = self.parse_immediate_const_expr("mov immediate")?;
            if let Some(inst) = mov_alias_imm(rd, imm, sf) {
                Ok(inst)
            } else {
                Err(self.err(format!(
                    "immediate {} is not encodable as a single MOV alias, use MOVZ/MOVN/MOVK sequence",
                    imm
                )))
            }
        } else {
            let rm_start = self.pos;
            let (rm, rm_is_64bit, rm_kind) = self.parse_add_sub_gp_reg_with_size_kind()?;
            if rm_is_64bit != sf {
                return Err(
                    self.err_at(rm_start, "mov requires registers of the same width".into())
                );
            }
            if (rd_kind == GpRegKind::Sp && rm_kind == GpRegKind::Zr)
                || (rd_kind == GpRegKind::Zr && rm_kind == GpRegKind::Sp)
            {
                return Err(self.err_at(
                    rm_start,
                    "mov cannot combine the stack pointer and zero register".into(),
                ));
            }
            if rm_kind == GpRegKind::Sp || rd_kind == GpRegKind::Sp {
                // MOV involving SP uses ADD because SP is unavailable to ORR.
                Ok(Inst::AddImm {
                    rd,
                    rn: rm,
                    imm12: 0,
                    shift: false,
                    sf,
                })
            } else {
                // MOV Xd, Xm → ORR Xd, XZR, Xm
                Ok(Inst::OrrReg {
                    rd,
                    rn: XZR,
                    rm,
                    sf,
                })
            }
        }
    }

    fn parse_simd_lane_extract(&mut self) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, index) = self.parse_simd_lane_ref(if is_double {
            SimdLaneWidth::D64
        } else {
            SimdLaneWidth::S32
        })?;
        Ok(if is_double {
            Inst::MovFromLaneD { rd, rn, index }
        } else {
            Inst::MovFromLaneS { rd, rn, index }
        })
    }

    fn parse_simd_lane_extract_gp(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let width = match mnemonic {
            "umov.h" => SimdLaneWidth::H16,
            "umov.b" => SimdLaneWidth::B8,
            _ => unreachable!(),
        };
        let rd = self.parse_lane_gp_reg(width, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let (rn, index) = self.parse_simd_lane_ref(width)?;
        Ok(match width {
            SimdLaneWidth::H16 => Inst::UmovFromLaneH { rd, rn, index },
            SimdLaneWidth::B8 => Inst::UmovFromLaneB { rd, rn, index },
            SimdLaneWidth::S32 | SimdLaneWidth::D64 => unreachable!(),
        })
    }

    fn parse_simd_lane_extract_gp_signed(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let width = match mnemonic {
            "smov.h" => SimdLaneWidth::H16,
            "smov.b" => SimdLaneWidth::B8,
            _ => unreachable!(),
        };
        let rd = self.parse_lane_gp_reg(width, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let (rn, index) = self.parse_simd_lane_ref(width)?;
        Ok(match width {
            SimdLaneWidth::H16 => Inst::SmovFromLaneH { rd, rn, index },
            SimdLaneWidth::B8 => Inst::SmovFromLaneB { rd, rn, index },
            SimdLaneWidth::S32 | SimdLaneWidth::D64 => unreachable!(),
        })
    }

    fn parse_simd_lane_insert(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let width = match mnemonic {
            "mov.s" => SimdLaneWidth::S32,
            "mov.d" => SimdLaneWidth::D64,
            "mov.h" => SimdLaneWidth::H16,
            "mov.b" => SimdLaneWidth::B8,
            _ => unreachable!(),
        };
        if matches!(width, SimdLaneWidth::S32 | SimdLaneWidth::D64) && self.peek_is_gp_reg() {
            let rd = self.parse_lane_gp_reg(width, mnemonic)?;
            self.expect(&Tok::Comma)?;
            let (rn, index) = self.parse_simd_lane_ref(width)?;
            return Ok(match width {
                SimdLaneWidth::S32 => Inst::MovFromLaneGpS { rd, rn, index },
                SimdLaneWidth::D64 => Inst::MovFromLaneGpD { rd, rn, index },
                SimdLaneWidth::B8 | SimdLaneWidth::H16 => unreachable!(),
            });
        }

        let (rd, rd_index) = self.parse_simd_lane_ref(width)?;
        self.expect(&Tok::Comma)?;
        if self.peek_is_simd_reg() {
            let (rn, rn_index) = self.parse_simd_lane_ref(width)?;
            Ok(match width {
                SimdLaneWidth::S32 => Inst::MovLaneS {
                    rd,
                    rd_index,
                    rn,
                    rn_index,
                },
                SimdLaneWidth::D64 => Inst::MovLaneD {
                    rd,
                    rd_index,
                    rn,
                    rn_index,
                },
                SimdLaneWidth::H16 => Inst::MovLaneH {
                    rd,
                    rd_index,
                    rn,
                    rn_index,
                },
                SimdLaneWidth::B8 => Inst::MovLaneB {
                    rd,
                    rd_index,
                    rn,
                    rn_index,
                },
            })
        } else {
            let rn = self.parse_lane_gp_reg(width, mnemonic)?;
            Ok(match width {
                SimdLaneWidth::S32 => Inst::MovLaneFromGpS { rd, rd_index, rn },
                SimdLaneWidth::D64 => Inst::MovLaneFromGpD { rd, rd_index, rn },
                SimdLaneWidth::H16 => Inst::MovLaneFromGpH { rd, rd_index, rn },
                SimdLaneWidth::B8 => Inst::MovLaneFromGpB { rd, rd_index, rn },
            })
        }
    }

    fn parse_simd_lane_load(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let width = match mnemonic {
            "ld1.s" => SimdLaneWidth::S32,
            "ld1.d" => SimdLaneWidth::D64,
            "ld1.h" => SimdLaneWidth::H16,
            "ld1.b" => SimdLaneWidth::B8,
            _ => unreachable!(),
        };
        self.expect(&Tok::LBrace)?;
        let rt = self.parse_simd_reg()?;
        self.expect(&Tok::RBrace)?;
        self.expect(&Tok::LBracket)?;
        let index = self.parse_lane_index(width.max_index())?;
        self.expect(&Tok::RBracket)?;
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg("SIMD lane load memory base")?;
        self.expect(&Tok::RBracket)?;
        Ok(match width {
            SimdLaneWidth::S32 => Inst::Ld1LaneS { rt, index, rn },
            SimdLaneWidth::D64 => Inst::Ld1LaneD { rt, index, rn },
            SimdLaneWidth::H16 => Inst::Ld1LaneH { rt, index, rn },
            SimdLaneWidth::B8 => Inst::Ld1LaneB { rt, index, rn },
        })
    }

    fn parse_mov_wide(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let imm = self.parse_u16_immediate("mov wide immediate")?;
        let shift = self.parse_optional_mov_wide_shift(sf)?;
        Ok(match mnemonic {
            "movz" => Inst::Movz {
                rd,
                imm16: imm,
                shift,
                sf,
            },
            "movk" => Inst::Movk {
                rd,
                imm16: imm,
                shift,
                sf,
            },
            "movn" => Inst::Movn {
                rd,
                imm16: imm,
                shift,
                sf,
            },
            _ => unreachable!(),
        })
    }

    fn parse_shift(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd_start = self.pos;
        let (rd, sf, rd_kind) = self.parse_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        let rn_start = self.pos;
        let (rn, rn_sf, rn_kind) = self.parse_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;

        if rd_kind == GpRegKind::Sp || rn_kind == GpRegKind::Sp {
            let start = if rd_kind == GpRegKind::Sp {
                rd_start
            } else {
                rn_start
            };
            return Err(self.err_at(start, format!("{} does not allow SP", mnemonic)));
        }
        if sf != rn_sf {
            return Err(self.err_at(
                rn_start,
                format!(
                    "{} requires source and destination registers of the same width",
                    mnemonic
                ),
            ));
        }

        if self.peek_is_gp_reg() {
            let rm_start = self.pos;
            let (rm, rm_sf, rm_kind) = self.parse_gp_reg_with_size_kind()?;
            if rm_kind == GpRegKind::Sp {
                return Err(self.err_at(
                    rm_start,
                    format!("{} register form does not allow SP", mnemonic),
                ));
            }
            if sf != rm_sf {
                return Err(self.err_at(
                    rm_start,
                    format!(
                        "{} register form requires registers of the same width",
                        mnemonic
                    ),
                ));
            }
            return Ok(match mnemonic {
                "lsl" => Inst::LslReg { rd, rn, rm, sf },
                "lsr" => Inst::LsrReg { rd, rn, rm, sf },
                "asr" => Inst::AsrReg { rd, rn, rm, sf },
                _ => unreachable!(),
            });
        }

        let amount = self.parse_immediate_const_expr("shift amount")?;
        let max_amount = if sf { 63 } else { 31 };
        if !(0..=max_amount).contains(&amount) {
            return Err(self.err(format!(
                "{} shift amount {} is out of range for a {}-bit register (expected 0..={})",
                mnemonic,
                amount,
                if sf { 64 } else { 32 },
                max_amount
            )));
        }
        let amount = u8::try_from(amount)
            .map_err(|_| self.err(format!("{} shift amount is out of range", mnemonic)))?;
        Ok(match mnemonic {
            "lsl" => Inst::LslImm { rd, rn, amount, sf },
            "lsr" => Inst::LsrImm { rd, rn, amount, sf },
            "asr" => Inst::AsrImm { rd, rn, amount, sf },
            _ => unreachable!(),
        })
    }

    fn parse_extend_alias(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd_start = self.pos;
        let (rd, rd_sf, rd_kind) = self.parse_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        let rn_start = self.pos;
        let (rn, rn_sf, rn_kind) = self.parse_gp_reg_with_size_kind()?;

        if rd_kind == GpRegKind::Sp || rn_kind == GpRegKind::Sp {
            let start = if rd_kind == GpRegKind::Sp {
                rd_start
            } else {
                rn_start
            };
            return Err(self.err_at(start, format!("{} does not allow SP", mnemonic)));
        }
        if rn_sf {
            return Err(self.err_at(
                rn_start,
                format!("{} requires a w-register source", mnemonic),
            ));
        }

        match mnemonic {
            "sxtw" if rd_sf => Ok(Inst::Sxtw { rd, rn }),
            "sxtw" => Err(self.err_at(rd_start, "sxtw requires an x-register destination".into())),
            "sxtb" => Ok(Inst::Sxtb { rd, rn, sf: rd_sf }),
            "sxth" => Ok(Inst::Sxth { rd, rn, sf: rd_sf }),
            "uxtw" if rd_sf => Ok(Inst::Uxtw { rd, rn }),
            "uxtw" => Err(self.err_at(rd_start, "uxtw requires an x-register destination".into())),
            _ => unreachable!(),
        }
    }

    fn parse_bitfield_alias(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_gp_reg_matching_width(sf, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let lsb_start = self.pos;
        let lsb = self.parse_immediate_const_expr("bitfield lsb")?;
        self.expect(&Tok::Comma)?;
        let width_start = self.pos;
        let width = self.parse_immediate_const_expr("bitfield width")?;
        self.validate_bitfield_alias_args(mnemonic, sf, lsb, width, lsb_start, width_start)?;
        let lsb = u8::try_from(lsb)
            .map_err(|_| self.err(format!("{} lsb does not fit in u8", mnemonic)))?;
        let width = u8::try_from(width)
            .map_err(|_| self.err(format!("{} width does not fit in u8", mnemonic)))?;
        Ok(match mnemonic {
            "ubfiz" => Inst::Ubfiz {
                rd,
                rn,
                lsb,
                width,
                sf,
            },
            "bfi" => Inst::Bfi {
                rd,
                rn,
                lsb,
                width,
                sf,
            },
            "bfxil" => Inst::Bfxil {
                rd,
                rn,
                lsb,
                width,
                sf,
            },
            _ => unreachable!(),
        })
    }

    fn parse_b(&mut self) -> Result<Stmt, ParseError> {
        if self.starts_immediate_expr() {
            let offset = self.parse_i32_signed_scaled_immediate("branch offset", 26, 2)?;
            Ok(Stmt::Instruction(Inst::B { offset }))
        } else {
            let label = self.parse_branch_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            Ok(Stmt::InstructionWithReloc(
                Inst::B { offset: 0 },
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Branch26,
                    addend,
                },
            ))
        }
    }

    fn parse_bl(&mut self) -> Result<Stmt, ParseError> {
        if self.starts_immediate_expr() {
            let offset = self.parse_i32_signed_scaled_immediate("branch offset", 26, 2)?;
            Ok(Stmt::Instruction(Inst::Bl { offset }))
        } else {
            let label = self.parse_branch_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            Ok(Stmt::InstructionWithReloc(
                Inst::Bl { offset: 0 },
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Branch26,
                    addend,
                },
            ))
        }
    }

    fn parse_bcond(&mut self, cond_str: &str) -> Result<Stmt, ParseError> {
        let cond = parse_condition(cond_str)
            .ok_or_else(|| self.err(format!("unknown condition: {}", cond_str)))?;
        if self.starts_immediate_expr() {
            let offset =
                self.parse_i32_signed_scaled_immediate("conditional branch offset", 19, 2)?;
            Ok(Stmt::Instruction(Inst::BCond { cond, offset }))
        } else {
            let label = self.parse_branch_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            Ok(Stmt::InstructionWithReloc(
                Inst::BCond { cond, offset: 0 },
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Branch19,
                    addend,
                },
            ))
        }
    }

    fn parse_cbz(&mut self, is_nz: bool) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_gp_data_reg_with_size("cbz/cbnz")?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let offset = self.parse_i32_signed_scaled_immediate("cbz/cbnz offset", 19, 2)?;
            let inst = if is_nz {
                Inst::Cbnz { rt, offset, sf }
            } else {
                Inst::Cbz { rt, offset, sf }
            };
            Ok(Stmt::Instruction(inst))
        } else {
            let label = self.parse_branch_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            let inst = if is_nz {
                Inst::Cbnz { rt, offset: 0, sf }
            } else {
                Inst::Cbz { rt, offset: 0, sf }
            };
            Ok(Stmt::InstructionWithReloc(
                inst,
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Branch19,
                    addend,
                },
            ))
        }
    }

    fn parse_tbz(&mut self, is_nz: bool) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_gp_data_reg_with_size("tbz/tbnz")?;
        self.expect(&Tok::Comma)?;
        let bit = self.parse_bit_index(
            sf,
            if is_nz {
                "tbnz bit index"
            } else {
                "tbz bit index"
            },
        )?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let offset = self.parse_i32_signed_scaled_immediate("tbz/tbnz offset", 14, 2)?;
            let inst = if is_nz {
                Inst::Tbnz {
                    rt,
                    bit,
                    offset,
                    sf,
                }
            } else {
                Inst::Tbz {
                    rt,
                    bit,
                    offset,
                    sf,
                }
            };
            Ok(Stmt::Instruction(inst))
        } else {
            let label = self.parse_branch_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            let inst = if is_nz {
                Inst::Tbnz {
                    rt,
                    bit,
                    offset: 0,
                    sf,
                }
            } else {
                Inst::Tbz {
                    rt,
                    bit,
                    offset: 0,
                    sf,
                }
            };
            Ok(Stmt::InstructionWithReloc(
                inst,
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Branch14,
                    addend,
                },
            ))
        }
    }

    fn parse_ret(&mut self) -> Result<Inst, ParseError> {
        if self.at_end_of_stmt() {
            Ok(Inst::Ret { rn: X30 })
        } else {
            let rn = self.parse_x_reg("ret")?;
            Ok(Inst::Ret { rn })
        }
    }

    fn parse_adr(&mut self) -> Result<Stmt, ParseError> {
        let rd = self.parse_x_reg("adr destination")?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let imm = self.parse_i32_signed_scaled_immediate("adr immediate", 21, 0)?;
            Ok(Stmt::Instruction(Inst::Adr { rd, imm }))
        } else {
            let label = self.parse_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            Ok(Stmt::InstructionWithReloc(
                Inst::Adr { rd, imm: 0 },
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Adr21,
                    addend,
                },
            ))
        }
    }

    fn parse_adrp(&mut self) -> Result<Stmt, ParseError> {
        let rd = self.parse_x_reg("adrp destination")?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let imm = self.parse_signed_scaled_immediate("adrp immediate", 21, 12)?;
            Ok(Stmt::Instruction(Inst::Adrp { rd, imm }))
        } else {
            let label = self.parse_label_reference()?;
            let kind = self.parse_symbol_reloc_modifier(
                Some(RelocKind::Page21),
                &[
                    ("PAGE", RelocKind::Page21),
                    ("GOTPAGE", RelocKind::GotLoadPage21),
                    ("TLVPPAGE", RelocKind::TlvpLoadPage21),
                ],
                "adrp symbol operand",
            )?;
            let addend = self.parse_optional_symbol_addend()?;
            Ok(Stmt::InstructionWithReloc(
                Inst::Adrp { rd, imm: 0 },
                LabelRef {
                    symbol: label,
                    kind,
                    addend,
                },
            ))
        }
    }

    fn parse_ldr_str(&mut self, is_load: bool) -> Result<Stmt, ParseError> {
        if self.starts_fp_register_like_operand() {
            return self.parse_ldr_str_fp(is_load);
        }
        self.parse_ldr_str_gp(is_load)
    }

    fn gp_mem_offset_inst(
        &self,
        is_load: bool,
        sf: bool,
        rt: GpReg,
        rn: GpReg,
        offset: i64,
        start: usize,
    ) -> Result<Inst, ParseError> {
        let scale = if sf { 3 } else { 2 };
        let fits_unsigned =
            offset >= 0 && offset % (1i64 << scale) == 0 && (offset >> scale) <= 0xFFF;
        if fits_unsigned {
            let offset = u16::try_from(offset).map_err(|_| {
                self.err_at(start, format!("memory offset {} is not encodable", offset))
            })?;
            return Ok(match (is_load, sf) {
                (true, true) => Inst::LdrImm64 { rt, rn, offset },
                (false, true) => Inst::StrImm64 { rt, rn, offset },
                (true, false) => Inst::LdrImm32 { rt, rn, offset },
                (false, false) => Inst::StrImm32 { rt, rn, offset },
            });
        }

        if (-256..=255).contains(&offset) {
            let offset = i16::try_from(offset).map_err(|_| {
                self.err_at(start, format!("memory offset {} is not encodable", offset))
            })?;
            return Ok(self.gp_unscaled_mem_inst(is_load, sf, rt, rn, offset));
        }

        Err(self.err_at(
            start,
            format!(
                "memory offset {} is not encodable as a signed unscaled or unsigned scaled offset",
                offset
            ),
        ))
    }

    fn gp_unscaled_mem_inst(
        &self,
        is_load: bool,
        sf: bool,
        rt: GpReg,
        rn: GpReg,
        offset: i16,
    ) -> Inst {
        match (is_load, sf) {
            (true, true) => Inst::Ldur64 { rt, rn, offset },
            (false, true) => Inst::Stur64 { rt, rn, offset },
            (true, false) => Inst::Ldur32 { rt, rn, offset },
            (false, false) => Inst::Stur32 { rt, rn, offset },
        }
    }

    fn gp_mem_pre_inst(&self, is_load: bool, sf: bool, rt: GpReg, rn: GpReg, offset: i16) -> Inst {
        match (is_load, sf) {
            (true, true) => Inst::LdrPre64 { rt, rn, offset },
            (false, true) => Inst::StrPre64 { rt, rn, offset },
            (true, false) => Inst::LdrPre32 { rt, rn, offset },
            (false, false) => Inst::StrPre32 { rt, rn, offset },
        }
    }

    fn gp_mem_post_inst(&self, is_load: bool, sf: bool, rt: GpReg, rn: GpReg, offset: i16) -> Inst {
        match (is_load, sf) {
            (true, true) => Inst::LdrPost64 { rt, rn, offset },
            (false, true) => Inst::StrPost64 { rt, rn, offset },
            (true, false) => Inst::LdrPost32 { rt, rn, offset },
            (false, false) => Inst::StrPost32 { rt, rn, offset },
        }
    }

    fn fp_mem_offset_inst(
        &self,
        is_load: bool,
        width: FpMemWidth,
        rt: FpReg,
        rn: GpReg,
        offset: i64,
        start: usize,
    ) -> Result<Inst, ParseError> {
        let scale = 1i64 << width.scale();
        let fits_unsigned =
            offset >= 0 && offset % scale == 0 && (offset >> width.scale()) <= 0xFFF;
        if fits_unsigned {
            let offset = offset as u16;
            return Ok(match (is_load, width) {
                (true, FpMemWidth::H16) => Inst::LdrFpImm16 { rt, rn, offset },
                (false, FpMemWidth::H16) => Inst::StrFpImm16 { rt, rn, offset },
                (true, FpMemWidth::B8) => Inst::LdrFpImm8 { rt, rn, offset },
                (false, FpMemWidth::B8) => Inst::StrFpImm8 { rt, rn, offset },
                (true, FpMemWidth::D64) => Inst::LdrFpImm64 { rt, rn, offset },
                (false, FpMemWidth::D64) => Inst::StrFpImm64 { rt, rn, offset },
                (true, FpMemWidth::S32) => Inst::LdrFpImm32 { rt, rn, offset },
                (false, FpMemWidth::S32) => Inst::StrFpImm32 { rt, rn, offset },
                (true, FpMemWidth::Q128) => Inst::LdrFpImm128 { rt, rn, offset },
                (false, FpMemWidth::Q128) => Inst::StrFpImm128 { rt, rn, offset },
            });
        }

        if (-256..=255).contains(&offset) {
            let offset = offset as i16;
            return Ok(match (is_load, width) {
                (true, FpMemWidth::H16) => Inst::LdurFp16 { rt, rn, offset },
                (false, FpMemWidth::H16) => Inst::SturFp16 { rt, rn, offset },
                (true, FpMemWidth::B8) => Inst::LdurFp8 { rt, rn, offset },
                (false, FpMemWidth::B8) => Inst::SturFp8 { rt, rn, offset },
                (true, FpMemWidth::D64) => Inst::LdurFp64 { rt, rn, offset },
                (false, FpMemWidth::D64) => Inst::SturFp64 { rt, rn, offset },
                (true, FpMemWidth::S32) => Inst::LdurFp32 { rt, rn, offset },
                (false, FpMemWidth::S32) => Inst::SturFp32 { rt, rn, offset },
                (true, FpMemWidth::Q128) => Inst::LdurFp128 { rt, rn, offset },
                (false, FpMemWidth::Q128) => Inst::SturFp128 { rt, rn, offset },
            });
        }

        Err(self.err_at(
            start,
            format!(
                "FP/SIMD memory offset {offset} is out of range for {}",
                if is_load { "LDR" } else { "STR" }
            ),
        ))
    }

    fn parse_ldur_stur(&mut self, is_load: bool) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_gp_data_reg_with_size("ldur/stur data operand")?;
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg("ldur/stur memory base")?;
        let offset = if self.eat(&Tok::RBracket) {
            0
        } else {
            self.expect(&Tok::Comma)?;
            let offset = self.parse_i16_signed_scaled_immediate("memory offset", 9, 0)?;
            self.expect(&Tok::RBracket)?;
            offset
        };

        if self.eat(&Tok::Bang) {
            return Err(self.err("ldur/stur do not support pre-index addressing".into()));
        }
        if self.eat(&Tok::Comma) {
            return Err(self.err("ldur/stur do not support post-index addressing".into()));
        }

        Ok(Stmt::Instruction(
            self.gp_unscaled_mem_inst(is_load, sf, rt, rn, offset),
        ))
    }

    fn parse_ldr_str_gp(&mut self, is_load: bool) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_gp_data_reg_with_size("ldr/str data operand")?;
        self.expect(&Tok::Comma)?;

        if is_load && self.starts_immediate_expr() {
            let offset = self.parse_i32_signed_scaled_immediate("ldr literal offset", 19, 2)?;
            let inst = if sf {
                Inst::LdrLit64 { rt, offset }
            } else {
                Inst::LdrLit32 { rt, offset }
            };
            return Ok(Stmt::Instruction(inst));
        }

        if is_load && self.starts_non_register_literal_reference() {
            let label = self.parse_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            let inst = if sf {
                Inst::LdrLit64 { rt, offset: 0 }
            } else {
                Inst::LdrLit32 { rt, offset: 0 }
            };
            return Ok(Stmt::InstructionWithReloc(
                inst,
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Literal19,
                    addend,
                },
            ));
        }

        if !is_load && self.peek() != &Tok::LBracket {
            return Err(self.err("STR expects a bracketed memory operand".into()));
        }

        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg("ldr/str memory base")?;

        if self.eat(&Tok::RBracket) {
            // [Xn] or [Xn], #off (post-index)
            if self.eat(&Tok::Comma) {
                let offset = self.parse_i16_signed_scaled_immediate("post-index offset", 9, 0)?;
                return Ok(Stmt::Instruction(
                    self.gp_mem_post_inst(is_load, sf, rt, rn, offset),
                ));
            }
            return Ok(Stmt::Instruction(
                self.gp_mem_offset_inst(is_load, sf, rt, rn, 0, self.pos)?,
            ));
        }

        self.expect(&Tok::Comma)?;
        if self.starts_non_register_symbol_reference() {
            let label = self.parse_label_reference()?;
            let allowed = if is_load {
                &[
                    ("PAGEOFF", RelocKind::PageOff12),
                    ("GOTPAGEOFF", RelocKind::GotLoadPageOff12),
                    ("TLVPPAGEOFF", RelocKind::TlvpLoadPageOff12),
                ][..]
            } else {
                &[("PAGEOFF", RelocKind::PageOff12)][..]
            };
            let kind = self.parse_symbol_reloc_modifier(None, allowed, "memory symbol operand")?;
            let addend = self.parse_optional_symbol_addend()?;
            self.expect(&Tok::RBracket)?;
            let inst = if sf {
                if is_load {
                    Inst::LdrImm64 { rt, rn, offset: 0 }
                } else {
                    Inst::StrImm64 { rt, rn, offset: 0 }
                }
            } else {
                if is_load {
                    Inst::LdrImm32 { rt, rn, offset: 0 }
                } else {
                    Inst::StrImm32 { rt, rn, offset: 0 }
                }
            };
            return Ok(Stmt::InstructionWithReloc(
                inst,
                LabelRef {
                    symbol: label,
                    kind,
                    addend,
                },
            ));
        }

        if self.starts_register_like_operand() {
            let (rm, extend, shift) = self.parse_reg_offset_operand(if sf { 3 } else { 2 })?;
            self.expect(&Tok::RBracket)?;
            let inst = if sf {
                if is_load {
                    Inst::LdrReg64 {
                        rt,
                        rn,
                        rm,
                        extend,
                        shift,
                    }
                } else {
                    Inst::StrReg64 {
                        rt,
                        rn,
                        rm,
                        extend,
                        shift,
                    }
                }
            } else if is_load {
                Inst::LdrReg32 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                }
            } else {
                Inst::StrReg32 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                }
            };
            return Ok(Stmt::Instruction(inst));
        }

        let offset_start = self.pos;
        let offset = self.parse_immediate_const_expr("memory offset")?;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            let offset = self.validate_i16_signed_scaled_immediate(
                offset,
                "memory offset",
                9,
                0,
                offset_start,
            )?;
            return Ok(Stmt::Instruction(
                self.gp_mem_pre_inst(is_load, sf, rt, rn, offset),
            ));
        }

        Ok(Stmt::Instruction(self.gp_mem_offset_inst(
            is_load,
            sf,
            rt,
            rn,
            offset,
            offset_start,
        )?))
    }

    fn parse_ldr_str_fp(&mut self, is_load: bool) -> Result<Stmt, ParseError> {
        let (rt, width) = self.parse_fp_mem_reg_with_width()?;
        self.expect(&Tok::Comma)?;
        let is_scalar_narrow = matches!(width, FpMemWidth::B8 | FpMemWidth::H16);

        if is_load && self.starts_immediate_expr() {
            if is_scalar_narrow {
                return Err(self.err("narrow FP literal loads are not supported".into()));
            }
            let offset = self.parse_i32_signed_scaled_immediate("ldr literal offset", 19, 2)?;
            let inst = match width {
                FpMemWidth::B8 | FpMemWidth::H16 => unreachable!(),
                FpMemWidth::D64 => Inst::LdrFpLit64 { rt, offset },
                FpMemWidth::S32 => Inst::LdrFpLit32 { rt, offset },
                FpMemWidth::Q128 => Inst::LdrFpLit128 { rt, offset },
            };
            return Ok(Stmt::Instruction(inst));
        }

        if is_load && self.starts_non_register_literal_reference() {
            if is_scalar_narrow {
                return Err(self.err("narrow FP literal loads are not supported".into()));
            }
            let label = self.parse_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            let inst = match width {
                FpMemWidth::B8 | FpMemWidth::H16 => unreachable!(),
                FpMemWidth::D64 => Inst::LdrFpLit64 { rt, offset: 0 },
                FpMemWidth::S32 => Inst::LdrFpLit32 { rt, offset: 0 },
                FpMemWidth::Q128 => Inst::LdrFpLit128 { rt, offset: 0 },
            };
            return Ok(Stmt::InstructionWithReloc(
                inst,
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Literal19,
                    addend,
                },
            ));
        }

        if !is_load && self.peek() != &Tok::LBracket {
            return Err(self.err("STR expects a bracketed memory operand".into()));
        }

        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg("FP/SIMD memory base")?;

        if self.eat(&Tok::RBracket) {
            if self.eat(&Tok::Comma) {
                if is_scalar_narrow {
                    return Err(
                        self.err("post-index narrow FP loads/stores are not supported".into())
                    );
                }
                let offset = self.parse_i16_signed_scaled_immediate("post-index offset", 9, 0)?;
                let inst = match (is_load, width) {
                    (_, FpMemWidth::B8 | FpMemWidth::H16) => unreachable!(),
                    (true, FpMemWidth::D64) => Inst::LdrFpPost64 { rt, rn, offset },
                    (false, FpMemWidth::D64) => Inst::StrFpPost64 { rt, rn, offset },
                    (true, FpMemWidth::S32) => Inst::LdrFpPost32 { rt, rn, offset },
                    (false, FpMemWidth::S32) => Inst::StrFpPost32 { rt, rn, offset },
                    (true, FpMemWidth::Q128) => Inst::LdrFpPost128 { rt, rn, offset },
                    (false, FpMemWidth::Q128) => Inst::StrFpPost128 { rt, rn, offset },
                };
                return Ok(Stmt::Instruction(inst));
            }
            let inst = match (is_load, width) {
                (true, FpMemWidth::H16) => Inst::LdrFpImm16 { rt, rn, offset: 0 },
                (false, FpMemWidth::H16) => Inst::StrFpImm16 { rt, rn, offset: 0 },
                (true, FpMemWidth::B8) => Inst::LdrFpImm8 { rt, rn, offset: 0 },
                (false, FpMemWidth::B8) => Inst::StrFpImm8 { rt, rn, offset: 0 },
                (true, FpMemWidth::D64) => Inst::LdrFpImm64 { rt, rn, offset: 0 },
                (false, FpMemWidth::D64) => Inst::StrFpImm64 { rt, rn, offset: 0 },
                (true, FpMemWidth::S32) => Inst::LdrFpImm32 { rt, rn, offset: 0 },
                (false, FpMemWidth::S32) => Inst::StrFpImm32 { rt, rn, offset: 0 },
                (true, FpMemWidth::Q128) => Inst::LdrFpImm128 { rt, rn, offset: 0 },
                (false, FpMemWidth::Q128) => Inst::StrFpImm128 { rt, rn, offset: 0 },
            };
            return Ok(Stmt::Instruction(inst));
        }

        self.expect(&Tok::Comma)?;
        if self.starts_non_register_symbol_reference() {
            if is_scalar_narrow {
                return Err(self.err("symbolic narrow FP loads/stores are not supported".into()));
            }
            let label = self.parse_label_reference()?;
            let kind = self.parse_symbol_reloc_modifier(
                None,
                &[("PAGEOFF", RelocKind::PageOff12)],
                "FP/SIMD memory symbol operand",
            )?;
            let addend = self.parse_optional_symbol_addend()?;
            self.expect(&Tok::RBracket)?;
            let inst = match (is_load, width) {
                (_, FpMemWidth::B8 | FpMemWidth::H16) => unreachable!(),
                (true, FpMemWidth::D64) => Inst::LdrFpImm64 { rt, rn, offset: 0 },
                (false, FpMemWidth::D64) => Inst::StrFpImm64 { rt, rn, offset: 0 },
                (true, FpMemWidth::S32) => Inst::LdrFpImm32 { rt, rn, offset: 0 },
                (false, FpMemWidth::S32) => Inst::StrFpImm32 { rt, rn, offset: 0 },
                (true, FpMemWidth::Q128) => Inst::LdrFpImm128 { rt, rn, offset: 0 },
                (false, FpMemWidth::Q128) => Inst::StrFpImm128 { rt, rn, offset: 0 },
            };
            return Ok(Stmt::InstructionWithReloc(
                inst,
                LabelRef {
                    symbol: label,
                    kind,
                    addend,
                },
            ));
        }

        if self.starts_register_like_operand() {
            if is_scalar_narrow {
                return Err(
                    self.err("register-offset narrow FP loads/stores are not supported".into())
                );
            }
            let (rm, extend, shift) = self.parse_reg_offset_operand(width.scale())?;
            self.expect(&Tok::RBracket)?;
            let inst = match (is_load, width) {
                (_, FpMemWidth::B8 | FpMemWidth::H16) => unreachable!(),
                (true, FpMemWidth::D64) => Inst::LdrFpReg64 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
                (false, FpMemWidth::D64) => Inst::StrFpReg64 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
                (true, FpMemWidth::S32) => Inst::LdrFpReg32 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
                (false, FpMemWidth::S32) => Inst::StrFpReg32 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
                (true, FpMemWidth::Q128) => Inst::LdrFpReg128 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
                (false, FpMemWidth::Q128) => Inst::StrFpReg128 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
            };
            return Ok(Stmt::Instruction(inst));
        }

        let offset_start = self.pos;
        let offset = self.parse_immediate_const_expr("memory offset")?;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            if is_scalar_narrow {
                return Err(self.err("pre-index narrow FP loads/stores are not supported".into()));
            }
            let offset = self.validate_i16_signed_scaled_immediate(
                offset,
                "memory offset",
                9,
                0,
                offset_start,
            )?;
            let inst = match (is_load, width) {
                (_, FpMemWidth::B8 | FpMemWidth::H16) => unreachable!(),
                (true, FpMemWidth::D64) => Inst::LdrFpPre64 { rt, rn, offset },
                (false, FpMemWidth::D64) => Inst::StrFpPre64 { rt, rn, offset },
                (true, FpMemWidth::S32) => Inst::LdrFpPre32 { rt, rn, offset },
                (false, FpMemWidth::S32) => Inst::StrFpPre32 { rt, rn, offset },
                (true, FpMemWidth::Q128) => Inst::LdrFpPre128 { rt, rn, offset },
                (false, FpMemWidth::Q128) => Inst::StrFpPre128 { rt, rn, offset },
            };
            return Ok(Stmt::Instruction(inst));
        }

        let inst = self.fp_mem_offset_inst(is_load, width, rt, rn, offset, offset_start)?;
        Ok(Stmt::Instruction(inst))
    }

    fn parse_ldstb_h_offset_inst(
        &self,
        mnemonic: &str,
        rt: GpReg,
        rn: GpReg,
        offset: i64,
        start: usize,
    ) -> Result<Inst, ParseError> {
        let scale = match mnemonic {
            "ldrb" | "strb" => 0,
            "ldrh" | "strh" => 1,
            _ => unreachable!(),
        };
        let fits_unsigned =
            offset >= 0 && offset % (1i64 << scale) == 0 && (offset >> scale) <= 0xFFF;
        if fits_unsigned {
            let offset = u16::try_from(offset).map_err(|_| {
                self.err_at(start, format!("memory offset {} is not encodable", offset))
            })?;
            return Ok(match mnemonic {
                "ldrb" => Inst::Ldrb { rt, rn, offset },
                "ldrh" => Inst::Ldrh { rt, rn, offset },
                "strb" => Inst::Strb { rt, rn, offset },
                "strh" => Inst::Strh { rt, rn, offset },
                _ => unreachable!(),
            });
        }

        if (-256..=255).contains(&offset) {
            let offset = i16::try_from(offset).map_err(|_| {
                self.err_at(start, format!("memory offset {} is not encodable", offset))
            })?;
            return Ok(match mnemonic {
                "ldrb" => Inst::Ldurb { rt, rn, offset },
                "ldrh" => Inst::Ldurh { rt, rn, offset },
                "strb" => Inst::Sturb { rt, rn, offset },
                "strh" => Inst::Sturh { rt, rn, offset },
                _ => unreachable!(),
            });
        }

        Err(self.err_at(
            start,
            format!(
                "memory offset {} is not encodable as a signed unscaled or unsigned scaled offset",
                offset
            ),
        ))
    }

    fn parse_ldst_signed_b_h_offset_inst(
        &self,
        mnemonic: &str,
        rt: GpReg,
        sf: bool,
        rn: GpReg,
        offset: i64,
        start: usize,
    ) -> Result<Inst, ParseError> {
        let scale = if mnemonic == "ldrsb" { 0 } else { 1 };
        let fits_unsigned =
            offset >= 0 && offset % (1i64 << scale) == 0 && (offset >> scale) <= 0xFFF;
        if fits_unsigned {
            let offset = u16::try_from(offset).map_err(|_| {
                self.err_at(start, format!("memory offset {} is not encodable", offset))
            })?;
            return Ok(match (mnemonic, sf) {
                ("ldrsb", false) => Inst::Ldrsb32 { rt, rn, offset },
                ("ldrsb", true) => Inst::Ldrsb64 { rt, rn, offset },
                ("ldrsh", false) => Inst::Ldrsh32 { rt, rn, offset },
                ("ldrsh", true) => Inst::Ldrsh64 { rt, rn, offset },
                _ => unreachable!(),
            });
        }

        if (-256..=255).contains(&offset) {
            let offset = i16::try_from(offset).map_err(|_| {
                self.err_at(start, format!("memory offset {} is not encodable", offset))
            })?;
            return Ok(match (mnemonic, sf) {
                ("ldrsb", false) => Inst::Ldursb32 { rt, rn, offset },
                ("ldrsb", true) => Inst::Ldursb64 { rt, rn, offset },
                ("ldrsh", false) => Inst::Ldursh32 { rt, rn, offset },
                ("ldrsh", true) => Inst::Ldursh64 { rt, rn, offset },
                _ => unreachable!(),
            });
        }

        Err(self.err_at(
            start,
            format!(
                "memory offset {} is not encodable as a signed unscaled or unsigned scaled offset",
                offset
            ),
        ))
    }

    fn parse_ldrsw_offset_inst(
        &self,
        rt: GpReg,
        rn: GpReg,
        offset: i64,
        start: usize,
    ) -> Result<Inst, ParseError> {
        let fits_unsigned = offset >= 0 && offset % 4 == 0 && (offset >> 2) <= 0xFFF;
        if fits_unsigned {
            let offset = u16::try_from(offset).map_err(|_| {
                self.err_at(start, format!("memory offset {} is not encodable", offset))
            })?;
            return Ok(Inst::Ldrsw { rt, rn, offset });
        }

        if (-256..=255).contains(&offset) {
            let offset = i16::try_from(offset).map_err(|_| {
                self.err_at(start, format!("memory offset {} is not encodable", offset))
            })?;
            return Ok(Inst::Ldursw { rt, rn, offset });
        }

        Err(self.err_at(
            start,
            format!(
                "memory offset {} is not encodable as a signed unscaled or unsigned scaled offset",
                offset
            ),
        ))
    }

    fn parse_ldstb_h_reg_inst(
        &self,
        mnemonic: &str,
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    ) -> Inst {
        match mnemonic {
            "ldrb" => Inst::LdrbReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            },
            "ldrh" => Inst::LdrhReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            },
            "strb" => Inst::StrbReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            },
            "strh" => Inst::StrhReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            },
            _ => unreachable!(),
        }
    }

    fn parse_ldst_signed_b_h_reg_inst(
        &self,
        mnemonic: &str,
        rt_and_sf: (GpReg, bool),
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    ) -> Inst {
        let (rt, sf) = rt_and_sf;
        match (mnemonic, sf) {
            ("ldrsb", false) => Inst::LdrsbReg32 {
                rt,
                rn,
                rm,
                extend,
                shift,
            },
            ("ldrsb", true) => Inst::LdrsbReg64 {
                rt,
                rn,
                rm,
                extend,
                shift,
            },
            ("ldrsh", false) => Inst::LdrshReg32 {
                rt,
                rn,
                rm,
                extend,
                shift,
            },
            ("ldrsh", true) => Inst::LdrshReg64 {
                rt,
                rn,
                rm,
                extend,
                shift,
            },
            _ => unreachable!(),
        }
    }

    fn parse_ldstb_h_pre_inst(&self, mnemonic: &str, rt: GpReg, rn: GpReg, offset: i16) -> Inst {
        match mnemonic {
            "ldrb" => Inst::LdrbPre { rt, rn, offset },
            "ldrh" => Inst::LdrhPre { rt, rn, offset },
            "strb" => Inst::StrbPre { rt, rn, offset },
            "strh" => Inst::StrhPre { rt, rn, offset },
            _ => unreachable!(),
        }
    }

    fn parse_ldst_signed_b_h_pre_inst(
        &self,
        mnemonic: &str,
        rt: GpReg,
        sf: bool,
        rn: GpReg,
        offset: i16,
    ) -> Inst {
        match (mnemonic, sf) {
            ("ldrsb", false) => Inst::LdrsbPre32 { rt, rn, offset },
            ("ldrsb", true) => Inst::LdrsbPre64 { rt, rn, offset },
            ("ldrsh", false) => Inst::LdrshPre32 { rt, rn, offset },
            ("ldrsh", true) => Inst::LdrshPre64 { rt, rn, offset },
            _ => unreachable!(),
        }
    }

    fn parse_ldstb_h_post_inst(&self, mnemonic: &str, rt: GpReg, rn: GpReg, offset: i16) -> Inst {
        match mnemonic {
            "ldrb" => Inst::LdrbPost { rt, rn, offset },
            "ldrh" => Inst::LdrhPost { rt, rn, offset },
            "strb" => Inst::StrbPost { rt, rn, offset },
            "strh" => Inst::StrhPost { rt, rn, offset },
            _ => unreachable!(),
        }
    }

    fn parse_ldst_signed_b_h_post_inst(
        &self,
        mnemonic: &str,
        rt: GpReg,
        sf: bool,
        rn: GpReg,
        offset: i16,
    ) -> Inst {
        match (mnemonic, sf) {
            ("ldrsb", false) => Inst::LdrsbPost32 { rt, rn, offset },
            ("ldrsb", true) => Inst::LdrsbPost64 { rt, rn, offset },
            ("ldrsh", false) => Inst::LdrshPost32 { rt, rn, offset },
            ("ldrsh", true) => Inst::LdrshPost64 { rt, rn, offset },
            _ => unreachable!(),
        }
    }

    fn parse_ldstb_h(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rt = self.parse_w_reg(&format!("{} data operand", mnemonic))?;
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg(&format!("{} memory base", mnemonic))?;
        if self.eat(&Tok::RBracket) {
            if self.eat(&Tok::Comma) {
                let offset = self.parse_i16_signed_scaled_immediate("post-index offset", 9, 0)?;
                return Ok(self.parse_ldstb_h_post_inst(mnemonic, rt, rn, offset));
            }
            return self.parse_ldstb_h_offset_inst(mnemonic, rt, rn, 0, self.pos);
        }

        self.expect(&Tok::Comma)?;
        if self.starts_register_like_operand() {
            let scale = match mnemonic {
                "ldrb" | "strb" => 0,
                "ldrh" | "strh" => 1,
                _ => unreachable!(),
            };
            let (rm, extend, shift) = self.parse_reg_offset_operand(scale)?;
            self.expect(&Tok::RBracket)?;
            return Ok(self.parse_ldstb_h_reg_inst(mnemonic, rt, rn, rm, extend, shift));
        }

        let offset_start = self.pos;
        let offset = self.parse_immediate_const_expr("memory offset")?;
        self.expect(&Tok::RBracket)?;
        if self.eat(&Tok::Bang) {
            let offset = self.validate_i16_signed_scaled_immediate(
                offset,
                "memory offset",
                9,
                0,
                offset_start,
            )?;
            return Ok(self.parse_ldstb_h_pre_inst(mnemonic, rt, rn, offset));
        }
        self.parse_ldstb_h_offset_inst(mnemonic, rt, rn, offset, offset_start)
    }

    fn parse_ldst_signed_b_h(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rt, sf) = self.parse_gp_data_reg_with_size(mnemonic)?;
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg(&format!("{} memory base", mnemonic))?;
        if self.eat(&Tok::RBracket) {
            if self.eat(&Tok::Comma) {
                let offset = self.parse_i16_signed_scaled_immediate("post-index offset", 9, 0)?;
                return Ok(self.parse_ldst_signed_b_h_post_inst(mnemonic, rt, sf, rn, offset));
            }
            return self.parse_ldst_signed_b_h_offset_inst(mnemonic, rt, sf, rn, 0, self.pos);
        }

        self.expect(&Tok::Comma)?;
        if self.starts_register_like_operand() {
            let scale = match mnemonic {
                "ldrsb" => 0,
                "ldrsh" => 1,
                _ => unreachable!(),
            };
            let (rm, extend, shift) = self.parse_reg_offset_operand(scale)?;
            self.expect(&Tok::RBracket)?;
            return Ok(self.parse_ldst_signed_b_h_reg_inst(
                mnemonic,
                (rt, sf),
                rn,
                rm,
                extend,
                shift,
            ));
        }

        let offset_start = self.pos;
        let offset = self.parse_immediate_const_expr("memory offset")?;
        self.expect(&Tok::RBracket)?;
        if self.eat(&Tok::Bang) {
            let offset = self.validate_i16_signed_scaled_immediate(
                offset,
                "memory offset",
                9,
                0,
                offset_start,
            )?;
            return Ok(self.parse_ldst_signed_b_h_pre_inst(mnemonic, rt, sf, rn, offset));
        }
        self.parse_ldst_signed_b_h_offset_inst(mnemonic, rt, sf, rn, offset, offset_start)
    }

    fn parse_ldrsw(&mut self) -> Result<Stmt, ParseError> {
        let rt_start = self.pos;
        let (rt, sf) = self.parse_gp_data_reg_with_size("ldrsw destination")?;
        if !sf {
            return Err(self.err_at(rt_start, "ldrsw destination must be an x-register".into()));
        }
        self.expect(&Tok::Comma)?;

        if self.starts_immediate_expr() {
            let offset = self.parse_i32_signed_scaled_immediate("ldrsw literal offset", 19, 2)?;
            return Ok(Stmt::Instruction(Inst::LdrswLit { rt, offset }));
        }

        if self.starts_non_register_literal_reference() {
            let label = self.parse_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            return Ok(Stmt::InstructionWithReloc(
                Inst::LdrswLit { rt, offset: 0 },
                LabelRef {
                    symbol: label,
                    kind: RelocKind::Literal19,
                    addend,
                },
            ));
        }

        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg("ldrsw memory base")?;
        let (offset, offset_start) = if self.eat(&Tok::Comma) {
            if self.starts_register_like_operand() {
                let (rm, extend, shift) = self.parse_reg_offset_operand(2)?;
                self.expect(&Tok::RBracket)?;
                return Ok(Stmt::Instruction(Inst::LdrswReg {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                }));
            }
            let offset_start = self.pos;
            let offset = self.parse_immediate_const_expr("memory offset")?;
            (offset, offset_start)
        } else {
            (0, self.pos)
        };
        self.expect(&Tok::RBracket)?;
        Ok(Stmt::Instruction(self.parse_ldrsw_offset_inst(
            rt,
            rn,
            offset,
            offset_start,
        )?))
    }

    fn parse_ldp_stp(&mut self, is_load: bool) -> Result<Inst, ParseError> {
        if self.starts_fp_register_like_operand() {
            return self.parse_ldp_stp_fp(is_load);
        }
        self.parse_ldp_stp_gp(is_load)
    }

    fn parse_ldp_stp_gp(&mut self, is_load: bool) -> Result<Inst, ParseError> {
        let (rt1, sf) = self.parse_gp_data_reg_with_size("ldp/stp data operand")?;
        self.expect(&Tok::Comma)?;
        let rt2_start = self.pos;
        let (rt2, second_sf) = self.parse_gp_data_reg_with_size("ldp/stp data operand")?;
        if sf != second_sf {
            return Err(self.err_at(
                rt2_start,
                "ldp/stp register pair must use matching register widths".into(),
            ));
        }
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg("ldp/stp memory base")?;
        let scale = if sf { 3 } else { 2 };

        if self.eat(&Tok::RBracket) {
            if self.eat(&Tok::Comma) {
                let offset =
                    self.parse_i16_signed_scaled_immediate("pair post-index offset", 7, scale)?;
                return Ok(match (is_load, sf) {
                    (true, true) => Inst::LdpPost64 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (false, true) => Inst::StpPost64 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (true, false) => Inst::LdpPost32 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (false, false) => Inst::StpPost32 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                });
            }

            return Ok(match (is_load, sf) {
                (true, true) => Inst::LdpOff64 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (false, true) => Inst::StpOff64 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (true, false) => Inst::LdpOff32 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (false, false) => Inst::StpOff32 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
            });
        }

        self.expect(&Tok::Comma)?;
        let offset = self.parse_i16_signed_scaled_immediate("pair offset", 7, scale)?;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            // Pre-index
            return Ok(match (is_load, sf) {
                (true, true) => Inst::LdpPre64 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (false, true) => Inst::StpPre64 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (true, false) => Inst::LdpPre32 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (false, false) => Inst::StpPre32 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
            });
        }

        // Signed offset
        Ok(match (is_load, sf) {
            (true, true) => Inst::LdpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (false, true) => Inst::StpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (true, false) => Inst::LdpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (false, false) => Inst::StpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            },
        })
    }

    fn parse_ldp_stp_fp(&mut self, is_load: bool) -> Result<Inst, ParseError> {
        let (rt1, width) = self.parse_fp_mem_reg_with_width()?;
        self.expect(&Tok::Comma)?;
        let rt2_start = self.pos;
        let (rt2, second_width) = self.parse_fp_mem_reg_with_width()?;
        if width != second_width {
            return Err(self.err_at(
                rt2_start,
                "ldp/stp FP register pair must use matching register widths".into(),
            ));
        }
        if matches!(width, FpMemWidth::B8 | FpMemWidth::H16) {
            return Err(self.err("ldp/stp does not support b/h FP register pairs".into()));
        }
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let rn = self.parse_memory_base_reg("ldp/stp FP memory base")?;
        let scale = width.scale();

        if self.eat(&Tok::RBracket) {
            if self.eat(&Tok::Comma) {
                let offset =
                    self.parse_i16_signed_scaled_immediate("pair post-index offset", 7, scale)?;
                return Ok(match (is_load, width) {
                    (_, FpMemWidth::B8 | FpMemWidth::H16) => unreachable!(),
                    (true, FpMemWidth::D64) => Inst::LdpFpPost64 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (false, FpMemWidth::D64) => Inst::StpFpPost64 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (true, FpMemWidth::S32) => Inst::LdpFpPost32 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (false, FpMemWidth::S32) => Inst::StpFpPost32 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (true, FpMemWidth::Q128) => Inst::LdpFpPost128 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (false, FpMemWidth::Q128) => Inst::StpFpPost128 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                });
            }

            return Ok(match (is_load, width) {
                (_, FpMemWidth::B8 | FpMemWidth::H16) => unreachable!(),
                (true, FpMemWidth::D64) => Inst::LdpFpOff64 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (false, FpMemWidth::D64) => Inst::StpFpOff64 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (true, FpMemWidth::S32) => Inst::LdpFpOff32 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (false, FpMemWidth::S32) => Inst::StpFpOff32 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (true, FpMemWidth::Q128) => Inst::LdpFpOff128 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (false, FpMemWidth::Q128) => Inst::StpFpOff128 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
            });
        }

        self.expect(&Tok::Comma)?;
        let offset = self.parse_i16_signed_scaled_immediate("pair offset", 7, scale)?;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            return Ok(match (is_load, width) {
                (_, FpMemWidth::B8 | FpMemWidth::H16) => unreachable!(),
                (true, FpMemWidth::D64) => Inst::LdpFpPre64 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (false, FpMemWidth::D64) => Inst::StpFpPre64 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (true, FpMemWidth::S32) => Inst::LdpFpPre32 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (false, FpMemWidth::S32) => Inst::StpFpPre32 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (true, FpMemWidth::Q128) => Inst::LdpFpPre128 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (false, FpMemWidth::Q128) => Inst::StpFpPre128 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
            });
        }

        Ok(match (is_load, width) {
            (_, FpMemWidth::B8 | FpMemWidth::H16) => unreachable!(),
            (true, FpMemWidth::D64) => Inst::LdpFpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (false, FpMemWidth::D64) => Inst::StpFpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (true, FpMemWidth::S32) => Inst::LdpFpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (false, FpMemWidth::S32) => Inst::StpFpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (true, FpMemWidth::Q128) => Inst::LdpFpOff128 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (false, FpMemWidth::Q128) => Inst::StpFpOff128 {
                rt1,
                rt2,
                rn,
                offset,
            },
        })
    }

    // ---- FP instruction parsers ----

    fn parse_fp_arith(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_fp_reg_matching_width(is_double, mnemonic)?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_fp_reg_matching_width(is_double, mnemonic)?;
        Ok(match (mnemonic, is_double) {
            ("fadd", true) => Inst::FaddD { rd, rn, rm },
            ("fadd", false) => Inst::FaddS { rd, rn, rm },
            ("fsub", true) => Inst::FsubD { rd, rn, rm },
            ("fsub", false) => Inst::FsubS { rd, rn, rm },
            ("fmul", true) => Inst::FmulD { rd, rn, rm },
            ("fmul", false) => Inst::FmulS { rd, rn, rm },
            ("fdiv", true) => Inst::FdivD { rd, rn, rm },
            ("fdiv", false) => Inst::FdivS { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_fp_arith_4s(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "fadd.4s" => Inst::FaddV4S { rd, rn, rm },
            "faddp.4s" => Inst::FaddpV4S { rd, rn, rm },
            "fmaxp.4s" => Inst::FmaxpV4S { rd, rn, rm },
            "fminp.4s" => Inst::FminpV4S { rd, rn, rm },
            "fmaxnmp.4s" => Inst::FmaxnmpV4S { rd, rn, rm },
            "fminnmp.4s" => Inst::FminnmpV4S { rd, rn, rm },
            "fmla.4s" => Inst::FmlaV4S { rd, rn, rm },
            "fmls.4s" => Inst::FmlsV4S { rd, rn, rm },
            "fmax.4s" => Inst::FmaxV4S { rd, rn, rm },
            "fmin.4s" => Inst::FminV4S { rd, rn, rm },
            "fmaxnm.4s" => Inst::FmaxnmV4S { rd, rn, rm },
            "fminnm.4s" => Inst::FminnmV4S { rd, rn, rm },
            "fsub.4s" => Inst::FsubV4S { rd, rn, rm },
            "fmul.4s" => Inst::FmulV4S { rd, rn, rm },
            "fdiv.4s" => Inst::FdivV4S { rd, rn, rm },
            "frecps.4s" => Inst::FrecpsV4S { rd, rn, rm },
            "frsqrts.4s" => Inst::FrsqrtsV4S { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_fp_arith_2d(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "fadd.2d" => Inst::FaddV2D { rd, rn, rm },
            "fsub.2d" => Inst::FsubV2D { rd, rn, rm },
            "fmul.2d" => Inst::FmulV2D { rd, rn, rm },
            "fdiv.2d" => Inst::FdivV2D { rd, rn, rm },
            "fabd.2d" => Inst::FabdV2D { rd, rn, rm },
            "fmla.2d" => Inst::FmlaV2D { rd, rn, rm },
            "fmls.2d" => Inst::FmlsV2D { rd, rn, rm },
            "fmax.2d" => Inst::FmaxV2D { rd, rn, rm },
            "fmin.2d" => Inst::FminV2D { rd, rn, rm },
            "fmaxnm.2d" => Inst::FmaxnmV2D { rd, rn, rm },
            "fminnm.2d" => Inst::FminnmV2D { rd, rn, rm },
            "faddp.2d" => Inst::FaddpV2D { rd, rn, rm },
            "fmaxp.2d" => Inst::FmaxpV2D { rd, rn, rm },
            "fminp.2d" => Inst::FminpV2D { rd, rn, rm },
            "fmaxnmp.2d" => Inst::FmaxnmpV2D { rd, rn, rm },
            "fminnmp.2d" => Inst::FminnmpV2D { rd, rn, rm },
            "frecps.2d" => Inst::FrecpsV2D { rd, rn, rm },
            "frsqrts.2d" => Inst::FrsqrtsV2D { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_fp_unary_4s(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "fabs.4s" => Inst::FabsV4S { rd, rn },
            "fneg.4s" => Inst::FnegV4S { rd, rn },
            "fsqrt.4s" => Inst::FsqrtV4S { rd, rn },
            "scvtf.4s" => Inst::ScvtfV4S { rd, rn },
            "ucvtf.4s" => Inst::UcvtfV4S { rd, rn },
            "fcvtzs.4s" => Inst::FcvtzsV4S { rd, rn },
            "fcvtzu.4s" => Inst::FcvtzuV4S { rd, rn },
            "frecpe.4s" => Inst::FrecpeV4S { rd, rn },
            "frsqrte.4s" => Inst::FrsqrteV4S { rd, rn },
            "frintn.4s" => Inst::FrintnV4S { rd, rn },
            "frintm.4s" => Inst::FrintmV4S { rd, rn },
            "frintp.4s" => Inst::FrintpV4S { rd, rn },
            "frintz.4s" => Inst::FrintzV4S { rd, rn },
            "frinta.4s" => Inst::FrintaV4S { rd, rn },
            "frinti.4s" => Inst::FrintiV4S { rd, rn },
            _ => unreachable!(),
        })
    }

    fn parse_simd_fp_unary_2d(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "fabs.2d" => Inst::FabsV2D { rd, rn },
            "fneg.2d" => Inst::FnegV2D { rd, rn },
            "fsqrt.2d" => Inst::FsqrtV2D { rd, rn },
            "scvtf.2d" => Inst::ScvtfV2D { rd, rn },
            "ucvtf.2d" => Inst::UcvtfV2D { rd, rn },
            "fcvtzs.2d" => Inst::FcvtzsV2D { rd, rn },
            "fcvtzu.2d" => Inst::FcvtzuV2D { rd, rn },
            "frecpe.2d" => Inst::FrecpeV2D { rd, rn },
            "frsqrte.2d" => Inst::FrsqrteV2D { rd, rn },
            "frintn.2d" => Inst::FrintnV2D { rd, rn },
            "frintm.2d" => Inst::FrintmV2D { rd, rn },
            "frintp.2d" => Inst::FrintpV2D { rd, rn },
            "frintz.2d" => Inst::FrintzV2D { rd, rn },
            "frinta.2d" => Inst::FrintaV2D { rd, rn },
            "frinti.2d" => Inst::FrintiV2D { rd, rn },
            _ => unreachable!(),
        })
    }

    fn parse_simd_int_arith_4s(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "add.4s" => Inst::AddV4S { rd, rn, rm },
            "addp.4s" => Inst::AddpV4S { rd, rn, rm },
            "smax.4s" => Inst::SmaxV4S { rd, rn, rm },
            "smaxp.4s" => Inst::SmaxpV4S { rd, rn, rm },
            "smin.4s" => Inst::SminV4S { rd, rn, rm },
            "sminp.4s" => Inst::SminpV4S { rd, rn, rm },
            "sub.4s" => Inst::SubV4S { rd, rn, rm },
            "umax.4s" => Inst::UmaxV4S { rd, rn, rm },
            "umaxp.4s" => Inst::UmaxpV4S { rd, rn, rm },
            "umin.4s" => Inst::UminV4S { rd, rn, rm },
            "uminp.4s" => Inst::UminpV4S { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_int_arith_8h(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "addp.8h" => Inst::AddpV8H { rd, rn, rm },
            "smaxp.8h" => Inst::SmaxpV8H { rd, rn, rm },
            "sminp.8h" => Inst::SminpV8H { rd, rn, rm },
            "umaxp.8h" => Inst::UmaxpV8H { rd, rn, rm },
            "uminp.8h" => Inst::UminpV8H { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_int_arith_16b(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "addp.16b" => Inst::AddpV16B { rd, rn, rm },
            "smaxp.16b" => Inst::SmaxpV16B { rd, rn, rm },
            "sminp.16b" => Inst::SminpV16B { rd, rn, rm },
            "umaxp.16b" => Inst::UmaxpV16B { rd, rn, rm },
            "uminp.16b" => Inst::UminpV16B { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_int_arith_2d(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "addp.2d" => Inst::AddpV2D { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_fpairwise_or_reduce_2d(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        if self.peek_is_scalar_fp_reg() {
            self.parse_simd_reduce_2d(mnemonic)
        } else {
            self.parse_simd_fp_arith_2d(mnemonic)
        }
    }

    fn parse_simd_reduce_4s(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        if is_double {
            return Err(self.err(format!("{} requires an s-register destination", mnemonic)));
        }
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "addv.4s" => Inst::AddvV4S { rd, rn },
            "faddp.2s" => Inst::FaddpV2S { rd, rn },
            "fmaxv.4s" => Inst::FmaxvV4S { rd, rn },
            "fminv.4s" => Inst::FminvV4S { rd, rn },
            "fmaxnmv.4s" => Inst::FmaxnmvV4S { rd, rn },
            "fminnmv.4s" => Inst::FminnmvV4S { rd, rn },
            "umaxv.4s" => Inst::UmaxvV4S { rd, rn },
            "smaxv.4s" => Inst::SmaxvV4S { rd, rn },
            "uminv.4s" => Inst::UminvV4S { rd, rn },
            "sminv.4s" => Inst::SminvV4S { rd, rn },
            _ => unreachable!(),
        })
    }

    fn parse_simd_reduce_2d(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        if !is_double {
            return Err(self.err(format!("{} requires a d-register destination", mnemonic)));
        }
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "faddp.2d" => Inst::FaddpV2DScalar { rd, rn },
            "fmaxp.2d" => Inst::FmaxpV2DScalar { rd, rn },
            "fminp.2d" => Inst::FminpV2DScalar { rd, rn },
            "fmaxnmp.2d" => Inst::FmaxnmpV2DScalar { rd, rn },
            "fminnmp.2d" => Inst::FminnmpV2DScalar { rd, rn },
            _ => unreachable!(),
        })
    }

    fn parse_simd_reduce_16b(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, width) = self.parse_fp_mem_reg_with_width()?;
        if width != FpMemWidth::B8 {
            return Err(self.err(format!("{} requires a b-register destination", mnemonic)));
        }
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "addv.16b" => Inst::AddvV16B { rd, rn },
            "umaxv.16b" => Inst::UmaxvV16B { rd, rn },
            "smaxv.16b" => Inst::SmaxvV16B { rd, rn },
            "uminv.16b" => Inst::UminvV16B { rd, rn },
            "sminv.16b" => Inst::SminvV16B { rd, rn },
            _ => unreachable!(),
        })
    }

    fn parse_simd_reduce_8h(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, width) = self.parse_fp_mem_reg_with_width()?;
        if width != FpMemWidth::H16 {
            return Err(self.err(format!("{} requires an h-register destination", mnemonic)));
        }
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "addv.8h" => Inst::AddvV8H { rd, rn },
            "umaxv.8h" => Inst::UmaxvV8H { rd, rn },
            "smaxv.8h" => Inst::SmaxvV8H { rd, rn },
            "uminv.8h" => Inst::UminvV8H { rd, rn },
            "sminv.8h" => Inst::SminvV8H { rd, rn },
            _ => unreachable!(),
        })
    }

    fn parse_simd_logic_16b(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "and.16b" => Inst::AndV16B { rd, rn, rm },
            "bic.16b" => Inst::BicV16B { rd, rn, rm },
            "bif.16b" => Inst::BifV16B { rd, rn, rm },
            "bit.16b" => Inst::BitV16B { rd, rn, rm },
            "bsl.16b" => Inst::BslV16B { rd, rn, rm },
            "orr.16b" => Inst::OrrV16B { rd, rn, rm },
            "eor.16b" => Inst::EorV16B { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_compare_4s(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "cmeq.4s" => Inst::CmeqV4S { rd, rn, rm },
            "fcmeq.4s" => Inst::FcmeqV4S { rd, rn, rm },
            "cmhs.4s" => Inst::CmhsV4S { rd, rn, rm },
            "cmhi.4s" => Inst::CmhiV4S { rd, rn, rm },
            "cmge.4s" => Inst::CmgeV4S { rd, rn, rm },
            "fcmge.4s" => Inst::FcmgeV4S { rd, rn, rm },
            "cmgt.4s" => Inst::CmgtV4S { rd, rn, rm },
            "fcmgt.4s" => Inst::FcmgtV4S { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_compare_2d(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        if matches!(
            self.peek(),
            Tok::Hash | Tok::Integer(_) | Tok::UnsignedInteger(_) | Tok::Float(_)
        ) {
            self.parse_fp_zero_immediate("vector FP compare immediate")?;
            return Ok(match mnemonic {
                "fcmge.2d" => Inst::FcmgeZeroV2D { rd, rn },
                "fcmgt.2d" => Inst::FcmgtZeroV2D { rd, rn },
                "fcmle.2d" => Inst::FcmleZeroV2D { rd, rn },
                "fcmlt.2d" => Inst::FcmltZeroV2D { rd, rn },
                _ => {
                    return Err(self.err(format!(
                        "{} does not support a zero immediate compare form",
                        mnemonic
                    )))
                }
            });
        }
        if matches!(mnemonic, "fcmle.2d" | "fcmlt.2d") {
            return Err(self.err(format!("{} requires a #0.0 immediate", mnemonic)));
        }
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "fcmeq.2d" => Inst::FcmeqV2D { rd, rn, rm },
            "fcmge.2d" => Inst::FcmgeV2D { rd, rn, rm },
            "fcmgt.2d" => Inst::FcmgtV2D { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_ext_16b(&mut self) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let index_value = self.parse_immediate_const_expr("vector extract offset")?;
        let index = u8::try_from(index_value).map_err(|_| {
            self.err(format!(
                "vector extract offset {} out of range",
                index_value
            ))
        })?;
        if index > 15 {
            return Err(self.err(format!(
                "vector extract offset {} out of range",
                index_value
            )));
        }
        Ok(Inst::ExtV16B { rd, rn, rm, index })
    }

    fn parse_simd_table_lookup(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let (table, table_len) = self.parse_simd_table_list()?;
        self.expect(&Tok::Comma)?;
        let index = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "tbl.16b" => Inst::TblV16B {
                rd,
                table,
                table_len,
                index,
            },
            "tbx.16b" => Inst::TbxV16B {
                rd,
                table,
                table_len,
                index,
            },
            _ => unreachable!(),
        })
    }

    fn parse_simd_table_list(&mut self) -> Result<(FpReg, u8), ParseError> {
        self.expect(&Tok::LBrace)?;
        let first = self.parse_simd_reg()?;
        let mut prev = first;
        let mut count = 1u8;
        while self.eat(&Tok::Comma) {
            let next = self.parse_simd_reg()?;
            if next.num() != prev.num() + 1 {
                return Err(self.err("SIMD table register list must be consecutive".to_string()));
            }
            count += 1;
            if count > 4 {
                return Err(
                    self.err("SIMD table register list supports at most 4 registers".to_string())
                );
            }
            prev = next;
        }
        self.expect(&Tok::RBrace)?;
        Ok((first, count))
    }

    fn parse_simd_rev64_4s(&mut self) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        Ok(Inst::Rev64V4S { rd, rn })
    }

    fn parse_simd_shuffle_4s(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "zip1.4s" => Inst::Zip1V4S { rd, rn, rm },
            "zip2.4s" => Inst::Zip2V4S { rd, rn, rm },
            "uzp1.4s" => Inst::Uzp1V4S { rd, rn, rm },
            "uzp2.4s" => Inst::Uzp2V4S { rd, rn, rm },
            "trn1.4s" => Inst::Trn1V4S { rd, rn, rm },
            "trn2.4s" => Inst::Trn2V4S { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_shuffle_2d(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "zip1.2d" => Inst::Zip1V2D { rd, rn, rm },
            "zip2.2d" => Inst::Zip2V2D { rd, rn, rm },
            "uzp1.2d" => Inst::Uzp1V2D { rd, rn, rm },
            "uzp2.2d" => Inst::Uzp2V2D { rd, rn, rm },
            "trn1.2d" => Inst::Trn1V2D { rd, rn, rm },
            "trn2.2d" => Inst::Trn2V2D { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_simd_mov(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_simd_reg()?;
        Ok(match mnemonic {
            "mov.8b" => Inst::MovV8B { rd, rn },
            "mov.16b" => Inst::MovV16B { rd, rn },
            "mov.4s" => Inst::MovV4S { rd, rn },
            "mov.2d" => Inst::MovV2D { rd, rn },
            _ => unreachable!(),
        })
    }

    fn parse_simd_dup(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (width, make_inst): (SimdLaneWidth, fn(FpReg, FpReg, u8) -> Inst) = match mnemonic {
            "dup.16b" => (SimdLaneWidth::B8, |rd, rn, index| Inst::DupV16B {
                rd,
                rn,
                index,
            }),
            "dup.8h" => (SimdLaneWidth::H16, |rd, rn, index| Inst::DupV8H {
                rd,
                rn,
                index,
            }),
            "dup.4s" => (SimdLaneWidth::S32, |rd, rn, index| Inst::DupV4S {
                rd,
                rn,
                index,
            }),
            "dup.2d" => (SimdLaneWidth::D64, |rd, rn, index| Inst::DupV2D {
                rd,
                rn,
                index,
            }),
            _ => unreachable!(),
        };
        let rd = self.parse_simd_reg()?;
        self.expect(&Tok::Comma)?;
        let (rn, index) = self.parse_simd_lane_ref(width)?;
        Ok(make_inst(rd, rn, index))
    }

    fn parse_simd_lane_ref(&mut self, width: SimdLaneWidth) -> Result<(FpReg, u8), ParseError> {
        let reg = self.parse_simd_reg()?;
        self.expect(&Tok::LBracket)?;
        let index = self.parse_lane_index(width.max_index())?;
        self.expect(&Tok::RBracket)?;
        Ok((reg, index))
    }

    fn parse_lane_index(&mut self, max_index: u8) -> Result<u8, ParseError> {
        let token = self.peek().clone();
        let value = match token {
            Tok::Integer(value) => {
                self.advance();
                value
            }
            other => return Err(self.err(format!("expected lane index, got {}", other))),
        };
        let index = u8::try_from(value)
            .map_err(|_| self.err(format!("lane index {} out of range", value)))?;
        if index > max_index {
            return Err(self.err(format!("lane index {} out of range for lane width", value)));
        }
        Ok(index)
    }

    fn parse_fp_unary(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_fp_reg_matching_width(is_double, mnemonic)?;
        Ok(match (mnemonic, is_double) {
            ("fneg", true) => Inst::FnegD { rd, rn },
            ("fneg", false) => Inst::FnegS { rd, rn },
            ("fabs", true) => Inst::FabsD { rd, rn },
            ("fabs", false) => Inst::FabsS { rd, rn },
            ("fsqrt", true) => Inst::FsqrtD { rd, rn },
            ("fsqrt", false) => Inst::FsqrtS { rd, rn },
            _ => unreachable!(),
        })
    }

    fn parse_fcmp(&mut self) -> Result<Inst, ParseError> {
        let (rn, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_fp_reg_matching_width(is_double, "fcmp")?;
        if is_double {
            Ok(Inst::FcmpD { rn, rm })
        } else {
            Ok(Inst::FcmpS { rn, rm })
        }
    }

    fn parse_fcsel(&mut self) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_fp_reg_matching_width(is_double, "fcsel")?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_fp_reg_matching_width(is_double, "fcsel")?;
        self.expect(&Tok::Comma)?;
        let cond_name = self.expect_ident()?;
        let cond = parse_condition(&cond_name)
            .ok_or_else(|| self.err(format!("unknown condition: {}", cond_name)))?;
        if is_double {
            Ok(Inst::FcselD { rd, rn, rm, cond })
        } else {
            Ok(Inst::FcselS { rd, rn, rm, cond })
        }
    }

    fn parse_fmadd(&mut self) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_fp_reg_matching_width(is_double, "fmadd")?;
        self.expect(&Tok::Comma)?;
        let rm = self.parse_fp_reg_matching_width(is_double, "fmadd")?;
        self.expect(&Tok::Comma)?;
        let ra = self.parse_fp_reg_matching_width(is_double, "fmadd")?;
        if is_double {
            Ok(Inst::FmaddD { rd, rn, rm, ra })
        } else {
            Ok(Inst::FmaddS { rd, rn, rm, ra })
        }
    }

    fn parse_fcvtzs(&mut self) -> Result<Inst, ParseError> {
        let (rd, dst_64bit) = self.parse_gp_data_reg_with_size("fcvtzs")?;
        self.expect(&Tok::Comma)?;
        let (rn, src_double) = self.parse_fp_reg_with_size()?;
        Ok(Inst::Fcvtzs {
            rd,
            rn,
            dst_64bit,
            src_double,
        })
    }

    fn parse_fcvt(&mut self) -> Result<Inst, ParseError> {
        let (rd, rd_is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, rn_is_double) = self.parse_fp_reg_with_size()?;
        match (rd_is_double, rn_is_double) {
            (true, false) => Ok(Inst::FcvtDFromS { rd, rn }),
            (false, true) => Ok(Inst::FcvtSFromD { rd, rn }),
            _ => Err(self.err("fcvt requires one s-register and one d-register".into())),
        }
    }

    fn parse_scvtf(&mut self) -> Result<Inst, ParseError> {
        let (rd, dst_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, src_64bit) = self.parse_gp_data_reg_with_size("scvtf")?;
        Ok(Inst::Scvtf {
            rd,
            rn,
            dst_double,
            src_64bit,
        })
    }

    fn parse_fmov(&mut self) -> Result<Inst, ParseError> {
        let rd_start = self.pos;
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        self.expect(&Tok::Comma)?;
        let rn_start = self.pos;

        if lower.starts_with('d') || lower.starts_with('s') {
            let rd = parse_fp_reg_name(&lower)
                .ok_or_else(|| self.err_at(rd_start, format!("bad FP register '{}'", name)))?;
            let is_double = lower.starts_with('d');
            match self.peek() {
                Tok::Integer(_) | Tok::UnsignedInteger(_) | Tok::Float(_) => {
                    let imm8 = self.parse_fp_modified_immediate(is_double)?;
                    if is_double {
                        Ok(Inst::FmovImmD { rd, imm8 })
                    } else {
                        Ok(Inst::FmovImmS { rd, imm8 })
                    }
                }
                Tok::Ident(src) if looks_like_fp_register_spelling(src) => {
                    let (rn, src_is_double) = self.parse_fp_reg_with_size()?;
                    if src_is_double != is_double {
                        Err(self.err_at(
                            rn_start,
                            "fmov register copy requires matching FP widths".into(),
                        ))
                    } else if is_double {
                        Ok(Inst::FmovRegD { rd, rn })
                    } else {
                        Ok(Inst::FmovRegS { rd, rn })
                    }
                }
                _ => {
                    // FMOV Dd, Xn or FMOV Sd, Wn (GP → FP)
                    let (rn, sf, kind) = self.parse_gp_reg_with_size_kind()?;
                    if kind == GpRegKind::Sp {
                        return Err(self.err_at(rn_start, "fmov does not allow SP".into()));
                    }
                    if is_double {
                        if !sf {
                            return Err(self.err_at(
                                rn_start,
                                "fmov dN, ... requires an x-register source".into(),
                            ));
                        }
                        Ok(Inst::FmovToD { rd, rn })
                    } else {
                        if sf {
                            return Err(self.err_at(
                                rn_start,
                                "fmov sN, ... requires a w-register source".into(),
                            ));
                        }
                        Ok(Inst::FmovToS { rd, rn })
                    }
                }
            }
        } else {
            // FMOV Xd, Dn or FMOV Wd, Sn (FP → GP)
            let (rd, sf, kind) = self.gp_reg_with_size_kind_from_name(&name, &lower, rd_start)?;
            if kind == GpRegKind::Sp {
                return Err(self.err_at(rd_start, "fmov does not allow SP".into()));
            }
            let (rn, is_double) = self.parse_fp_reg_with_size()?;
            match (sf, is_double) {
                (true, true) => Ok(Inst::FmovFromD { rd, rn }),
                (false, false) => Ok(Inst::FmovFromS { rd, rn }),
                (true, false) => {
                    Err(self.err_at(rn_start, "fmov xN, ... requires a d-register source".into()))
                }
                (false, true) => Err(self.err_at(
                    rn_start,
                    "fmov wN, ... requires an s-register source".into(),
                )),
            }
        }
    }

    fn parse_simd_reg(&mut self) -> Result<FpReg, ParseError> {
        let start = self.pos;
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        parse_simd_reg_name(&lower)
            .ok_or_else(|| self.err_at(start, format!("expected vector register, got '{}'", name)))
    }

    fn peek_is_gp_reg(&self) -> bool {
        !self.starts_absolute_symbol_expr()
            && matches!(self.peek(), Tok::Ident(name) if looks_like_gp_register_name(name))
    }

    fn peek_is_simd_reg(&self) -> bool {
        matches!(self.peek(), Tok::Ident(name) if looks_like_simd_register_spelling(name))
    }

    fn peek_is_scalar_fp_reg(&self) -> bool {
        matches!(self.peek(), Tok::Ident(name) if looks_like_scalar_fp_register_spelling(name))
    }

    fn parse_fp_modified_immediate(&mut self, is_double: bool) -> Result<u8, ParseError> {
        let literal = match self.peek().clone() {
            Tok::Integer(value) => {
                self.advance();
                value.to_string()
            }
            Tok::Float(value) => {
                self.advance();
                value
            }
            Tok::UnsignedInteger(value) => {
                return Err(self.err(format!(
                    "unsigned integer {} exceeds the i64 range for a floating-point immediate",
                    value
                )))
            }
            other => {
                return Err(self.err(format!("expected floating-point immediate, got {}", other)))
            }
        };

        if is_double {
            encode_fp_modified_immediate64(&literal).ok_or_else(|| {
                self.err(format!(
                    "unsupported floating-point immediate '{}'",
                    literal
                ))
            })
        } else {
            encode_fp_modified_immediate32(&literal).ok_or_else(|| {
                self.err(format!(
                    "unsupported floating-point immediate '{}'",
                    literal
                ))
            })
        }
    }

    fn parse_fp_zero_immediate(&mut self, context: &str) -> Result<(), ParseError> {
        self.eat(&Tok::Hash);
        match self.peek().clone() {
            Tok::Integer(0) => {
                self.advance();
                Ok(())
            }
            Tok::Float(value) => match value.parse::<f64>() {
                Ok(0.0) => {
                    self.advance();
                    Ok(())
                }
                _ => Err(self.err(format!("{} must be #0.0", context))),
            },
            Tok::UnsignedInteger(value) => Err(self.err(format!(
                "unsigned integer {} exceeds the i64 range for {}",
                value, context
            ))),
            other => Err(self.err(format!("{} must be #0.0, got {}", context, other))),
        }
    }

    fn parse_barrier_option(&mut self, context: &str) -> Result<BarrierOpt, ParseError> {
        let name = self.expect_ident()?.to_lowercase();
        match name.as_str() {
            "oshld" => Ok(BarrierOpt::Oshld),
            "oshst" => Ok(BarrierOpt::Oshst),
            "osh" => Ok(BarrierOpt::Osh),
            "nshld" => Ok(BarrierOpt::Nshld),
            "nshst" => Ok(BarrierOpt::Nshst),
            "nsh" => Ok(BarrierOpt::Nsh),
            "ishld" => Ok(BarrierOpt::Ishld),
            "ishst" => Ok(BarrierOpt::Ishst),
            "ish" => Ok(BarrierOpt::Ish),
            "ld" => Ok(BarrierOpt::Ld),
            "st" => Ok(BarrierOpt::St),
            "sy" => Ok(BarrierOpt::Sy),
            _ => Err(self.err(format!("unknown {} '{}'", context, name))),
        }
    }

    // ---- Helpers ----

    fn parse_optional_add_sub_immediate_shift(&mut self) -> Result<Option<bool>, ParseError> {
        let has_lsl = self.peek() == &Tok::Comma
            && matches!(
                self.tokens.get(self.pos + 1).map(|token| &token.kind),
                Some(Tok::Ident(name)) if name.eq_ignore_ascii_case("lsl")
            );
        if !has_lsl {
            return Ok(None);
        }

        self.advance();
        self.advance();
        let amount = self.parse_immediate_const_expr("lsl amount")?;
        match amount {
            0 => Ok(Some(false)),
            12 => Ok(Some(true)),
            _ => Err(self.err(format!(
                "add/sub immediate shift must be lsl #0 or lsl #12, got lsl #{}",
                amount
            ))),
        }
    }

    fn parse_add_sub_immediate(&mut self, context: &str) -> Result<(u16, bool, bool), ParseError> {
        let start = self.pos;
        let value = self.parse_immediate_const_expr(context)?;
        let explicit_shift = self.parse_optional_add_sub_immediate_shift()?;
        let magnitude = value.unsigned_abs();

        let (imm12, shift) = match explicit_shift {
            Some(shift) if magnitude <= 0xFFF => (magnitude, shift),
            Some(_) => {
                return Err(self.err_at(
                    start,
                    format!(
                        "{} {} is not encodable with an explicit shift; expected magnitude 0..=4095",
                        context, value
                    ),
                ));
            }
            None if magnitude <= 0xFFF => (magnitude, false),
            None if magnitude <= (0xFFF << 12) && magnitude % (1 << 12) == 0 => {
                (magnitude >> 12, true)
            }
            None => {
                return Err(self.err_at(
                    start,
                    format!(
                        "{} {} is not encodable; expected magnitude 0..=4095 or a multiple of 4096 through 16773120",
                        context, value
                    ),
                ));
            }
        };

        let imm12 = u16::try_from(imm12)
            .map_err(|_| self.err(format!("{} does not fit in u16", context)))?;
        Ok((imm12, shift, value.is_negative()))
    }

    fn parse_optional_mov_wide_shift(&mut self, sf: bool) -> Result<u8, ParseError> {
        if !self.eat(&Tok::Comma) {
            return Ok(0);
        }

        let modifier = self.expect_ident()?;
        if modifier.to_lowercase() != "lsl" {
            return Err(self.err(format!("expected 'lsl', got '{}'", modifier)));
        }

        let start = self.pos;
        let amount = self.parse_immediate_const_expr("mov wide shift")?;
        let valid = if sf {
            matches!(amount, 0 | 16 | 32 | 48)
        } else {
            matches!(amount, 0 | 16)
        };
        if !valid {
            return Err(self.err_at(
                start,
                format!(
                    "mov wide shift must be one of {} for a {}-bit register, got {}",
                    if sf { "0, 16, 32, or 48" } else { "0 or 16" },
                    if sf { 64 } else { 32 },
                    amount
                ),
            ));
        }

        u8::try_from(amount).map_err(|_| self.err("mov wide shift does not fit in u8".into()))
    }

    fn parse_bit_index(&mut self, sf: bool, context: &str) -> Result<u8, ParseError> {
        let bit = self.parse_immediate_const_expr(context)?;
        let max = if sf { 63 } else { 31 };
        if !(0..=max).contains(&bit) {
            return Err(self.err(format!(
                "{} must be in the range 0..={} for this register width",
                context, max
            )));
        }
        Ok(bit as u8)
    }

    fn parse_optional_add_sub_modifier(
        &mut self,
        sf: bool,
        sets_flags: bool,
        rm: GpOperandMeta,
    ) -> Result<Option<AddSubModifier>, ParseError> {
        if !self.eat(&Tok::Comma) {
            return Ok(None);
        }

        let name = self.expect_ident()?.to_lowercase();
        match name.as_str() {
            "lsl" | "lsr" | "asr" => {
                let shift = match name.as_str() {
                    "lsl" => RegShift::Lsl,
                    "lsr" => RegShift::Lsr,
                    "asr" => RegShift::Asr,
                    _ => unreachable!(),
                };
                let amount = self.parse_immediate_const_expr("shift amount")?;
                let max = if sf { 63 } else { 31 };
                if !(0..=max).contains(&amount) {
                    return Err(self.err(format!(
                        "shift amount must be in the range 0..={} for this register width",
                        max
                    )));
                }
                Ok(Some(AddSubModifier::Shift(shift, amount as u8)))
            }
            "uxtb" | "uxth" | "uxtw" | "uxtx" | "sxtb" | "sxth" | "sxtw" | "sxtx" => {
                let x_extension = matches!(name.as_str(), "uxtx" | "sxtx");
                // LLVM accepts Wm or Xm for UXTX/SXTX only in 64-bit flag-setting forms.
                let valid_width = match (sf, sets_flags, x_extension) {
                    (_, _, false) | (false, _, true) => !rm.is_64bit,
                    (true, false, true) => rm.is_64bit,
                    (true, true, true) => true,
                };
                if !valid_width {
                    let requires_64bit = sf && x_extension;
                    return Err(self.err_at(
                        rm.start,
                        format!(
                            "{} add/sub extensions require a {}-register operand",
                            name,
                            if requires_64bit { "x" } else { "w" }
                        ),
                    ));
                }
                let extend = match name.as_str() {
                    "uxtb" => RegExtend::Uxtb,
                    "uxth" => RegExtend::Uxth,
                    "uxtw" => RegExtend::Uxtw,
                    "uxtx" => RegExtend::Uxtx,
                    "sxtb" => RegExtend::Sxtb,
                    "sxth" => RegExtend::Sxth,
                    "sxtw" => RegExtend::Sxtw,
                    "sxtx" => RegExtend::Sxtx,
                    _ => unreachable!(),
                };
                let amount = if self.starts_immediate_expr() {
                    self.parse_immediate_const_expr("extend shift amount")?
                } else {
                    0
                };
                if !(0..=4).contains(&amount) {
                    return Err(self.err("extend shift amount must be in the range 0..=4".into()));
                }
                Ok(Some(AddSubModifier::Extend(extend, amount as u8)))
            }
            _ => Err(self.err(format!(
                "expected add/sub modifier (lsl/lsr/asr/uxtb/uxth/uxtw/uxtx/sxtb/sxth/sxtw/sxtx), got '{}'",
                name
            ))),
        }
    }

    fn normalize_add_sub_register_modifier(
        &self,
        rd: GpOperandMeta,
        rn: GpOperandMeta,
        rm: GpOperandMeta,
        sets_flags: bool,
        modifier: Option<AddSubModifier>,
    ) -> Result<Option<AddSubModifier>, ParseError> {
        if rm.kind == GpRegKind::Sp {
            return Err(self.err_at(
                rm.start,
                "add/sub register forms do not allow sp as the third operand".into(),
            ));
        }

        let uses_sp = rd.kind == GpRegKind::Sp || rn.kind == GpRegKind::Sp;
        if !uses_sp && rn.is_64bit != rd.is_64bit {
            return Err(self.err_at(
                rn.start,
                "add/sub requires registers of the same width".into(),
            ));
        }
        let modifier = if uses_sp {
            if rn.is_64bit != rd.is_64bit {
                return Err(self.err_at(
                    rn.start,
                    "add/sub register operands involving sp must have matching widths".into(),
                ));
            }

            let default_extend = if rd.is_64bit {
                RegExtend::Uxtx
            } else {
                RegExtend::Uxtw
            };
            match modifier {
                None if rm.is_64bit == rd.is_64bit => {
                    Some(AddSubModifier::Extend(default_extend, 0))
                }
                None => {
                    return Err(self.err_at(
                        rm.start,
                        "add/sub register operands involving sp must have matching widths".into(),
                    ));
                }
                Some(AddSubModifier::Shift(RegShift::Lsl, amount))
                    if rm.is_64bit == rd.is_64bit =>
                {
                    if amount > 4 {
                        return Err(
                            self.err("sp add/sub lsl amount must be in the range 0..=4".into())
                        );
                    }
                    Some(AddSubModifier::Extend(default_extend, amount))
                }
                Some(AddSubModifier::Shift(RegShift::Lsl, _)) => {
                    return Err(self.err_at(
                        rm.start,
                        "add/sub register operands involving sp must have matching widths".into(),
                    ));
                }
                Some(AddSubModifier::Shift(_, _)) => {
                    return Err(self.err(
                        "sp add/sub register forms only support lsl or an extend modifier".into(),
                    ));
                }
                Some(AddSubModifier::Extend(..)) if !rd.is_64bit && rm.is_64bit => {
                    return Err(self.err_at(
                        rm.start,
                        "add/sub register operands involving sp must have matching widths".into(),
                    ));
                }
                Some(modifier @ AddSubModifier::Extend(..)) => Some(modifier),
            }
        } else {
            modifier
        };

        if !uses_sp
            && !matches!(modifier, Some(AddSubModifier::Extend(..)))
            && rm.is_64bit != rd.is_64bit
        {
            return Err(self.err_at(
                rm.start,
                "add/sub requires registers of the same width".into(),
            ));
        }

        if matches!(modifier, Some(AddSubModifier::Extend(..))) {
            self.validate_add_sub_extended_base_reg(rn, modifier)?;
            match (sets_flags, rd.kind) {
                (false, GpRegKind::Zr) => {
                    return Err(self.err_at(
                        rd.start,
                        "extended add/sub forms require a register or sp destination, not xzr/wzr"
                            .into(),
                    ));
                }
                (true, GpRegKind::Sp) => {
                    return Err(self.err_at(
                        rd.start,
                        "flag-setting extended add/sub forms do not allow an sp destination".into(),
                    ));
                }
                _ => {}
            }
        }

        Ok(modifier)
    }

    fn validate_add_sub_immediate_registers(
        &self,
        rd: GpOperandMeta,
        rn: GpOperandMeta,
        sets_flags: bool,
    ) -> Result<(), ParseError> {
        if rd.is_64bit != rn.is_64bit {
            let message = if rd.kind == GpRegKind::Sp || rn.kind == GpRegKind::Sp {
                "add/sub immediate operands involving sp must have matching widths"
            } else {
                "add/sub immediate operands must have matching widths"
            };
            return Err(self.err_at(rn.start, message.into()));
        }
        if rn.kind == GpRegKind::Zr {
            return Err(self.err_at(
                rn.start,
                "add/sub immediate forms require a register or sp base, not xzr/wzr".into(),
            ));
        }
        match (sets_flags, rd.kind) {
            (false, GpRegKind::Zr) => Err(self.err_at(
                rd.start,
                "add/sub immediate forms require a register or sp destination, not xzr/wzr".into(),
            )),
            (true, GpRegKind::Sp) => Err(self.err_at(
                rd.start,
                "flag-setting add/sub immediate forms do not allow an sp destination".into(),
            )),
            _ => Ok(()),
        }
    }

    fn validate_add_sub_extended_base_reg(
        &self,
        rn: GpOperandMeta,
        modifier: Option<AddSubModifier>,
    ) -> Result<(), ParseError> {
        if matches!(modifier, Some(AddSubModifier::Extend(..))) && rn.kind == GpRegKind::Zr {
            return Err(self.err_at(
                rn.start,
                "extended add/sub forms require an x-register or sp base operand, not xzr/wzr"
                    .into(),
            ));
        }
        Ok(())
    }

    fn validate_bitfield_alias_args(
        &self,
        mnemonic: &str,
        sf: bool,
        lsb: i64,
        width: i64,
        lsb_start: usize,
        width_start: usize,
    ) -> Result<(), ParseError> {
        let bits = if sf { 64i64 } else { 32i64 };
        if width <= 0 {
            return Err(self.err_at(
                width_start,
                format!("{} width must be at least 1", mnemonic),
            ));
        }
        if !(0..bits).contains(&lsb) {
            return Err(self.err_at(
                lsb_start,
                format!(
                    "{} lsb {} is out of range for {}-bit register",
                    mnemonic, lsb, bits
                ),
            ));
        }
        if width > bits - lsb {
            return Err(self.err_at(
                width_start,
                format!(
                    "{} width {} with lsb {} exceeds {}-bit register width",
                    mnemonic, width, lsb, bits
                ),
            ));
        }
        Ok(())
    }

    fn starts_register_like_operand(&self) -> bool {
        !self.starts_absolute_symbol_expr()
            && matches!(self.peek(), Tok::Ident(name) if looks_like_gp_register_name(name))
    }

    fn starts_absolute_symbol_expr(&self) -> bool {
        matches!(
            self.peek(),
            Tok::Ident(name)
                if !is_canonical_register_name(name)
                    && self.absolute_symbols.contains_key(name)
        )
    }

    fn starts_fp_register_like_operand(&self) -> bool {
        matches!(self.peek(), Tok::Ident(name) if looks_like_fp_register_spelling(name))
    }

    fn parse_reg_offset_operand(
        &mut self,
        scale: u8,
    ) -> Result<(GpReg, AddrExtend, bool), ParseError> {
        let rm_start = self.pos;
        let (rm, is_64bit, kind) = self.parse_gp_reg_with_size_kind()?;
        if kind == GpRegKind::Sp {
            return Err(self.err_at(rm_start, "register offset does not allow sp".into()));
        }
        let mut extend = AddrExtend::Lsl;
        let mut shift = false;

        if self.eat(&Tok::Comma) {
            let modifier = self.expect_ident()?.to_lowercase();
            extend = match modifier.as_str() {
                "lsl" if is_64bit => AddrExtend::Lsl,
                "uxtw" if !is_64bit => AddrExtend::Uxtw,
                "sxtw" if !is_64bit => AddrExtend::Sxtw,
                "sxtx" if is_64bit => AddrExtend::Sxtx,
                "lsl" => {
                    return Err(self.err("lsl register offsets require an x-register index".into()));
                }
                "uxtw" | "sxtw" => {
                    return Err(self.err(format!(
                        "{} register offsets require a w-register index",
                        modifier
                    )));
                }
                "sxtx" => {
                    return Err(
                        self.err("sxtx register offsets require an x-register index".into())
                    );
                }
                _ => {
                    return Err(self.err(format!(
                        "unsupported register offset modifier '{}'",
                        modifier
                    )));
                }
            };

            let amount = if self.starts_immediate_expr() {
                self.parse_immediate_const_expr("register offset shift")?
            } else {
                0
            };
            shift = parse_index_shift(amount, scale)
                .map_err(|msg| self.err(format!("{} for {}", msg, modifier)))?;
        } else if !is_64bit {
            return Err(self
                .err("32-bit register offsets require an explicit uxtw or sxtw modifier".into()));
        }

        Ok((rm, extend, shift))
    }
}

fn contains_wide_unsigned_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Unsigned(_) => true,
        Expr::UnaryMinus(inner) => contains_wide_unsigned_literal(inner),
        Expr::Add(lhs, rhs) | Expr::Sub(lhs, rhs) => {
            contains_wide_unsigned_literal(lhs) || contains_wide_unsigned_literal(rhs)
        }
        Expr::Int(_) | Expr::Symbol(_) | Expr::ModifiedSymbol { .. } | Expr::CurrentLocation => {
            false
        }
    }
}

pub(crate) fn signed_data_value_fits(value: i64, bits: u8) -> bool {
    let signed_min = -(1i64 << (bits - 1));
    let unsigned_max = (1i64 << bits) - 1;
    (signed_min..=unsigned_max).contains(&value)
}

pub(crate) fn unsigned_data_value_fits(value: u64, bits: u8) -> bool {
    let unsigned_max = (1u64 << bits) - 1;
    let sign_extended_min = u64::MAX - ((1u64 << (bits - 1)) - 1);
    value <= unsigned_max || value >= sign_extended_min
}

// ---- Name resolution helpers ----

fn numeric_label_symbol(number: u32, ordinal: u32) -> String {
    format!(".Ltmp${number}${ordinal}")
}

fn decimal_width(mut value: u32) -> u32 {
    let mut width = 1;
    while value >= 10 {
        value /= 10;
        width += 1;
    }
    width
}

fn classify_gp_reg_name(lower: &str) -> Option<(GpReg, bool, GpRegKind)> {
    match lower {
        "sp" => return Some((SP, true, GpRegKind::Sp)),
        "wsp" => return Some((SP, false, GpRegKind::Sp)),
        "xzr" | "x31" => return Some((XZR, true, GpRegKind::Zr)),
        "wzr" | "w31" => return Some((WZR, false, GpRegKind::Zr)),
        _ => {}
    }

    let (is_64bit, num_str) = if let Some(num) = lower.strip_prefix('x') {
        (true, num)
    } else if let Some(num) = lower.strip_prefix('w') {
        (false, num)
    } else {
        return None;
    };
    let num = parse_canonical_register_number(num_str)?;
    (num <= 30).then(|| (GpReg::new(num), is_64bit, GpRegKind::Reg))
}

fn looks_like_gp_register_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "sp"
        || lower == "wsp"
        || lower == "xzr"
        || lower == "wzr"
        || lower.starts_with('x')
        || lower.starts_with('w')
}

fn parse_fp_reg_name(name: &str) -> Option<FpReg> {
    let lower = name.to_lowercase();
    let (prefix, num_str) = if lower.starts_with('b')
        || lower.starts_with('h')
        || lower.starts_with('d')
        || lower.starts_with('s')
        || lower.starts_with('q')
    {
        (&lower[..1], &lower[1..])
    } else {
        return None;
    };
    let num = parse_canonical_register_number(num_str)?;
    if num > 31 {
        return None;
    }
    let _ = prefix;
    Some(FpReg::new(num))
}

fn parse_simd_reg_name(name: &str) -> Option<FpReg> {
    let lower = name.to_lowercase();
    let num_str = lower.strip_prefix('v')?;
    let num = parse_canonical_register_number(num_str)?;
    if num > 31 {
        return None;
    }
    Some(FpReg::new(num))
}

fn is_canonical_register_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    classify_gp_reg_name(&lower).is_some()
        || parse_fp_reg_name(&lower).is_some()
        || parse_simd_reg_name(&lower).is_some()
}

fn parse_canonical_register_number(suffix: &str) -> Option<u8> {
    if suffix.is_empty() || (suffix.len() > 1 && suffix.starts_with('0')) {
        return None;
    }
    suffix.parse().ok()
}

fn looks_like_fp_register_spelling(name: &str) -> bool {
    let lower = name.to_lowercase();
    let mut chars = lower.chars();
    matches!(chars.next(), Some('b' | 'h' | 'd' | 's' | 'q'))
        && chars.clone().next().is_some()
        && chars.all(|ch| ch.is_ascii_digit())
}

fn looks_like_scalar_fp_register_spelling(name: &str) -> bool {
    let lower = name.to_lowercase();
    let mut chars = lower.chars();
    matches!(chars.next(), Some('d' | 's'))
        && chars.clone().next().is_some()
        && chars.all(|ch| ch.is_ascii_digit())
}

fn looks_like_simd_register_spelling(name: &str) -> bool {
    let lower = name.to_lowercase();
    let Some(suffix) = lower.strip_prefix('v') else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
}

fn parse_condition(s: &str) -> Option<Cond> {
    match s.to_lowercase().as_str() {
        "eq" => Some(Cond::EQ),
        "ne" => Some(Cond::NE),
        "cs" | "hs" => Some(Cond::CS),
        "cc" | "lo" => Some(Cond::CC),
        "mi" => Some(Cond::MI),
        "pl" => Some(Cond::PL),
        "vs" => Some(Cond::VS),
        "vc" => Some(Cond::VC),
        "hi" => Some(Cond::HI),
        "ls" => Some(Cond::LS),
        "ge" => Some(Cond::GE),
        "lt" => Some(Cond::LT),
        "gt" => Some(Cond::GT),
        "le" => Some(Cond::LE),
        "al" => Some(Cond::AL),
        _ => None,
    }
}

fn invert_condition(cond: Cond) -> Cond {
    match cond {
        Cond::EQ => Cond::NE,
        Cond::NE => Cond::EQ,
        Cond::CS => Cond::CC,
        Cond::CC => Cond::CS,
        Cond::MI => Cond::PL,
        Cond::PL => Cond::MI,
        Cond::VS => Cond::VC,
        Cond::VC => Cond::VS,
        Cond::HI => Cond::LS,
        Cond::LS => Cond::HI,
        Cond::GE => Cond::LT,
        Cond::LT => Cond::GE,
        Cond::GT => Cond::LE,
        Cond::LE => Cond::GT,
        Cond::AL => Cond::NV,
        Cond::NV => Cond::AL,
    }
}

fn encode_fp_modified_immediate32(literal: &str) -> Option<u8> {
    let value: f32 = literal.parse().ok()?;
    let bits = value.to_bits();
    (0u8..=u8::MAX).find(|imm8| expand_fp_modified_immediate(*imm8, false) as u32 == bits)
}

fn encode_fp_modified_immediate64(literal: &str) -> Option<u8> {
    let value: f64 = literal.parse().ok()?;
    let bits = value.to_bits();
    (0u8..=u8::MAX).find(|imm8| expand_fp_modified_immediate(*imm8, true) == bits)
}

fn expand_fp_modified_immediate(imm8: u8, is_double: bool) -> u64 {
    let exponent_bits = if is_double { 11 } else { 8 };
    let fraction_bits = if is_double { 52 } else { 23 };
    let sign = ((imm8 >> 7) & 1) as u64;
    let bit6 = ((imm8 >> 6) & 1) as u64;
    let low_exponent = ((imm8 >> 4) & 0b11) as u64;
    let repeated_len = exponent_bits - 3;
    let repeated = if bit6 == 0 {
        0
    } else {
        ((1u64 << repeated_len) - 1) << 2
    };
    let exponent = (((bit6 ^ 1) & 1) << (exponent_bits - 1)) | repeated | low_exponent;
    let fraction = ((imm8 & 0xF) as u64) << (fraction_bits - 4);
    (sign << (exponent_bits + fraction_bits)) | (exponent << fraction_bits) | fraction
}

fn parse_index_shift(amount: i64, scale: u8) -> Result<bool, &'static str> {
    match amount {
        0 => Ok(false),
        value if value == i64::from(scale) => Ok(true),
        _ => Err("register offset shift must be omitted, #0, or the element scale"),
    }
}

fn logical_immediate_encodable(imm: u64, width: u8) -> bool {
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let imm = imm & mask;
    if imm == 0 || imm == mask {
        return false;
    }

    for esize in [2u8, 4, 8, 16, 32, 64] {
        if esize > width {
            continue;
        }
        for ones in 1..esize {
            let base = if ones == 64 {
                u64::MAX
            } else {
                (1u64 << ones) - 1
            };
            for rot in 0..esize {
                let pattern = rotate_right_for_logical_immediate(base, rot, esize);
                let candidate = replicate_logical_immediate_pattern(pattern, esize, width);
                if candidate == imm {
                    return true;
                }
            }
        }
    }

    false
}

fn rotate_right_for_logical_immediate(value: u64, rot: u8, width: u8) -> u64 {
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let value = value & mask;
    let rot = rot % width;
    if rot == 0 {
        value
    } else {
        ((value >> rot) | (value << (width - rot))) & mask
    }
}

fn replicate_logical_immediate_pattern(pattern: u64, esize: u8, width: u8) -> u64 {
    let mut out = 0u64;
    let mut shift = 0u8;
    while shift < width {
        out |= pattern << shift;
        shift += esize;
    }
    if width == 64 {
        out
    } else {
        out & ((1u64 << width) - 1)
    }
}

fn mov_alias_imm(rd: GpReg, imm: i64, sf: bool) -> Option<Inst> {
    let mask = if sf { u64::MAX } else { u32::MAX as u64 };
    let shifts: &[u8] = if sf { &[0, 16, 32, 48] } else { &[0, 16] };
    let value = (imm as u64) & mask;
    let inverted = (!value) & mask;

    for &shift in shifts {
        let shift_bits = shift as u32;
        let movz_imm = ((value >> shift_bits) & 0xFFFF) as u16;
        if value == ((movz_imm as u64) << shift_bits) {
            return Some(Inst::Movz {
                rd,
                imm16: movz_imm,
                shift,
                sf,
            });
        }

        let movn_imm = ((inverted >> shift_bits) & 0xFFFF) as u16;
        if inverted == ((movn_imm as u64) << shift_bits) {
            return Some(Inst::Movn {
                rd,
                imm16: movn_imm,
                shift,
                sf,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_inst(src: &str) -> Inst {
        let stmts = parse(src).unwrap();
        stmts
            .into_iter()
            .find_map(|s| {
                if let Stmt::Instruction(i) = s {
                    Some(i)
                } else {
                    None
                }
            })
            .unwrap()
    }

    fn parse_stmts(src: &str) -> Vec<Stmt> {
        parse(src).unwrap()
    }

    fn parse_err(src: &str) -> String {
        parse(src).unwrap_err().to_string()
    }

    fn immediate_shift_fields(inst: Inst) -> (u8, bool) {
        match inst {
            Inst::LslImm { amount, sf, .. }
            | Inst::LsrImm { amount, sf, .. }
            | Inst::AsrImm { amount, sf, .. } => (amount, sf),
            other => panic!("expected immediate shift, got {other:?}"),
        }
    }

    // ---- Data processing ----

    #[test]
    fn parse_add_reg() {
        assert_eq!(
            parse_inst("add x0, x1, x2"),
            Inst::AddReg {
                rd: X0,
                rn: X1,
                rm: X2,
                sf: true
            }
        );
    }

    #[test]
    fn parse_add_w_reg() {
        assert_eq!(
            parse_inst("add w3, w4, w5"),
            Inst::AddReg {
                rd: W3,
                rn: W4,
                rm: W5,
                sf: false
            }
        );
    }

    #[test]
    fn parse_sp_register_arithmetic_uses_extended_forms() {
        let cases = [
            ("add x0, sp, x1", 0x8B21_63E0),
            ("add sp, x1, x2", 0x8B22_603F),
            ("sub x3, sp, x4", 0xCB24_63E3),
            ("sub sp, x5, x6", 0xCB26_60BF),
            ("adds x7, sp, x8", 0xAB28_63E7),
            ("subs x9, sp, x10", 0xEB2A_63E9),
            ("cmp sp, x11", 0xEB2B_63FF),
            ("cmn sp, x12", 0xAB2C_63FF),
            ("add x13, sp, x14, lsl #4", 0x8B2E_73ED),
            ("sub sp, x15, x16, lsl #2", 0xCB30_69FF),
            ("add w0, wsp, w1", 0x0B21_43E0),
            ("add wsp, w1, w2", 0x0B22_403F),
            ("sub w3, wsp, w4", 0x4B24_43E3),
            ("sub wsp, w5, w6", 0x4B26_40BF),
            ("adds w7, wsp, w8", 0x2B28_43E7),
            ("subs w9, wsp, w10", 0x6B2A_43E9),
            ("cmp wsp, w11", 0x6B2B_43FF),
            ("cmn wsp, w12", 0x2B2C_43FF),
            ("add w13, wsp, w14, lsl #4", 0x0B2E_53ED),
            ("sub wsp, w15, w16, lsl #2", 0x4B30_49FF),
            ("add w17, wsp, w18, uxtw #3", 0x0B32_4FF1),
            ("sub wsp, w19, w20, sxtw #4", 0x4B34_D27F),
            ("sub sp, sp, x16", 0xCB30_63FF),
            ("add sp, sp, x16", 0x8B30_63FF),
            ("sub wsp, wsp, w16", 0x4B30_43FF),
            ("add wsp, wsp, w16", 0x0B30_43FF),
        ];

        for (source, expected_encoding) in cases {
            assert_eq!(
                parse_inst(source).encode(),
                expected_encoding,
                "source: {source}"
            );
        }
    }

    #[test]
    fn parse_sp_immediate_arithmetic_preserves_register_roles() {
        let cases = [
            ("add sp, sp, #16", 0x9100_43FF),
            ("sub sp, sp, #16", 0xD100_43FF),
            ("adds xzr, sp, #1", 0xB100_07FF),
            ("subs xzr, sp, #1", 0xF100_07FF),
            ("add wsp, wsp, #16", 0x1100_43FF),
            ("subs wzr, wsp, #1", 0x7100_07FF),
        ];

        for (source, expected_encoding) in cases {
            assert_eq!(
                parse_inst(source).encode(),
                expected_encoding,
                "source: {source}"
            );
        }
    }

    #[test]
    fn error_sp_register_arithmetic_rejects_non_encodable_forms() {
        let cases = [
            (
                "add x0, sp, x1, lsl #5",
                "lsl amount must be in the range 0..=4",
            ),
            (
                "add x0, sp, x1, lsr #1",
                "only support lsl or an extend modifier",
            ),
            ("add x0, x1, sp", "do not allow sp as the third operand"),
            ("add xzr, sp, x1", "require a register or sp destination"),
            ("adds sp, x1, x2", "do not allow an sp destination"),
            ("add sp, w1, x2", "involving sp must have matching widths"),
            (
                "add w0, wsp, w1, lsl #5",
                "lsl amount must be in the range 0..=4",
            ),
            (
                "add w0, wsp, w1, lsr #1",
                "only support lsl or an extend modifier",
            ),
            ("add w0, w1, wsp", "do not allow sp as the third operand"),
            ("add wzr, wsp, w1", "require a register or sp destination"),
            ("adds wsp, w1, w2", "do not allow an sp destination"),
            ("add wsp, x1, w2", "involving sp must have matching widths"),
            ("add w0, wsp, x1, uxtx", "require a w-register operand"),
            ("add wsp, w1, x2, sxtx", "require a w-register operand"),
            ("cmp wsp, x1, uxtx", "require a w-register operand"),
            ("neg sp, x1", "neg does not allow sp"),
            ("neg x0, sp", "neg does not allow sp"),
            ("neg wsp, w1", "neg does not allow sp"),
            ("neg w0, wsp", "neg does not allow sp"),
            ("add xzr, x1, #1", "require a register or sp destination"),
            ("add x0, xzr, #1", "require a register or sp base"),
            ("adds sp, x1, #1", "do not allow an sp destination"),
            ("subs sp, x1, #1", "do not allow an sp destination"),
            (
                "add xzr, x1, _sym@PAGEOFF",
                "require a register or sp destination",
            ),
            ("add x0, xzr, _sym@PAGEOFF", "require a register or sp base"),
        ];

        for (source, expected) in cases {
            let error = parse_err(source);
            assert!(error.contains(expected), "source: {source}; got: {error}");
        }
    }

    #[test]
    fn parse_sub_imm() {
        assert_eq!(
            parse_inst("sub x0, x1, #42"),
            Inst::SubImm {
                rd: X0,
                rn: X1,
                imm12: 42,
                shift: false,
                sf: true
            }
        );
    }

    #[test]
    fn parse_add_imm_lsl12() {
        assert_eq!(
            parse_inst("add x0, x1, #42, lsl #12"),
            Inst::AddImm {
                rd: X0,
                rn: X1,
                imm12: 42,
                shift: true,
                sf: true
            }
        );
    }

    #[test]
    fn parse_add_immediate_expression() {
        assert_eq!(
            parse_inst("add x0, x1, #1 + 2"),
            Inst::AddImm {
                rd: X0,
                rn: X1,
                imm12: 3,
                shift: false,
                sf: true
            }
        );
    }

    #[test]
    fn parse_add_shifted_reg() {
        assert_eq!(
            parse_inst("add x0, x1, x2, lsl #3"),
            Inst::AddShiftReg {
                rd: X0,
                rn: X1,
                rm: X2,
                shift: RegShift::Lsl,
                amount: 3,
                sf: true
            }
        );
    }

    #[test]
    fn parse_sub_shifted_reg() {
        assert_eq!(
            parse_inst("sub w3, w4, w5, asr #7"),
            Inst::SubShiftReg {
                rd: W3,
                rn: W4,
                rm: W5,
                shift: RegShift::Asr,
                amount: 7,
                sf: false
            }
        );
    }

    #[test]
    fn parse_add_extended_reg() {
        assert_eq!(
            parse_inst("add x0, x0, w1, sxtw #3"),
            Inst::AddExtReg {
                rd: X0,
                rn: X0,
                rm: W1,
                extend: RegExtend::Sxtw,
                amount: 3,
                sf: true
            }
        );
    }

    #[test]
    fn parse_add_extended_reg_with_sp_base() {
        assert_eq!(
            parse_inst("add x11, sp, w12, sxtw #2"),
            Inst::AddExtReg {
                rd: X11,
                rn: SP,
                rm: W12,
                extend: RegExtend::Sxtw,
                amount: 2,
                sf: true
            }
        );
    }

    #[test]
    fn parse_sub_extended_reg() {
        assert_eq!(
            parse_inst("sub x2, x3, w4, uxtw #2"),
            Inst::SubExtReg {
                rd: X2,
                rn: X3,
                rm: W4,
                extend: RegExtend::Uxtw,
                amount: 2,
                sf: true
            }
        );
    }

    #[test]
    fn parse_subs_extended_reg_uxtb() {
        assert_eq!(
            parse_inst("subs w10, w8, w9, uxtb"),
            Inst::SubsExtReg {
                rd: W10,
                rn: W8,
                rm: W9,
                extend: RegExtend::Uxtb,
                amount: 0,
                sf: false
            }
        );
    }

    #[test]
    fn parse_subs_extended_reg_uxth() {
        assert_eq!(
            parse_inst("subs w11, w12, w13, uxth"),
            Inst::SubsExtReg {
                rd: W11,
                rn: W12,
                rm: W13,
                extend: RegExtend::Uxth,
                amount: 0,
                sf: false
            }
        );
    }

    #[test]
    fn parse_subs_extended_reg_sxtb() {
        assert_eq!(
            parse_inst("subs x14, x15, w16, sxtb"),
            Inst::SubsExtReg {
                rd: X14,
                rn: X15,
                rm: W16,
                extend: RegExtend::Sxtb,
                amount: 0,
                sf: true
            }
        );
    }

    #[test]
    fn parse_subs_extended_reg_sxth() {
        assert_eq!(
            parse_inst("subs x17, x18, w19, sxth #1"),
            Inst::SubsExtReg {
                rd: X17,
                rn: X18,
                rm: W19,
                extend: RegExtend::Sxth,
                amount: 1,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cmp_reg() {
        assert_eq!(
            parse_inst("cmp x0, x1"),
            Inst::SubsReg {
                rd: XZR,
                rn: X0,
                rm: X1,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cmp_shifted_reg() {
        assert_eq!(
            parse_inst("cmp x6, x7, lsr #4"),
            Inst::SubsShiftReg {
                rd: XZR,
                rn: X6,
                rm: X7,
                shift: RegShift::Lsr,
                amount: 4,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cmp_extended_reg() {
        assert_eq!(
            parse_inst("cmp x0, w1, sxtw"),
            Inst::SubsExtReg {
                rd: XZR,
                rn: X0,
                rm: W1,
                extend: RegExtend::Sxtw,
                amount: 0,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cmp_imm() {
        assert_eq!(
            parse_inst("cmp x5, #255"),
            Inst::SubsImm {
                rd: XZR,
                rn: X5,
                imm12: 255,
                shift: false,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cmn_shifted_reg() {
        assert_eq!(
            parse_inst("cmn x8, x9, lsl #1"),
            Inst::AddsShiftReg {
                rd: XZR,
                rn: X8,
                rm: X9,
                shift: RegShift::Lsl,
                amount: 1,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cmn_extended_reg() {
        assert_eq!(
            parse_inst("cmn x6, w7, sxtw #3"),
            Inst::AddsExtReg {
                rd: XZR,
                rn: X6,
                rm: W7,
                extend: RegExtend::Sxtw,
                amount: 3,
                sf: true
            }
        );
    }

    #[test]
    fn parse_tst_() {
        assert_eq!(
            parse_inst("tst x0, x1"),
            Inst::AndsReg {
                rd: XZR,
                rn: X0,
                rm: X1,
                sf: true
            }
        );
    }

    #[test]
    fn parse_tst_imm() {
        assert_eq!(
            parse_inst("tst w8, #0x7"),
            Inst::AndsImm {
                rd: XZR,
                rn: W8,
                imm: 0x7,
                sf: false
            }
        );
    }

    #[test]
    fn parse_mul_() {
        assert_eq!(
            parse_inst("mul x6, x7, x8"),
            Inst::Mul {
                rd: X6,
                rn: X7,
                rm: X8,
                sf: true
            }
        );
    }

    #[test]
    fn parse_madd_() {
        assert_eq!(
            parse_inst("madd w0, w0, w0, w8"),
            Inst::Madd {
                rd: W0,
                rn: W0,
                rm: W0,
                ra: W8,
                sf: false
            }
        );
    }

    #[test]
    fn parse_msub_() {
        assert_eq!(
            parse_inst("msub w9, w8, w1, w0"),
            Inst::Msub {
                rd: W9,
                rn: W8,
                rm: W1,
                ra: W0,
                sf: false
            }
        );
    }

    #[test]
    fn parse_umull_() {
        assert_eq!(
            parse_inst("umull x9, w8, w9"),
            Inst::Umull {
                rd: X9,
                rn: W8,
                rm: W9
            }
        );
    }

    #[test]
    fn parse_and_() {
        assert_eq!(
            parse_inst("and x3, x4, x5"),
            Inst::AndReg {
                rd: X3,
                rn: X4,
                rm: X5,
                sf: true
            }
        );
    }

    #[test]
    fn parse_and_imm() {
        assert_eq!(
            parse_inst("and w8, w8, #0x7"),
            Inst::AndImm {
                rd: W8,
                rn: W8,
                imm: 0x7,
                sf: false
            }
        );
    }

    // ---- Move ----

    #[test]
    fn parse_mov_imm() {
        assert_eq!(
            parse_inst("mov x0, #42"),
            Inst::Movz {
                rd: X0,
                imm16: 42,
                shift: 0,
                sf: true
            }
        );
    }

    #[test]
    fn parse_mov_reg() {
        assert_eq!(
            parse_inst("mov x0, x1"),
            Inst::OrrReg {
                rd: X0,
                rn: XZR,
                rm: X1,
                sf: true
            }
        );
    }

    #[test]
    fn parse_mov_wzr_keeps_zero_register() {
        assert_eq!(
            parse_inst("mov w26, wzr"),
            Inst::OrrReg {
                rd: W26,
                rn: WZR,
                rm: WZR,
                sf: false
            }
        );
    }

    #[test]
    fn parse_ubfiz_() {
        assert_eq!(
            parse_inst("ubfiz w8, w0, #5, #3"),
            Inst::Ubfiz {
                rd: W8,
                rn: W0,
                lsb: 5,
                width: 3,
                sf: false
            }
        );
    }

    #[test]
    fn parse_bfi_() {
        assert_eq!(
            parse_inst("bfi w0, w8, #5, #27"),
            Inst::Bfi {
                rd: W0,
                rn: W8,
                lsb: 5,
                width: 27,
                sf: false
            }
        );
    }

    #[test]
    fn parse_bfxil_() {
        assert_eq!(
            parse_inst("bfxil w8, w0, #3, #5"),
            Inst::Bfxil {
                rd: W8,
                rn: W0,
                lsb: 3,
                width: 5,
                sf: false
            }
        );
    }

    #[test]
    fn parse_neg_alias() {
        assert_eq!(
            parse_inst("neg x0, x1"),
            Inst::SubReg {
                rd: X0,
                rn: XZR,
                rm: X1,
                sf: true
            }
        );
    }

    #[test]
    fn parse_neg_shift_alias() {
        assert_eq!(
            parse_inst("neg x0, x1, lsl #2"),
            Inst::SubShiftReg {
                rd: X0,
                rn: XZR,
                rm: X1,
                shift: RegShift::Lsl,
                amount: 2,
                sf: true
            }
        );
    }

    #[test]
    fn error_neg_extend_alias_rejects_zero_base_register() {
        let err = parse_err("neg x8, w9, sxtw #2");
        assert!(
            err.contains("x-register or sp base operand"),
            "got: {}",
            err
        );
    }

    #[test]
    fn parse_mvn_alias() {
        assert_eq!(
            parse_inst("mvn x0, x1"),
            Inst::OrnReg {
                rd: X0,
                rn: XZR,
                rm: X1,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cset_alias() {
        assert_eq!(
            parse_inst("cset x0, eq"),
            Inst::Csinc {
                rd: X0,
                rn: XZR,
                rm: XZR,
                cond: Cond::NE,
                sf: true
            }
        );
    }

    #[test]
    fn parse_csel() {
        assert_eq!(
            parse_inst("csel w0, w0, w1, gt"),
            Inst::Csel {
                rd: W0,
                rn: W0,
                rm: W1,
                cond: Cond::GT,
                sf: false
            }
        );
    }

    #[test]
    fn parse_csinc() {
        assert_eq!(
            parse_inst("csinc x2, x3, x4, ne"),
            Inst::Csinc {
                rd: X2,
                rn: X3,
                rm: X4,
                cond: Cond::NE,
                sf: true
            }
        );
    }

    #[test]
    fn parse_csinv() {
        assert_eq!(
            parse_inst("csinv x2, x3, x4, ne"),
            Inst::Csinv {
                rd: X2,
                rn: X3,
                rm: X4,
                cond: Cond::NE,
                sf: true
            }
        );
    }

    #[test]
    fn parse_ccmp() {
        assert_eq!(
            parse_inst("ccmp w0, #3, #4, ne"),
            Inst::CcmpImm {
                rn: W0,
                imm5: 3,
                nzcv: 4,
                cond: Cond::NE,
                sf: false
            }
        );
    }

    #[test]
    fn parse_ccmn() {
        assert_eq!(
            parse_inst("ccmn x3, #9, #1, ge"),
            Inst::CcmnImm {
                rn: X3,
                imm5: 9,
                nzcv: 1,
                cond: Cond::GE,
                sf: true
            }
        );
    }

    #[test]
    fn parse_csneg() {
        assert_eq!(
            parse_inst("csneg x5, x6, x7, gt"),
            Inst::Csneg {
                rd: X5,
                rn: X6,
                rm: X7,
                cond: Cond::GT,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cinc_alias() {
        assert_eq!(
            parse_inst("cinc w2, w3, ne"),
            Inst::Csinc {
                rd: W2,
                rn: W3,
                rm: W3,
                cond: Cond::EQ,
                sf: false
            }
        );
    }

    #[test]
    fn parse_csetm_alias() {
        assert_eq!(
            parse_inst("csetm w8, eq"),
            Inst::Csinv {
                rd: W8,
                rn: XZR,
                rm: XZR,
                cond: Cond::NE,
                sf: false
            }
        );
    }

    #[test]
    fn parse_cinv_alias() {
        assert_eq!(
            parse_inst("cinv w9, w10, mi"),
            Inst::Csinv {
                rd: W9,
                rn: W10,
                rm: W10,
                cond: Cond::PL,
                sf: false
            }
        );
    }

    #[test]
    fn parse_cneg_alias() {
        assert_eq!(
            parse_inst("cneg x11, x12, lt"),
            Inst::Csneg {
                rd: X11,
                rn: X12,
                rm: X12,
                cond: Cond::GE,
                sf: true
            }
        );
    }

    #[test]
    fn parse_movz_shift() {
        assert_eq!(
            parse_inst("movz x0, #0x1234, lsl #16"),
            Inst::Movz {
                rd: X0,
                imm16: 0x1234,
                shift: 16,
                sf: true
            }
        );
    }

    // ---- Shifts ----

    #[test]
    fn parse_lsl_() {
        assert_eq!(
            parse_inst("lsl x0, x1, #3"),
            Inst::LslImm {
                rd: X0,
                rn: X1,
                amount: 3,
                sf: true
            }
        );
    }

    #[test]
    fn parse_immediate_shift_boundaries_for_all_mnemonics() {
        for mnemonic in ["lsl", "lsr", "asr"] {
            for (register, amount, sf) in [
                ("w", 0, false),
                ("w", 31, false),
                ("x", 0, true),
                ("x", 63, true),
            ] {
                let source = format!("{mnemonic} {register}0, {register}1, #{amount}");
                assert_eq!(
                    immediate_shift_fields(parse_inst(&source)),
                    (amount, sf),
                    "{source}"
                );
            }
        }
    }

    #[test]
    fn reject_immediate_shift_amounts_outside_register_width() {
        for mnemonic in ["lsl", "lsr", "asr"] {
            for (register, amount) in [
                ("w", -1),
                ("w", 32),
                ("w", 255),
                ("w", 256),
                ("x", -1),
                ("x", 64),
                ("x", 255),
                ("x", 256),
            ] {
                let source = format!("{mnemonic} {register}0, {register}1, #{amount}");
                let err = parse_err(&source);
                assert!(err.contains("out of range"), "{source}: {err}");
            }
        }
    }

    #[test]
    fn reject_sp_and_mismatched_widths_in_immediate_shifts() {
        for mnemonic in ["lsl", "lsr", "asr"] {
            for source in [
                format!("{mnemonic} sp, x1, #0"),
                format!("{mnemonic} x0, sp, #0"),
            ] {
                let err = parse_err(&source);
                assert!(err.contains("does not allow SP"), "{source}: {err}");
            }
            for source in [
                format!("{mnemonic} w0, x1, #0"),
                format!("{mnemonic} x0, w1, #0"),
            ] {
                let err = parse_err(&source);
                assert!(err.contains("same width"), "{source}: {err}");
            }
        }
    }

    #[test]
    fn parse_register_shifts_x() {
        assert_eq!(
            parse_inst("lsl x0, x1, x2"),
            Inst::LslReg {
                rd: X0,
                rn: X1,
                rm: X2,
                sf: true
            }
        );
        assert_eq!(
            parse_inst("lsr x3, x4, x5"),
            Inst::LsrReg {
                rd: X3,
                rn: X4,
                rm: X5,
                sf: true
            }
        );
        assert_eq!(
            parse_inst("asr x6, x7, x8"),
            Inst::AsrReg {
                rd: X6,
                rn: X7,
                rm: X8,
                sf: true
            }
        );
    }

    #[test]
    fn parse_register_shifts_w() {
        assert_eq!(
            parse_inst("lsl w0, w1, w2"),
            Inst::LslReg {
                rd: W0,
                rn: W1,
                rm: W2,
                sf: false
            }
        );
        assert_eq!(
            parse_inst("lsr w3, w4, w5"),
            Inst::LsrReg {
                rd: W3,
                rn: W4,
                rm: W5,
                sf: false
            }
        );
        assert_eq!(
            parse_inst("asr w6, w7, w8"),
            Inst::AsrReg {
                rd: W6,
                rn: W7,
                rm: W8,
                sf: false
            }
        );
    }

    #[test]
    fn reject_register_shift_width_mismatch() {
        for source in ["lsl w0, x1, w2", "lsr x0, x1, w2", "asr x0, w1, x2"] {
            let err = parse_err(source);
            assert!(err.contains("same width"), "{source}: {err}");
        }
    }

    #[test]
    fn reject_sp_in_register_shifts() {
        for source in ["lsl sp, x1, x2", "lsr x0, sp, x2", "asr x0, x1, sp"] {
            let err = parse_err(source);
            assert!(err.contains("does not allow SP"), "{source}: {err}");
        }
    }

    #[test]
    fn parse_integer_extend_aliases() {
        assert_eq!(parse_inst("sxtw x0, w1"), Inst::Sxtw { rd: X0, rn: W1 });
        assert_eq!(
            parse_inst("sxtb w2, w3"),
            Inst::Sxtb {
                rd: W2,
                rn: W3,
                sf: false
            }
        );
        assert_eq!(
            parse_inst("sxtb x4, w5"),
            Inst::Sxtb {
                rd: X4,
                rn: W5,
                sf: true
            }
        );
        assert_eq!(
            parse_inst("sxth w2, w3"),
            Inst::Sxth {
                rd: W2,
                rn: W3,
                sf: false
            }
        );
        assert_eq!(
            parse_inst("sxth x2, w3"),
            Inst::Sxth {
                rd: X2,
                rn: W3,
                sf: true
            }
        );
        assert_eq!(parse_inst("uxtw x6, w7"), Inst::Uxtw { rd: X6, rn: W7 });
    }

    #[test]
    fn reject_invalid_integer_extend_aliases() {
        for source in ["sxtw w0, w1", "uxtw w0, w1"] {
            let err = parse_err(source);
            assert!(err.contains("x-register destination"), "{source}: {err}");
        }
        for source in [
            "sxtw x0, x1",
            "sxtb w0, x1",
            "sxtb x0, x1",
            "sxth w0, x1",
            "uxtw x0, x1",
        ] {
            let err = parse_err(source);
            assert!(err.contains("w-register source"), "{source}: {err}");
        }
        for source in ["sxtw sp, w1", "sxtb x0, sp", "sxth w0, sp", "uxtw x0, sp"] {
            let err = parse_err(source);
            assert!(err.contains("does not allow SP"), "{source}: {err}");
        }
    }

    // ---- Branches ----

    #[test]
    fn parse_b_() {
        assert_eq!(parse_inst("b #20"), Inst::B { offset: 20 });
    }

    #[test]
    fn parse_bl_() {
        assert_eq!(parse_inst("bl #40"), Inst::Bl { offset: 40 });
    }

    #[test]
    fn parse_b_eq() {
        assert_eq!(
            parse_inst("b.eq #8"),
            Inst::BCond {
                cond: Cond::EQ,
                offset: 8
            }
        );
    }

    #[test]
    fn parse_b_ne() {
        assert_eq!(
            parse_inst("b.ne #12"),
            Inst::BCond {
                cond: Cond::NE,
                offset: 12
            }
        );
    }

    #[test]
    fn parse_b_ge() {
        assert_eq!(
            parse_inst("b.ge #16"),
            Inst::BCond {
                cond: Cond::GE,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_tbz_() {
        assert_eq!(
            parse_inst("tbz x0, #5, #8"),
            Inst::Tbz {
                rt: X0,
                bit: 5,
                offset: 8,
                sf: true
            }
        );
    }

    #[test]
    fn parse_tbnz_() {
        assert_eq!(
            parse_inst("tbnz w1, #31, #12"),
            Inst::Tbnz {
                rt: W1,
                bit: 31,
                offset: 12,
                sf: false
            }
        );
    }

    #[test]
    fn parse_tbz_label() {
        assert_eq!(
            parse_stmts("tbz x0, #5, target"),
            vec![Stmt::InstructionWithReloc(
                Inst::Tbz {
                    rt: X0,
                    bit: 5,
                    offset: 0,
                    sf: true
                },
                LabelRef {
                    symbol: "target".into(),
                    kind: RelocKind::Branch14,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_tbnz_numeric_local_label() {
        assert_eq!(
            parse_stmts("tbnz x0, #33, 1f\n1:\n"),
            vec![
                Stmt::InstructionWithReloc(
                    Inst::Tbnz {
                        rt: X0,
                        bit: 33,
                        offset: 0,
                        sf: true
                    },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::Branch14,
                        addend: 0
                    },
                ),
                Stmt::Label(".Ltmp$1$1".into()),
            ]
        );
    }

    #[test]
    fn parse_adr_label() {
        assert_eq!(
            parse_stmts("adr x0, target"),
            vec![Stmt::InstructionWithReloc(
                Inst::Adr { rd: X0, imm: 0 },
                LabelRef {
                    symbol: "target".into(),
                    kind: RelocKind::Adr21,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_adr_offset() {
        assert_eq!(parse_inst("adr x0, #8"), Inst::Adr { rd: X0, imm: 8 });
    }

    #[test]
    fn parse_adr_numeric_local_label() {
        assert_eq!(
            parse_stmts("adr x0, 1f\n1:\n"),
            vec![
                Stmt::InstructionWithReloc(
                    Inst::Adr { rd: X0, imm: 0 },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::Adr21,
                        addend: 0
                    },
                ),
                Stmt::Label(".Ltmp$1$1".into()),
            ]
        );
    }

    #[test]
    fn parse_cbz_() {
        assert_eq!(
            parse_inst("cbz x0, #8"),
            Inst::Cbz {
                rt: X0,
                offset: 8,
                sf: true
            }
        );
    }

    #[test]
    fn error_tbz_bit_index_out_of_range() {
        let err = parse_err("tbz w0, #32, #8");
        assert!(err.contains("range 0..=31"), "got: {}", err);
    }

    #[test]
    fn error_shift_amount_out_of_range_for_w_reg() {
        let err = parse_err("add w0, w1, w2, lsl #32");
        assert!(err.contains("range 0..=31"), "got: {}", err);
    }

    #[test]
    fn error_extend_shift_amount_out_of_range() {
        let err = parse_err("add x0, x1, w2, sxtw #5");
        assert!(err.contains("range 0..=4"), "got: {}", err);
    }

    #[test]
    fn error_uxtw_requires_w_register_operand() {
        let err = parse_err("add x0, x1, x2, uxtw");
        assert!(err.contains("w-register operand"), "got: {}", err);
    }

    #[test]
    fn error_extended_add_sub_rejects_zero_base_register() {
        let err = parse_err("sub x11, xzr, w12, sxtw #2");
        assert!(
            err.contains("x-register or sp base operand"),
            "got: {}",
            err
        );
    }

    #[test]
    fn parse_ret_default() {
        assert_eq!(parse_inst("ret"), Inst::Ret { rn: X30 });
    }

    #[test]
    fn parse_ret_reg() {
        assert_eq!(parse_inst("ret x16"), Inst::Ret { rn: X16 });
    }

    // ---- Load/store ----

    #[test]
    fn parse_ldr_base() {
        assert_eq!(
            parse_inst("ldr x0, [x1]"),
            Inst::LdrImm64 {
                rt: X0,
                rn: X1,
                offset: 0
            }
        );
    }

    #[test]
    fn parse_ldr_offset() {
        assert_eq!(
            parse_inst("ldr x0, [x1, #8]"),
            Inst::LdrImm64 {
                rt: X0,
                rn: X1,
                offset: 8
            }
        );
    }

    #[test]
    fn parse_ldr_register_offset() {
        assert_eq!(
            parse_inst("ldr x0, [x1, x2]"),
            Inst::LdrReg64 {
                rt: X0,
                rn: X1,
                rm: X2,
                extend: AddrExtend::Lsl,
                shift: false
            }
        );
    }

    #[test]
    fn parse_ldr_register_offset_with_extend() {
        assert_eq!(
            parse_inst("ldr x6, [x7, w8, uxtw #3]"),
            Inst::LdrReg64 {
                rt: X6,
                rn: X7,
                rm: W8,
                extend: AddrExtend::Uxtw,
                shift: true
            }
        );
    }

    #[test]
    fn parse_str_offset() {
        assert_eq!(
            parse_inst("str x2, [x3, #16]"),
            Inst::StrImm64 {
                rt: X2,
                rn: X3,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_str_register_offset() {
        assert_eq!(
            parse_inst("str w9, [x10, x11]"),
            Inst::StrReg32 {
                rt: W9,
                rn: X10,
                rm: X11,
                extend: AddrExtend::Lsl,
                shift: false
            }
        );
    }

    #[test]
    fn parse_ldr_w() {
        assert_eq!(
            parse_inst("ldr w4, [x5, #4]"),
            Inst::LdrImm32 {
                rt: W4,
                rn: X5,
                offset: 4
            }
        );
    }

    #[test]
    fn parse_ldr_negative_offset_aliases_to_ldur() {
        assert_eq!(
            parse_inst("ldr x9, [x29, #-8]"),
            Inst::Ldur64 {
                rt: X9,
                rn: X29,
                offset: -8
            }
        );
    }

    #[test]
    fn parse_str_negative_offset_aliases_to_stur() {
        assert_eq!(
            parse_inst("str w6, [x7, #-4]"),
            Inst::Stur32 {
                rt: W6,
                rn: X7,
                offset: -4
            }
        );
    }

    #[test]
    fn parse_ldur_() {
        assert_eq!(
            parse_inst("ldur x9, [x29, #-8]"),
            Inst::Ldur64 {
                rt: X9,
                rn: X29,
                offset: -8
            }
        );
    }

    #[test]
    fn parse_stur_() {
        assert_eq!(
            parse_inst("stur w6, [x7, #-4]"),
            Inst::Stur32 {
                rt: W6,
                rn: X7,
                offset: -4
            }
        );
    }

    #[test]
    fn parse_ldr_w_post_index() {
        assert_eq!(
            parse_inst("ldr w0, [x1], #4"),
            Inst::LdrPost32 {
                rt: W0,
                rn: X1,
                offset: 4
            }
        );
    }

    #[test]
    fn parse_str_w_pre_index() {
        assert_eq!(
            parse_inst("str w2, [x3, #-4]!"),
            Inst::StrPre32 {
                rt: W2,
                rn: X3,
                offset: -4
            }
        );
    }

    #[test]
    fn parse_ldr_d_base() {
        assert_eq!(
            parse_inst("ldr d0, [x1]"),
            Inst::LdrFpImm64 {
                rt: D0,
                rn: X1,
                offset: 0
            }
        );
    }

    #[test]
    fn parse_ldr_q_offset() {
        assert_eq!(
            parse_inst("ldr q0, [sp, #16]"),
            Inst::LdrFpImm128 {
                rt: FpReg::new(0),
                rn: SP,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_ldr_h_offset() {
        assert_eq!(
            parse_inst("ldr h2, [sp, #14]"),
            Inst::LdrFpImm16 {
                rt: FpReg::new(2),
                rn: SP,
                offset: 14
            }
        );
    }

    #[test]
    fn parse_ldr_b_offset() {
        assert_eq!(
            parse_inst("ldr b2, [sp, #15]"),
            Inst::LdrFpImm8 {
                rt: FpReg::new(2),
                rn: SP,
                offset: 15
            }
        );
    }

    #[test]
    fn parse_str_q_base() {
        assert_eq!(
            parse_inst("str q1, [x0]"),
            Inst::StrFpImm128 {
                rt: FpReg::new(1),
                rn: X0,
                offset: 0
            }
        );
    }

    #[test]
    fn parse_str_h_offset() {
        assert_eq!(
            parse_inst("str h2, [sp, #14]"),
            Inst::StrFpImm16 {
                rt: FpReg::new(2),
                rn: SP,
                offset: 14
            }
        );
    }

    #[test]
    fn parse_str_b_offset() {
        assert_eq!(
            parse_inst("str b2, [sp, #15]"),
            Inst::StrFpImm8 {
                rt: FpReg::new(2),
                rn: SP,
                offset: 15
            }
        );
    }

    #[test]
    fn parse_ldr_q_literal_offset() {
        assert_eq!(
            parse_inst("ldr q0, #16"),
            Inst::LdrFpLit128 {
                rt: FpReg::new(0),
                offset: 16
            }
        );
    }

    #[test]
    fn parse_ldr_q_register_offset() {
        assert_eq!(
            parse_inst("ldr q0, [x1, x2]"),
            Inst::LdrFpReg128 {
                rt: FpReg::new(0),
                rn: X1,
                rm: X2,
                extend: AddrExtend::Lsl,
                shift: false
            }
        );
    }

    #[test]
    fn parse_ldr_q_pageoff_memory_operand() {
        assert_eq!(
            parse_stmts("ldr q2, [x8, lCPI0_0@PAGEOFF]"),
            vec![Stmt::InstructionWithReloc(
                Inst::LdrFpImm128 {
                    rt: FpReg::new(2),
                    rn: X8,
                    offset: 0
                },
                LabelRef {
                    symbol: "lCPI0_0".into(),
                    kind: RelocKind::PageOff12,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_str_q_register_offset_with_extend() {
        assert_eq!(
            parse_inst("str q1, [x3, w4, uxtw #4]"),
            Inst::StrFpReg128 {
                rt: FpReg::new(1),
                rn: X3,
                rm: W4,
                extend: AddrExtend::Uxtw,
                shift: true
            }
        );
    }

    #[test]
    fn parse_ldr_q_post_index() {
        assert_eq!(
            parse_inst("ldr q0, [sp], #16"),
            Inst::LdrFpPost128 {
                rt: FpReg::new(0),
                rn: SP,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_str_q_pre_index() {
        assert_eq!(
            parse_inst("str q1, [sp, #-16]!"),
            Inst::StrFpPre128 {
                rt: FpReg::new(1),
                rn: SP,
                offset: -16
            }
        );
    }

    #[test]
    fn parse_str_d_offset() {
        assert_eq!(
            parse_inst("str d2, [x3, #16]"),
            Inst::StrFpImm64 {
                rt: D2,
                rn: X3,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_str_s_negative_offset_uses_unscaled() {
        assert_eq!(
            parse_inst("str s8, [x29, #-4]"),
            Inst::SturFp32 {
                rt: S8,
                rn: X29,
                offset: -4
            }
        );
    }

    #[test]
    fn parse_ldr_s_negative_offset_uses_unscaled() {
        assert_eq!(
            parse_inst("ldr s9, [x29, #-4]"),
            Inst::LdurFp32 {
                rt: S9,
                rn: X29,
                offset: -4
            }
        );
    }

    #[test]
    fn parse_ldr_s_register_offset() {
        assert_eq!(
            parse_inst("ldr s4, [x5, x6]"),
            Inst::LdrFpReg32 {
                rt: S4,
                rn: X5,
                rm: X6,
                extend: AddrExtend::Lsl,
                shift: false
            }
        );
    }

    #[test]
    fn parse_str_s_register_offset_with_extend() {
        assert_eq!(
            parse_inst("str s7, [x8, w9, uxtw #2]"),
            Inst::StrFpReg32 {
                rt: S7,
                rn: X8,
                rm: W9,
                extend: AddrExtend::Uxtw,
                shift: true
            }
        );
    }

    #[test]
    fn parse_ldr_d_post_index() {
        assert_eq!(
            parse_inst("ldr d0, [sp], #8"),
            Inst::LdrFpPost64 {
                rt: D0,
                rn: SP,
                offset: 8
            }
        );
    }

    #[test]
    fn parse_str_s_pre_index() {
        assert_eq!(
            parse_inst("str s3, [sp, #-8]!"),
            Inst::StrFpPre32 {
                rt: S3,
                rn: SP,
                offset: -8
            }
        );
    }

    #[test]
    fn parse_ldrb_() {
        assert_eq!(
            parse_inst("ldrb w0, [x1, #3]"),
            Inst::Ldrb {
                rt: W0,
                rn: X1,
                offset: 3
            }
        );
    }

    #[test]
    fn parse_ldrsb_32() {
        assert_eq!(
            parse_inst("ldrsb w0, [x1, #3]"),
            Inst::Ldrsb32 {
                rt: W0,
                rn: X1,
                offset: 3
            }
        );
    }

    #[test]
    fn parse_ldrsb_post_index_64() {
        assert_eq!(
            parse_inst("ldrsb x9, [x1], #1"),
            Inst::LdrsbPost64 {
                rt: X9,
                rn: X1,
                offset: 1
            }
        );
    }

    #[test]
    fn parse_ldrb_post_index() {
        assert_eq!(
            parse_inst("ldrb w9, [x1], #1"),
            Inst::LdrbPost {
                rt: W9,
                rn: X1,
                offset: 1
            }
        );
    }

    #[test]
    fn parse_ldrb_register_offset() {
        assert_eq!(
            parse_inst("ldrb w0, [x1, x2]"),
            Inst::LdrbReg {
                rt: W0,
                rn: X1,
                rm: X2,
                extend: AddrExtend::Lsl,
                shift: false
            }
        );
    }

    #[test]
    fn parse_strb_() {
        assert_eq!(
            parse_inst("strb w8, [x9]"),
            Inst::Strb {
                rt: W8,
                rn: X9,
                offset: 0
            }
        );
    }

    #[test]
    fn parse_strb_post_index() {
        assert_eq!(
            parse_inst("strb w9, [x8], #1"),
            Inst::StrbPost {
                rt: W9,
                rn: X8,
                offset: 1
            }
        );
    }

    #[test]
    fn parse_ldrh_register_offset() {
        assert_eq!(
            parse_inst("ldrh w3, [x4, w5, uxtw #1]"),
            Inst::LdrhReg {
                rt: W3,
                rn: X4,
                rm: W5,
                extend: AddrExtend::Uxtw,
                shift: true
            }
        );
    }

    #[test]
    fn parse_ldrsh_register_offset_32() {
        assert_eq!(
            parse_inst("ldrsh w3, [x4, w5, uxtw #1]"),
            Inst::LdrshReg32 {
                rt: W3,
                rn: X4,
                rm: W5,
                extend: AddrExtend::Uxtw,
                shift: true
            }
        );
    }

    #[test]
    fn parse_strh_pre_index() {
        assert_eq!(
            parse_inst("strh w5, [x6, #2]!"),
            Inst::StrhPre {
                rt: W5,
                rn: X6,
                offset: 2
            }
        );
    }

    #[test]
    fn parse_ldrsw_register_offset() {
        assert_eq!(
            parse_inst("ldrsw x6, [x7, w8, sxtw #2]"),
            Inst::LdrswReg {
                rt: X6,
                rn: X7,
                rm: W8,
                extend: AddrExtend::Sxtw,
                shift: true
            }
        );
    }

    #[test]
    fn parse_ldrsh_pre_index_64() {
        assert_eq!(
            parse_inst("ldrsh x5, [x6, #2]!"),
            Inst::LdrshPre64 {
                rt: X5,
                rn: X6,
                offset: 2
            }
        );
    }

    #[test]
    fn parse_ldapr_w() {
        assert_eq!(
            parse_inst("ldapr w8, [x9]"),
            Inst::Ldapr32 { rt: W8, rn: X9 }
        );
    }

    #[test]
    fn parse_ldaprb_w() {
        assert_eq!(
            parse_inst("ldaprb w0, [x1]"),
            Inst::Ldaprb { rt: W0, rn: X1 }
        );
    }

    #[test]
    fn parse_ldaprh_w() {
        assert_eq!(
            parse_inst("ldaprh w2, [x3]"),
            Inst::Ldaprh { rt: W2, rn: X3 }
        );
    }

    #[test]
    fn parse_stlr_x() {
        assert_eq!(
            parse_inst("stlr x10, [x11]"),
            Inst::Stlr64 { rt: X10, rn: X11 }
        );
    }

    #[test]
    fn parse_stlrb_w() {
        assert_eq!(parse_inst("stlrb w4, [x5]"), Inst::Stlrb { rt: W4, rn: X5 });
    }

    #[test]
    fn parse_stlrh_w() {
        assert_eq!(parse_inst("stlrh w6, [x7]"), Inst::Stlrh { rt: W6, rn: X7 });
    }

    #[test]
    fn parse_ldaddal_w() {
        assert_eq!(
            parse_inst("ldaddal w0, w8, [x8]"),
            Inst::Ldaddal32 {
                rs: W0,
                rt: W8,
                rn: X8
            }
        );
    }

    #[test]
    fn parse_ldaddalb_w() {
        assert_eq!(
            parse_inst("ldaddalb w0, w1, [x2]"),
            Inst::Ldaddalb {
                rs: W0,
                rt: W1,
                rn: X2
            }
        );
    }

    #[test]
    fn parse_ldaddalh_w() {
        assert_eq!(
            parse_inst("ldaddalh w3, w4, [x5]"),
            Inst::Ldaddalh {
                rs: W3,
                rt: W4,
                rn: X5
            }
        );
    }

    #[test]
    fn parse_ldumaxalb_w() {
        assert_eq!(
            parse_inst("ldumaxalb w0, w1, [x2]"),
            Inst::Ldumaxalb {
                rs: W0,
                rt: W1,
                rn: X2
            }
        );
    }

    #[test]
    fn parse_ldumaxalh_w() {
        assert_eq!(
            parse_inst("ldumaxalh w3, w4, [x5]"),
            Inst::Ldumaxalh {
                rs: W3,
                rt: W4,
                rn: X5
            }
        );
    }

    #[test]
    fn parse_ldsmaxalb_w() {
        assert_eq!(
            parse_inst("ldsmaxalb w18, w19, [x20]"),
            Inst::Ldsmaxalb {
                rs: W18,
                rt: W19,
                rn: X20
            }
        );
    }

    #[test]
    fn parse_ldsmaxalh_w() {
        assert_eq!(
            parse_inst("ldsmaxalh w24, w25, [x26]"),
            Inst::Ldsmaxalh {
                rs: W24,
                rt: W25,
                rn: X26
            }
        );
    }

    #[test]
    fn parse_ldumaxal_w() {
        assert_eq!(
            parse_inst("ldumaxal w6, w7, [x8]"),
            Inst::Ldumaxal32 {
                rs: W6,
                rt: W7,
                rn: X8
            }
        );
    }

    #[test]
    fn parse_ldsmaxal_w() {
        assert_eq!(
            parse_inst("ldsmaxal w0, w1, [x2]"),
            Inst::Ldsmaxal32 {
                rs: W0,
                rt: W1,
                rn: X2
            }
        );
    }

    #[test]
    fn parse_ldsminal_x() {
        assert_eq!(
            parse_inst("ldsminal x9, x10, [x11]"),
            Inst::Ldsminal64 {
                rs: X9,
                rt: X10,
                rn: X11
            }
        );
    }

    #[test]
    fn parse_lduminalb_w() {
        assert_eq!(
            parse_inst("lduminalb w12, w13, [x14]"),
            Inst::Lduminalb {
                rs: W12,
                rt: W13,
                rn: X14
            }
        );
    }

    #[test]
    fn parse_lduminalh_w() {
        assert_eq!(
            parse_inst("lduminalh w15, w16, [x17]"),
            Inst::Lduminalh {
                rs: W15,
                rt: W16,
                rn: X17
            }
        );
    }

    #[test]
    fn parse_ldsminalb_w() {
        assert_eq!(
            parse_inst("ldsminalb w21, w22, [x23]"),
            Inst::Ldsminalb {
                rs: W21,
                rt: W22,
                rn: X23
            }
        );
    }

    #[test]
    fn parse_ldsminalh_w() {
        assert_eq!(
            parse_inst("ldsminalh w27, w28, [x29]"),
            Inst::Ldsminalh {
                rs: W27,
                rt: W28,
                rn: X29
            }
        );
    }

    #[test]
    fn parse_lduminal_w() {
        assert_eq!(
            parse_inst("lduminal w18, w19, [x20]"),
            Inst::Lduminal32 {
                rs: W18,
                rt: W19,
                rn: X20
            }
        );
    }

    #[test]
    fn parse_ldclral_w() {
        assert_eq!(
            parse_inst("ldclral w12, w13, [x14]"),
            Inst::Ldclral32 {
                rs: W12,
                rt: W13,
                rn: X14
            }
        );
    }

    #[test]
    fn parse_ldclralb_w() {
        assert_eq!(
            parse_inst("ldclralb w6, w7, [x8]"),
            Inst::Ldclralb {
                rs: W6,
                rt: W7,
                rn: X8
            }
        );
    }

    #[test]
    fn parse_ldclralh_w() {
        assert_eq!(
            parse_inst("ldclralh w15, w16, [x17]"),
            Inst::Ldclralh {
                rs: W15,
                rt: W16,
                rn: X17
            }
        );
    }

    #[test]
    fn parse_ldeoral_x() {
        assert_eq!(
            parse_inst("ldeoral x9, x10, [x11]"),
            Inst::Ldeoral64 {
                rs: X9,
                rt: X10,
                rn: X11
            }
        );
    }

    #[test]
    fn parse_ldeoralb_w() {
        assert_eq!(
            parse_inst("ldeoralb w3, w4, [x5]"),
            Inst::Ldeoralb {
                rs: W3,
                rt: W4,
                rn: X5
            }
        );
    }

    #[test]
    fn parse_ldeoralh_w() {
        assert_eq!(
            parse_inst("ldeoralh w12, w13, [x14]"),
            Inst::Ldeoralh {
                rs: W12,
                rt: W13,
                rn: X14
            }
        );
    }

    #[test]
    fn parse_ldsetal_w() {
        assert_eq!(
            parse_inst("ldsetal w0, w1, [x2]"),
            Inst::Ldsetal32 {
                rs: W0,
                rt: W1,
                rn: X2
            }
        );
    }

    #[test]
    fn parse_ldsetalb_w() {
        assert_eq!(
            parse_inst("ldsetalb w0, w1, [x2]"),
            Inst::Ldsetalb {
                rs: W0,
                rt: W1,
                rn: X2
            }
        );
    }

    #[test]
    fn parse_ldsetalh_w() {
        assert_eq!(
            parse_inst("ldsetalh w9, w10, [x11]"),
            Inst::Ldsetalh {
                rs: W9,
                rt: W10,
                rn: X11
            }
        );
    }

    #[test]
    fn parse_swpal_x() {
        assert_eq!(
            parse_inst("swpal x1, x2, [x3]"),
            Inst::Swpal64 {
                rs: X1,
                rt: X2,
                rn: X3
            }
        );
    }

    #[test]
    fn parse_swpalb_w() {
        assert_eq!(
            parse_inst("swpalb w8, w9, [x10]"),
            Inst::Swpalb {
                rs: W8,
                rt: W9,
                rn: X10
            }
        );
    }

    #[test]
    fn parse_swpalh_w() {
        assert_eq!(
            parse_inst("swpalh w11, w12, [x13]"),
            Inst::Swpalh {
                rs: W11,
                rt: W12,
                rn: X13
            }
        );
    }

    #[test]
    fn parse_casal_w() {
        assert_eq!(
            parse_inst("casal w4, w5, [x6]"),
            Inst::Casal32 {
                rs: W4,
                rt: W5,
                rn: X6
            }
        );
    }

    #[test]
    fn parse_casalb_w() {
        assert_eq!(
            parse_inst("casalb w6, w7, [x8]"),
            Inst::Casalb {
                rs: W6,
                rt: W7,
                rn: X8
            }
        );
    }

    #[test]
    fn parse_casalh_w() {
        assert_eq!(
            parse_inst("casalh w9, w10, [x11]"),
            Inst::Casalh {
                rs: W9,
                rt: W10,
                rn: X11
            }
        );
    }

    #[test]
    fn parse_ldr_literal_label() {
        assert_eq!(
            parse_stmts("ldr x0, target"),
            vec![Stmt::InstructionWithReloc(
                Inst::LdrLit64 { rt: X0, offset: 0 },
                LabelRef {
                    symbol: "target".into(),
                    kind: RelocKind::Literal19,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_ldr_literal_offset() {
        assert_eq!(
            parse_inst("ldr x0, #8"),
            Inst::LdrLit64 { rt: X0, offset: 8 }
        );
    }

    #[test]
    fn parse_ldr_d_literal_label() {
        assert_eq!(
            parse_stmts("ldr d10, target"),
            vec![Stmt::InstructionWithReloc(
                Inst::LdrFpLit64 { rt: D10, offset: 0 },
                LabelRef {
                    symbol: "target".into(),
                    kind: RelocKind::Literal19,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_ldr_s_literal_offset() {
        assert_eq!(
            parse_inst("ldr s11, #20"),
            Inst::LdrFpLit32 {
                rt: S11,
                offset: 20
            }
        );
    }

    #[test]
    fn parse_ldr_literal_numeric_local_label() {
        assert_eq!(
            parse_stmts("ldr x0, 1f\n1:\n"),
            vec![
                Stmt::InstructionWithReloc(
                    Inst::LdrLit64 { rt: X0, offset: 0 },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::Literal19,
                        addend: 0
                    },
                ),
                Stmt::Label(".Ltmp$1$1".into()),
            ]
        );
    }

    #[test]
    fn parse_ldr_s_literal_numeric_local_label() {
        assert_eq!(
            parse_stmts("ldr s0, 1f\n1:\n"),
            vec![
                Stmt::InstructionWithReloc(
                    Inst::LdrFpLit32 { rt: S0, offset: 0 },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::Literal19,
                        addend: 0
                    },
                ),
                Stmt::Label(".Ltmp$1$1".into()),
            ]
        );
    }

    #[test]
    fn parse_ldrsw_literal_label() {
        assert_eq!(
            parse_stmts("ldrsw x0, target"),
            vec![Stmt::InstructionWithReloc(
                Inst::LdrswLit { rt: X0, offset: 0 },
                LabelRef {
                    symbol: "target".into(),
                    kind: RelocKind::Literal19,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_ldrsw_literal_offset() {
        assert_eq!(
            parse_inst("ldrsw x1, #12"),
            Inst::LdrswLit { rt: X1, offset: 12 }
        );
    }

    #[test]
    fn parse_ldrsw_literal_numeric_local_label() {
        assert_eq!(
            parse_stmts("ldrsw x0, 1f\n1:\n"),
            vec![
                Stmt::InstructionWithReloc(
                    Inst::LdrswLit { rt: X0, offset: 0 },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::Literal19,
                        addend: 0
                    },
                ),
                Stmt::Label(".Ltmp$1$1".into()),
            ]
        );
    }

    #[test]
    fn parse_stp_pre() {
        assert_eq!(
            parse_inst("stp x29, x30, [sp, #-16]!"),
            Inst::StpPre64 {
                rt1: X29,
                rt2: X30,
                rn: SP,
                offset: -16
            }
        );
    }

    #[test]
    fn parse_ldp_d_pre() {
        assert_eq!(
            parse_inst("ldp d8, d9, [sp, #-16]!"),
            Inst::LdpFpPre64 {
                rt1: D8,
                rt2: D9,
                rn: SP,
                offset: -16
            }
        );
    }

    #[test]
    fn parse_stp_d_post() {
        assert_eq!(
            parse_inst("stp d10, d11, [sp], #16"),
            Inst::StpFpPost64 {
                rt1: D10,
                rt2: D11,
                rn: SP,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_ldp_d_offset() {
        assert_eq!(
            parse_inst("ldp d12, d13, [sp, #32]"),
            Inst::LdpFpOff64 {
                rt1: D12,
                rt2: D13,
                rn: SP,
                offset: 32
            }
        );
    }

    #[test]
    fn parse_stp_s_post() {
        assert_eq!(
            parse_inst("stp s0, s1, [sp], #8"),
            Inst::StpFpPost32 {
                rt1: S0,
                rt2: S1,
                rn: SP,
                offset: 8
            }
        );
    }

    #[test]
    fn parse_ldp_s_pre() {
        assert_eq!(
            parse_inst("ldp s2, s3, [sp, #-8]!"),
            Inst::LdpFpPre32 {
                rt1: S2,
                rt2: S3,
                rn: SP,
                offset: -8
            }
        );
    }

    #[test]
    fn parse_stp_q_pre() {
        assert_eq!(
            parse_inst("stp q0, q1, [sp, #-32]!"),
            Inst::StpFpPre128 {
                rt1: FpReg::new(0),
                rt2: FpReg::new(1),
                rn: SP,
                offset: -32
            }
        );
    }

    #[test]
    fn parse_ldp_q_post() {
        assert_eq!(
            parse_inst("ldp q2, q3, [sp], #32"),
            Inst::LdpFpPost128 {
                rt1: FpReg::new(2),
                rt2: FpReg::new(3),
                rn: SP,
                offset: 32
            }
        );
    }

    #[test]
    fn parse_ldp_q_offset() {
        assert_eq!(
            parse_inst("ldp q4, q5, [sp, #64]"),
            Inst::LdpFpOff128 {
                rt1: FpReg::new(4),
                rt2: FpReg::new(5),
                rn: SP,
                offset: 64
            }
        );
    }

    #[test]
    fn error_ldp_fp_pair_requires_matching_widths() {
        let err = parse_err("ldp d0, s1, [sp]");
        assert!(err.contains("matching register widths"), "got: {}", err);
    }

    #[test]
    fn parse_ldp_post() {
        assert_eq!(
            parse_inst("ldp x29, x30, [sp], #16"),
            Inst::LdpPost64 {
                rt1: X29,
                rt2: X30,
                rn: SP,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_stp_post() {
        assert_eq!(
            parse_inst("stp x29, x30, [sp], #16"),
            Inst::StpPost64 {
                rt1: X29,
                rt2: X30,
                rn: SP,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_ldp_pre() {
        assert_eq!(
            parse_inst("ldp x29, x30, [sp, #-16]!"),
            Inst::LdpPre64 {
                rt1: X29,
                rt2: X30,
                rn: SP,
                offset: -16
            }
        );
    }

    #[test]
    fn parse_ldp_off() {
        assert_eq!(
            parse_inst("ldp x19, x20, [sp, #16]"),
            Inst::LdpOff64 {
                rt1: X19,
                rt2: X20,
                rn: SP,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_ldp_w_off() {
        assert_eq!(
            parse_inst("ldp w9, w8, [x8]"),
            Inst::LdpOff32 {
                rt1: W9,
                rt2: W8,
                rn: X8,
                offset: 0
            }
        );
    }

    #[test]
    fn parse_stp_w_off() {
        assert_eq!(
            parse_inst("stp w1, w2, [sp, #16]"),
            Inst::StpOff32 {
                rt1: W1,
                rt2: W2,
                rn: SP,
                offset: 16
            }
        );
    }

    #[test]
    fn parse_ldp_w_post() {
        assert_eq!(
            parse_inst("ldp w9, w8, [sp], #8"),
            Inst::LdpPost32 {
                rt1: W9,
                rt2: W8,
                rn: SP,
                offset: 8
            }
        );
    }

    #[test]
    fn parse_ldp_w_pre() {
        assert_eq!(
            parse_inst("ldp w9, w8, [sp, #-8]!"),
            Inst::LdpPre32 {
                rt1: W9,
                rt2: W8,
                rn: SP,
                offset: -8
            }
        );
    }

    #[test]
    fn error_ldp_gp_pair_requires_matching_widths() {
        let err = parse_err("ldp x0, w1, [sp]");
        assert!(err.contains("matching register widths"), "got: {}", err);
    }

    // ---- FP ----

    #[test]
    fn parse_fadd_d() {
        assert_eq!(
            parse_inst("fadd d0, d1, d2"),
            Inst::FaddD {
                rd: D0,
                rn: D1,
                rm: D2
            }
        );
    }

    #[test]
    fn parse_fadd_s() {
        assert_eq!(
            parse_inst("fadd s0, s1, s2"),
            Inst::FaddS {
                rd: S0,
                rn: S1,
                rm: S2
            }
        );
    }

    #[test]
    fn parse_fadd_4s() {
        assert_eq!(
            parse_inst("fadd.4s v0, v1, v2"),
            Inst::FaddV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fadd_2d() {
        assert_eq!(
            parse_inst("fadd.2d v0, v1, v2"),
            Inst::FaddV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fmla_2d() {
        assert_eq!(
            parse_inst("fmla.2d v0, v1, v2"),
            Inst::FmlaV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fmls_2d() {
        assert_eq!(
            parse_inst("fmls.2d v3, v4, v5"),
            Inst::FmlsV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_faddp_4s() {
        assert_eq!(
            parse_inst("faddp.4s v0, v1, v2"),
            Inst::FaddpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_faddp_2d() {
        assert_eq!(
            parse_inst("faddp.2d v0, v1, v2"),
            Inst::FaddpV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fmaxp_4s() {
        assert_eq!(
            parse_inst("fmaxp.4s v0, v1, v2"),
            Inst::FmaxpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fmaxp_2d() {
        assert_eq!(
            parse_inst("fmaxp.2d v3, v4, v5"),
            Inst::FmaxpV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fmaxp_2d_scalar() {
        assert_eq!(
            parse_inst("fmaxp.2d d1, v2"),
            Inst::FmaxpV2DScalar {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fminp_4s() {
        assert_eq!(
            parse_inst("fminp.4s v3, v4, v5"),
            Inst::FminpV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fmaxnmp_4s() {
        assert_eq!(
            parse_inst("fmaxnmp.4s v0, v1, v2"),
            Inst::FmaxnmpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fmaxnmp_2d() {
        assert_eq!(
            parse_inst("fmaxnmp.2d v0, v1, v2"),
            Inst::FmaxnmpV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fmaxnmp_2d_scalar() {
        assert_eq!(
            parse_inst("fmaxnmp.2d d0, v0"),
            Inst::FmaxnmpV2DScalar {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_fminnmp_4s() {
        assert_eq!(
            parse_inst("fminnmp.4s v3, v4, v5"),
            Inst::FminnmpV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fminnmp_2d() {
        assert_eq!(
            parse_inst("fminnmp.2d v3, v4, v5"),
            Inst::FminnmpV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fminnmp_2d_scalar() {
        assert_eq!(
            parse_inst("fminnmp.2d d1, v2"),
            Inst::FminnmpV2DScalar {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fabs_2d() {
        assert_eq!(
            parse_inst("fabs.2d v0, v0"),
            Inst::FabsV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_fneg_2d() {
        assert_eq!(
            parse_inst("fneg.2d v3, v4"),
            Inst::FnegV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_fsqrt_2d() {
        assert_eq!(
            parse_inst("fsqrt.2d v1, v2"),
            Inst::FsqrtV2D {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_scvtf_2d() {
        assert_eq!(
            parse_inst("scvtf.2d v0, v0"),
            Inst::ScvtfV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_ucvtf_2d() {
        assert_eq!(
            parse_inst("ucvtf.2d v1, v2"),
            Inst::UcvtfV2D {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fcvtzs_2d() {
        assert_eq!(
            parse_inst("fcvtzs.2d v3, v4"),
            Inst::FcvtzsV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_fcvtzu_2d() {
        assert_eq!(
            parse_inst("fcvtzu.2d v5, v6"),
            Inst::FcvtzuV2D {
                rd: FpReg::new(5),
                rn: FpReg::new(6)
            }
        );
    }

    #[test]
    fn parse_frecpe_2d() {
        assert_eq!(
            parse_inst("frecpe.2d v0, v0"),
            Inst::FrecpeV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_frecps_2d() {
        assert_eq!(
            parse_inst("frecps.2d v1, v2, v3"),
            Inst::FrecpsV2D {
                rd: FpReg::new(1),
                rn: FpReg::new(2),
                rm: FpReg::new(3)
            }
        );
    }

    #[test]
    fn parse_frsqrte_2d() {
        assert_eq!(
            parse_inst("frsqrte.2d v4, v5"),
            Inst::FrsqrteV2D {
                rd: FpReg::new(4),
                rn: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_frsqrts_2d() {
        assert_eq!(
            parse_inst("frsqrts.2d v6, v7, v8"),
            Inst::FrsqrtsV2D {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_frintn_2d() {
        assert_eq!(
            parse_inst("frintn.2d v0, v0"),
            Inst::FrintnV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_frintm_2d() {
        assert_eq!(
            parse_inst("frintm.2d v1, v2"),
            Inst::FrintmV2D {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_frintp_2d() {
        assert_eq!(
            parse_inst("frintp.2d v3, v4"),
            Inst::FrintpV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_frintz_2d() {
        assert_eq!(
            parse_inst("frintz.2d v5, v6"),
            Inst::FrintzV2D {
                rd: FpReg::new(5),
                rn: FpReg::new(6)
            }
        );
    }

    #[test]
    fn parse_frinta_2d() {
        assert_eq!(
            parse_inst("frinta.2d v7, v8"),
            Inst::FrintaV2D {
                rd: FpReg::new(7),
                rn: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_frinti_2d() {
        assert_eq!(
            parse_inst("frinti.2d v9, v10"),
            Inst::FrintiV2D {
                rd: FpReg::new(9),
                rn: FpReg::new(10)
            }
        );
    }

    #[test]
    fn parse_add_4s() {
        assert_eq!(
            parse_inst("add.4s v0, v1, v2"),
            Inst::AddV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fminp_2d() {
        assert_eq!(
            parse_inst("fminp.2d v6, v7, v8"),
            Inst::FminpV2D {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_fminp_2d_scalar() {
        assert_eq!(
            parse_inst("fminp.2d d3, v4"),
            Inst::FminpV2DScalar {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_addp_4s() {
        assert_eq!(
            parse_inst("addp.4s v0, v1, v2"),
            Inst::AddpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_addp_2d() {
        assert_eq!(
            parse_inst("addp.2d v0, v1, v2"),
            Inst::AddpV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_addp_8h() {
        assert_eq!(
            parse_inst("addp.8h v0, v1, v2"),
            Inst::AddpV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_addp_16b() {
        assert_eq!(
            parse_inst("addp.16b v6, v7, v8"),
            Inst::AddpV16B {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_fmax_2d() {
        assert_eq!(
            parse_inst("fmax.2d v0, v0, v1"),
            Inst::FmaxV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
        );
    }

    #[test]
    fn parse_fmax_4s() {
        assert_eq!(
            parse_inst("fmax.4s v0, v0, v1"),
            Inst::FmaxV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
        );
    }

    #[test]
    fn parse_fmaxnm_4s() {
        assert_eq!(
            parse_inst("fmaxnm.4s v0, v1, v2"),
            Inst::FmaxnmV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fmin_2d() {
        assert_eq!(
            parse_inst("fmin.2d v2, v3, v4"),
            Inst::FminV2D {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_fmaxnm_2d() {
        assert_eq!(
            parse_inst("fmaxnm.2d v0, v0, v1"),
            Inst::FmaxnmV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
        );
    }

    #[test]
    fn parse_fmin_4s() {
        assert_eq!(
            parse_inst("fmin.4s v2, v3, v4"),
            Inst::FminV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_fminnm_4s() {
        assert_eq!(
            parse_inst("fminnm.4s v3, v4, v5"),
            Inst::FminnmV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fminnm_2d() {
        assert_eq!(
            parse_inst("fminnm.2d v2, v3, v4"),
            Inst::FminnmV2D {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_smax_4s() {
        assert_eq!(
            parse_inst("smax.4s v5, v6, v7"),
            Inst::SmaxV4S {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                rm: FpReg::new(7)
            }
        );
    }

    #[test]
    fn parse_smaxp_4s() {
        assert_eq!(
            parse_inst("smaxp.4s v6, v7, v8"),
            Inst::SmaxpV4S {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_smaxp_8h() {
        assert_eq!(
            parse_inst("smaxp.8h v6, v7, v8"),
            Inst::SmaxpV8H {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_smaxp_16b() {
        assert_eq!(
            parse_inst("smaxp.16b v6, v7, v8"),
            Inst::SmaxpV16B {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_smin_4s() {
        assert_eq!(
            parse_inst("smin.4s v8, v9, v10"),
            Inst::SminV4S {
                rd: FpReg::new(8),
                rn: FpReg::new(9),
                rm: FpReg::new(10)
            }
        );
    }

    #[test]
    fn parse_sminp_4s() {
        assert_eq!(
            parse_inst("sminp.4s v9, v10, v11"),
            Inst::SminpV4S {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
        );
    }

    #[test]
    fn parse_sminp_8h() {
        assert_eq!(
            parse_inst("sminp.8h v9, v10, v11"),
            Inst::SminpV8H {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
        );
    }

    #[test]
    fn parse_sminp_16b() {
        assert_eq!(
            parse_inst("sminp.16b v9, v10, v11"),
            Inst::SminpV16B {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
        );
    }

    #[test]
    fn parse_umax_4s() {
        assert_eq!(
            parse_inst("umax.4s v0, v0, v1"),
            Inst::UmaxV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
        );
    }

    #[test]
    fn parse_umaxp_4s() {
        assert_eq!(
            parse_inst("umaxp.4s v0, v1, v2"),
            Inst::UmaxpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_umaxp_8h() {
        assert_eq!(
            parse_inst("umaxp.8h v0, v1, v2"),
            Inst::UmaxpV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_umaxp_16b() {
        assert_eq!(
            parse_inst("umaxp.16b v0, v1, v2"),
            Inst::UmaxpV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_umin_4s() {
        assert_eq!(
            parse_inst("umin.4s v2, v3, v4"),
            Inst::UminV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_uminp_4s() {
        assert_eq!(
            parse_inst("uminp.4s v3, v4, v5"),
            Inst::UminpV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_uminp_8h() {
        assert_eq!(
            parse_inst("uminp.8h v3, v4, v5"),
            Inst::UminpV8H {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_uminp_16b() {
        assert_eq!(
            parse_inst("uminp.16b v3, v4, v5"),
            Inst::UminpV16B {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_addv_4s() {
        assert_eq!(
            parse_inst("addv.4s s0, v0"),
            Inst::AddvV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_addv_16b() {
        assert_eq!(
            parse_inst("addv.16b b0, v0"),
            Inst::AddvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_addv_8h() {
        assert_eq!(
            parse_inst("addv.8h h0, v0"),
            Inst::AddvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_faddp_2s() {
        assert_eq!(
            parse_inst("faddp.2s s3, v4"),
            Inst::FaddpV2S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_faddp_2d_scalar() {
        assert_eq!(
            parse_inst("faddp.2d d0, v0"),
            Inst::FaddpV2DScalar {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_fmaxv_4s() {
        assert_eq!(
            parse_inst("fmaxv.4s s1, v2"),
            Inst::FmaxvV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fmaxnmv_4s() {
        assert_eq!(
            parse_inst("fmaxnmv.4s s1, v2"),
            Inst::FmaxnmvV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fminv_4s() {
        assert_eq!(
            parse_inst("fminv.4s s3, v4"),
            Inst::FminvV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_fminnmv_4s() {
        assert_eq!(
            parse_inst("fminnmv.4s s3, v4"),
            Inst::FminnmvV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_umaxv_4s() {
        assert_eq!(
            parse_inst("umaxv.4s s1, v2"),
            Inst::UmaxvV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_umaxv_16b() {
        assert_eq!(
            parse_inst("umaxv.16b b0, v0"),
            Inst::UmaxvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_umaxv_8h() {
        assert_eq!(
            parse_inst("umaxv.8h h0, v0"),
            Inst::UmaxvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_smaxv_4s() {
        assert_eq!(
            parse_inst("smaxv.4s s3, v4"),
            Inst::SmaxvV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_smaxv_16b() {
        assert_eq!(
            parse_inst("smaxv.16b b0, v0"),
            Inst::SmaxvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_smaxv_8h() {
        assert_eq!(
            parse_inst("smaxv.8h h0, v0"),
            Inst::SmaxvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_uminv_4s() {
        assert_eq!(
            parse_inst("uminv.4s s1, v2"),
            Inst::UminvV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_uminv_16b() {
        assert_eq!(
            parse_inst("uminv.16b b0, v0"),
            Inst::UminvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_uminv_8h() {
        assert_eq!(
            parse_inst("uminv.8h h0, v0"),
            Inst::UminvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_sminv_4s() {
        assert_eq!(
            parse_inst("sminv.4s s3, v4"),
            Inst::SminvV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_sminv_16b() {
        assert_eq!(
            parse_inst("sminv.16b b0, v0"),
            Inst::SminvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_sminv_8h() {
        assert_eq!(
            parse_inst("sminv.8h h0, v0"),
            Inst::SminvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_fsub_4s() {
        assert_eq!(
            parse_inst("fsub.4s v3, v4, v5"),
            Inst::FsubV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fsub_2d() {
        assert_eq!(
            parse_inst("fsub.2d v3, v4, v5"),
            Inst::FsubV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_sub_4s() {
        assert_eq!(
            parse_inst("sub.4s v3, v4, v5"),
            Inst::SubV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fmul_4s() {
        assert_eq!(
            parse_inst("fmul.4s v6, v7, v8"),
            Inst::FmulV4S {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_fmul_2d() {
        assert_eq!(
            parse_inst("fmul.2d v6, v7, v8"),
            Inst::FmulV2D {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_fdiv_4s() {
        assert_eq!(
            parse_inst("fdiv.4s v9, v10, v11"),
            Inst::FdivV4S {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
        );
    }

    #[test]
    fn parse_fdiv_2d() {
        assert_eq!(
            parse_inst("fdiv.2d v9, v10, v11"),
            Inst::FdivV2D {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
        );
    }

    #[test]
    fn parse_fabd_2d() {
        assert_eq!(
            parse_inst("fabd.2d v0, v1, v2"),
            Inst::FabdV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_and_16b() {
        assert_eq!(
            parse_inst("and.16b v6, v7, v8"),
            Inst::AndV16B {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_bic_16b() {
        assert_eq!(
            parse_inst("bic.16b v5, v6, v7"),
            Inst::BicV16B {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                rm: FpReg::new(7)
            }
        );
    }

    #[test]
    fn parse_bif_16b() {
        assert_eq!(
            parse_inst("bif.16b v0, v1, v2"),
            Inst::BifV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_bit_16b() {
        assert_eq!(
            parse_inst("bit.16b v3, v4, v5"),
            Inst::BitV16B {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_bsl_16b() {
        assert_eq!(
            parse_inst("bsl.16b v6, v7, v8"),
            Inst::BslV16B {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_cmeq_4s() {
        assert_eq!(
            parse_inst("cmeq.4s v0, v0, v1"),
            Inst::CmeqV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
        );
    }

    #[test]
    fn parse_fcmeq_4s() {
        assert_eq!(
            parse_inst("fcmeq.4s v0, v1, v2"),
            Inst::FcmeqV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fcmeq_2d() {
        assert_eq!(
            parse_inst("fcmeq.2d v0, v1, v2"),
            Inst::FcmeqV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_cmhs_4s() {
        assert_eq!(
            parse_inst("cmhs.4s v0, v0, v1"),
            Inst::CmhsV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
        );
    }

    #[test]
    fn parse_cmhi_4s() {
        assert_eq!(
            parse_inst("cmhi.4s v2, v3, v4"),
            Inst::CmhiV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_cmge_4s() {
        assert_eq!(
            parse_inst("cmge.4s v5, v6, v7"),
            Inst::CmgeV4S {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                rm: FpReg::new(7)
            }
        );
    }

    #[test]
    fn parse_fcmge_4s() {
        assert_eq!(
            parse_inst("fcmge.4s v3, v4, v5"),
            Inst::FcmgeV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fcmge_2d() {
        assert_eq!(
            parse_inst("fcmge.2d v3, v4, v5"),
            Inst::FcmgeV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_fcmge_2d_zero() {
        assert_eq!(
            parse_inst("fcmge.2d v0, v0, #0.0"),
            Inst::FcmgeZeroV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
        );
    }

    #[test]
    fn parse_cmgt_4s() {
        assert_eq!(
            parse_inst("cmgt.4s v2, v3, v4"),
            Inst::CmgtV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_fcmgt_4s() {
        assert_eq!(
            parse_inst("fcmgt.4s v6, v7, v8"),
            Inst::FcmgtV4S {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_fcmgt_2d() {
        assert_eq!(
            parse_inst("fcmgt.2d v6, v7, v8"),
            Inst::FcmgtV2D {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_fcmgt_2d_zero() {
        assert_eq!(
            parse_inst("fcmgt.2d v1, v1, #0.0"),
            Inst::FcmgtZeroV2D {
                rd: FpReg::new(1),
                rn: FpReg::new(1)
            }
        );
    }

    #[test]
    fn parse_fcmle_2d_zero() {
        assert_eq!(
            parse_inst("fcmle.2d v2, v2, #0.0"),
            Inst::FcmleZeroV2D {
                rd: FpReg::new(2),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_fcmlt_2d_zero() {
        assert_eq!(
            parse_inst("fcmlt.2d v3, v3, #0.0"),
            Inst::FcmltZeroV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(3)
            }
        );
    }

    #[test]
    fn parse_orr_16b() {
        assert_eq!(
            parse_inst("orr.16b v9, v10, v11"),
            Inst::OrrV16B {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
        );
    }

    #[test]
    fn parse_eor_16b() {
        assert_eq!(
            parse_inst("eor.16b v12, v13, v14"),
            Inst::EorV16B {
                rd: FpReg::new(12),
                rn: FpReg::new(13),
                rm: FpReg::new(14)
            }
        );
    }

    #[test]
    fn parse_ext_16b() {
        assert_eq!(
            parse_inst("ext.16b v0, v0, v0, #8"),
            Inst::ExtV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(0),
                index: 8
            }
        );
    }

    #[test]
    fn parse_rev64_4s() {
        assert_eq!(
            parse_inst("rev64.4s v1, v2"),
            Inst::Rev64V4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_zip1_4s() {
        assert_eq!(
            parse_inst("zip1.4s v0, v0, v1"),
            Inst::Zip1V4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
        );
    }

    #[test]
    fn parse_zip2_4s() {
        assert_eq!(
            parse_inst("zip2.4s v2, v3, v4"),
            Inst::Zip2V4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
        );
    }

    #[test]
    fn parse_uzp1_4s() {
        assert_eq!(
            parse_inst("uzp1.4s v5, v6, v7"),
            Inst::Uzp1V4S {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                rm: FpReg::new(7)
            }
        );
    }

    #[test]
    fn parse_uzp2_4s() {
        assert_eq!(
            parse_inst("uzp2.4s v8, v9, v10"),
            Inst::Uzp2V4S {
                rd: FpReg::new(8),
                rn: FpReg::new(9),
                rm: FpReg::new(10)
            }
        );
    }

    #[test]
    fn parse_trn1_4s() {
        assert_eq!(
            parse_inst("trn1.4s v11, v12, v13"),
            Inst::Trn1V4S {
                rd: FpReg::new(11),
                rn: FpReg::new(12),
                rm: FpReg::new(13)
            }
        );
    }

    #[test]
    fn parse_trn2_4s() {
        assert_eq!(
            parse_inst("trn2.4s v3, v4, v5"),
            Inst::Trn2V4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_zip1_2d() {
        assert_eq!(
            parse_inst("zip1.2d v0, v1, v2"),
            Inst::Zip1V2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_zip2_2d() {
        assert_eq!(
            parse_inst("zip2.2d v3, v4, v5"),
            Inst::Zip2V2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_uzp1_2d() {
        assert_eq!(
            parse_inst("uzp1.2d v6, v7, v8"),
            Inst::Uzp1V2D {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
        );
    }

    #[test]
    fn parse_uzp2_2d() {
        assert_eq!(
            parse_inst("uzp2.2d v9, v10, v11"),
            Inst::Uzp2V2D {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
        );
    }

    #[test]
    fn parse_trn1_2d() {
        assert_eq!(
            parse_inst("trn1.2d v12, v13, v14"),
            Inst::Trn1V2D {
                rd: FpReg::new(12),
                rn: FpReg::new(13),
                rm: FpReg::new(14)
            }
        );
    }

    #[test]
    fn parse_trn2_2d() {
        assert_eq!(
            parse_inst("trn2.2d v15, v16, v17"),
            Inst::Trn2V2D {
                rd: FpReg::new(15),
                rn: FpReg::new(16),
                rm: FpReg::new(17)
            }
        );
    }

    #[test]
    fn parse_tbl_16b_single() {
        assert_eq!(
            parse_inst("tbl.16b v0, { v1 }, v2"),
            Inst::TblV16B {
                rd: FpReg::new(0),
                table: FpReg::new(1),
                table_len: 1,
                index: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_tbl_16b_pair() {
        assert_eq!(
            parse_inst("tbl.16b v3, { v4, v5 }, v6"),
            Inst::TblV16B {
                rd: FpReg::new(3),
                table: FpReg::new(4),
                table_len: 2,
                index: FpReg::new(6)
            }
        );
    }

    #[test]
    fn parse_tbx_16b_pair() {
        assert_eq!(
            parse_inst("tbx.16b v21, { v22, v23 }, v24"),
            Inst::TbxV16B {
                rd: FpReg::new(21),
                table: FpReg::new(22),
                table_len: 2,
                index: FpReg::new(24)
            }
        );
    }

    #[test]
    fn parse_tbl_16b_requires_consecutive_table_regs() {
        let err = parse("tbl.16b v0, { v1, v3 }, v2").unwrap_err();
        assert!(err.msg.contains("consecutive"), "got: {}", err);
    }

    #[test]
    fn parse_mov_16b() {
        assert_eq!(
            parse_inst("mov.16b v0, v2"),
            Inst::MovV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(2)
            }
        );
    }

    #[test]
    fn parse_mov_8b() {
        assert_eq!(
            parse_inst("mov.8b v1, v3"),
            Inst::MovV8B {
                rd: FpReg::new(1),
                rn: FpReg::new(3)
            }
        );
    }

    #[test]
    fn parse_mov_4s() {
        assert_eq!(
            parse_inst("mov.4s v4, v5"),
            Inst::MovV4S {
                rd: FpReg::new(4),
                rn: FpReg::new(5)
            }
        );
    }

    #[test]
    fn parse_mov_2d() {
        assert_eq!(
            parse_inst("mov.2d v6, v7"),
            Inst::MovV2D {
                rd: FpReg::new(6),
                rn: FpReg::new(7)
            }
        );
    }

    #[test]
    fn parse_mov_from_lane_s() {
        assert_eq!(
            parse_inst("mov s0, v1[2]"),
            Inst::MovFromLaneS {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                index: 2
            }
        );
    }

    #[test]
    fn parse_fmov_reg_s() {
        assert_eq!(parse_inst("fmov s1, s2"), Inst::FmovRegS { rd: S1, rn: S2 });
    }

    #[test]
    fn parse_fmov_reg_d() {
        assert_eq!(parse_inst("fmov d1, d2"), Inst::FmovRegD { rd: D1, rn: D2 });
    }

    #[test]
    fn parse_fmov_to_s() {
        assert_eq!(parse_inst("fmov s0, w1"), Inst::FmovToS { rd: S0, rn: W1 });
    }

    #[test]
    fn parse_fmov_from_s() {
        assert_eq!(
            parse_inst("fmov w0, s1"),
            Inst::FmovFromS { rd: W0, rn: S1 }
        );
    }

    #[test]
    fn parse_mov_from_lane_d() {
        assert_eq!(
            parse_inst("mov d3, v4[1]"),
            Inst::MovFromLaneD {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                index: 1
            }
        );
    }

    #[test]
    fn parse_mov_lane_s() {
        assert_eq!(
            parse_inst("mov.s v5[0], v6[0]"),
            Inst::MovLaneS {
                rd: FpReg::new(5),
                rd_index: 0,
                rn: FpReg::new(6),
                rn_index: 0
            }
        );
    }

    #[test]
    fn parse_mov_lane_d() {
        assert_eq!(
            parse_inst("mov.d v7[1], v8[1]"),
            Inst::MovLaneD {
                rd: FpReg::new(7),
                rd_index: 1,
                rn: FpReg::new(8),
                rn_index: 1
            }
        );
    }

    #[test]
    fn parse_mov_lane_h() {
        assert_eq!(
            parse_inst("mov.h v0[5], v1[0]"),
            Inst::MovLaneH {
                rd: FpReg::new(0),
                rd_index: 5,
                rn: FpReg::new(1),
                rn_index: 0
            }
        );
    }

    #[test]
    fn parse_mov_lane_b() {
        assert_eq!(
            parse_inst("mov.b v0[7], v1[0]"),
            Inst::MovLaneB {
                rd: FpReg::new(0),
                rd_index: 7,
                rn: FpReg::new(1),
                rn_index: 0
            }
        );
    }

    #[test]
    fn parse_mov_from_lane_gp_s() {
        assert_eq!(
            parse_inst("mov.s w0, v1[2]"),
            Inst::MovFromLaneGpS {
                rd: W0,
                rn: FpReg::new(1),
                index: 2
            }
        );
    }

    #[test]
    fn parse_mov_from_lane_gp_d() {
        assert_eq!(
            parse_inst("mov.d x0, v1[1]"),
            Inst::MovFromLaneGpD {
                rd: X0,
                rn: FpReg::new(1),
                index: 1
            }
        );
    }

    #[test]
    fn parse_umov_h() {
        assert_eq!(
            parse_inst("umov.h w1, v2[5]"),
            Inst::UmovFromLaneH {
                rd: W1,
                rn: FpReg::new(2),
                index: 5
            }
        );
    }

    #[test]
    fn parse_umov_b() {
        assert_eq!(
            parse_inst("umov.b w3, v4[7]"),
            Inst::UmovFromLaneB {
                rd: W3,
                rn: FpReg::new(4),
                index: 7
            }
        );
    }

    #[test]
    fn parse_smov_h() {
        assert_eq!(
            parse_inst("smov.h w1, v2[3]"),
            Inst::SmovFromLaneH {
                rd: W1,
                rn: FpReg::new(2),
                index: 3
            }
        );
    }

    #[test]
    fn parse_smov_b() {
        assert_eq!(
            parse_inst("smov.b w0, v0[0]"),
            Inst::SmovFromLaneB {
                rd: W0,
                rn: FpReg::new(0),
                index: 0
            }
        );
    }

    #[test]
    fn parse_mov_lane_from_gp_s() {
        assert_eq!(
            parse_inst("mov.s v5[1], w6"),
            Inst::MovLaneFromGpS {
                rd: FpReg::new(5),
                rd_index: 1,
                rn: W6
            }
        );
    }

    #[test]
    fn parse_mov_lane_from_gp_d() {
        assert_eq!(
            parse_inst("mov.d v0[1], x1"),
            Inst::MovLaneFromGpD {
                rd: FpReg::new(0),
                rd_index: 1,
                rn: X1
            }
        );
    }

    #[test]
    fn parse_mov_lane_from_gp_h() {
        assert_eq!(
            parse_inst("mov.h v7[5], w8"),
            Inst::MovLaneFromGpH {
                rd: FpReg::new(7),
                rd_index: 5,
                rn: W8
            }
        );
    }

    #[test]
    fn parse_mov_lane_from_gp_b() {
        assert_eq!(
            parse_inst("mov.b v9[7], w10"),
            Inst::MovLaneFromGpB {
                rd: FpReg::new(9),
                rd_index: 7,
                rn: W10
            }
        );
    }

    #[test]
    fn parse_ld1_lane_s() {
        assert_eq!(
            parse_inst("ld1.s { v0 }[1], [x8]"),
            Inst::Ld1LaneS {
                rt: FpReg::new(0),
                index: 1,
                rn: X8
            }
        );
    }

    #[test]
    fn parse_ld1_lane_d() {
        assert_eq!(
            parse_inst("ld1.d { v2 }[1], [x10]"),
            Inst::Ld1LaneD {
                rt: FpReg::new(2),
                index: 1,
                rn: X10
            }
        );
    }

    #[test]
    fn parse_ld1_lane_h() {
        assert_eq!(
            parse_inst("ld1.h { v3 }[5], [x11]"),
            Inst::Ld1LaneH {
                rt: FpReg::new(3),
                index: 5,
                rn: X11
            }
        );
    }

    #[test]
    fn parse_ld1_lane_b() {
        assert_eq!(
            parse_inst("ld1.b { v4 }[7], [x12]"),
            Inst::Ld1LaneB {
                rt: FpReg::new(4),
                index: 7,
                rn: X12
            }
        );
    }

    #[test]
    fn parse_dup_16b() {
        assert_eq!(
            parse_inst("dup.16b v0, v1[15]"),
            Inst::DupV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                index: 15
            }
        );
    }

    #[test]
    fn parse_dup_8h() {
        assert_eq!(
            parse_inst("dup.8h v1, v2[5]"),
            Inst::DupV8H {
                rd: FpReg::new(1),
                rn: FpReg::new(2),
                index: 5
            }
        );
    }

    #[test]
    fn parse_dup_4s() {
        assert_eq!(
            parse_inst("dup.4s v3, v4[2]"),
            Inst::DupV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                index: 2
            }
        );
    }

    #[test]
    fn parse_dup_2d() {
        assert_eq!(
            parse_inst("dup.2d v5, v6[1]"),
            Inst::DupV2D {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                index: 1
            }
        );
    }

    #[test]
    fn parse_fcmp_() {
        assert_eq!(parse_inst("fcmp d0, d1"), Inst::FcmpD { rn: D0, rm: D1 });
    }

    #[test]
    fn parse_fmadd_() {
        assert_eq!(
            parse_inst("fmadd d0, d1, d2, d3"),
            Inst::FmaddD {
                rd: D0,
                rn: D1,
                rm: D2,
                ra: D3
            }
        );
    }

    #[test]
    fn parse_fcvtzs_() {
        assert_eq!(
            parse_inst("fcvtzs x0, d1"),
            Inst::Fcvtzs {
                rd: X0,
                rn: D1,
                dst_64bit: true,
                src_double: true,
            }
        );
    }

    #[test]
    fn parse_fcvt_between_single_and_double() {
        assert_eq!(
            parse_inst("fcvt d8, s9"),
            Inst::FcvtDFromS { rd: D8, rn: S9 }
        );
        assert_eq!(
            parse_inst("fcvt s10, d11"),
            Inst::FcvtSFromD { rd: S10, rn: D11 }
        );
    }

    #[test]
    fn reject_fcvt_with_matching_widths() {
        for source in ["fcvt d0, d1", "fcvt s0, s1"] {
            let err = parse_err(source);
            assert!(
                err.contains("one s-register and one d-register"),
                "{source}: {err}"
            );
        }
    }

    #[test]
    fn parse_scvtf_() {
        assert_eq!(
            parse_inst("scvtf d0, x1"),
            Inst::Scvtf {
                rd: D0,
                rn: X1,
                dst_double: true,
                src_64bit: true,
            }
        );
    }

    #[test]
    fn parse_fmov_to_fp() {
        assert_eq!(parse_inst("fmov d0, x1"), Inst::FmovToD { rd: D0, rn: X1 });
    }

    #[test]
    fn parse_fmov_imm_d() {
        assert_eq!(
            parse_inst("fmov d2, #3.50000000"),
            Inst::FmovImmD { rd: D2, imm8: 12 }
        );
    }

    #[test]
    fn parse_fmov_imm_s() {
        assert_eq!(
            parse_inst("fmov s2, #3.50000000"),
            Inst::FmovImmS { rd: S2, imm8: 12 }
        );
    }

    #[test]
    fn parse_fmov_from_fp() {
        assert_eq!(
            parse_inst("fmov x0, d1"),
            Inst::FmovFromD { rd: X0, rn: D1 }
        );
    }

    #[test]
    fn parse_fcsel_d() {
        assert_eq!(
            parse_inst("fcsel d0, d0, d1, mi"),
            Inst::FcselD {
                rd: D0,
                rn: D0,
                rm: D1,
                cond: Cond::MI
            }
        );
    }

    #[test]
    fn parse_fcsel_s() {
        assert_eq!(
            parse_inst("fcsel s0, s0, s1, mi"),
            Inst::FcselS {
                rd: S0,
                rn: S0,
                rm: S1,
                cond: Cond::MI
            }
        );
    }

    // ---- FP single-precision ----

    #[test]
    fn parse_fneg_s() {
        assert_eq!(parse_inst("fneg s0, s1"), Inst::FnegS { rd: S0, rn: S1 });
    }

    #[test]
    fn parse_fabs_s() {
        assert_eq!(parse_inst("fabs s0, s1"), Inst::FabsS { rd: S0, rn: S1 });
    }

    #[test]
    fn parse_fsqrt_s() {
        assert_eq!(parse_inst("fsqrt s0, s1"), Inst::FsqrtS { rd: S0, rn: S1 });
    }

    #[test]
    fn parse_fcmp_s() {
        assert_eq!(parse_inst("fcmp s0, s1"), Inst::FcmpS { rn: S0, rm: S1 });
    }

    #[test]
    fn parse_fmadd_s() {
        assert_eq!(
            parse_inst("fmadd s0, s1, s2, s3"),
            Inst::FmaddS {
                rd: S0,
                rn: S1,
                rm: S2,
                ra: S3
            }
        );
    }

    // ---- System ----

    #[test]
    fn parse_svc_() {
        assert_eq!(parse_inst("svc #0x80"), Inst::Svc { imm16: 0x80 });
    }

    #[test]
    fn parse_svc_expression() {
        assert_eq!(parse_inst("svc #0x40 + 0x40"), Inst::Svc { imm16: 0x80 });
    }

    #[test]
    fn parse_nop_() {
        assert_eq!(parse_inst("nop"), Inst::Nop);
    }

    #[test]
    fn parse_yield_() {
        assert_eq!(parse_inst("yield"), Inst::Yield);
    }

    #[test]
    fn parse_wfe_() {
        assert_eq!(parse_inst("wfe"), Inst::Wfe);
    }

    #[test]
    fn parse_sevl_() {
        assert_eq!(parse_inst("sevl"), Inst::Sevl);
    }

    #[test]
    fn parse_dmb_ish() {
        assert_eq!(
            parse_inst("dmb ish"),
            Inst::Dmb {
                option: BarrierOpt::Ish
            }
        );
    }

    #[test]
    fn parse_dsb_ishst() {
        assert_eq!(
            parse_inst("dsb ishst"),
            Inst::Dsb {
                option: BarrierOpt::Ishst
            }
        );
    }

    #[test]
    fn parse_isb_default_sy() {
        assert_eq!(
            parse_inst("isb"),
            Inst::Isb {
                option: BarrierOpt::Sy
            }
        );
    }

    #[test]
    fn parse_isb_sy() {
        assert_eq!(
            parse_inst("isb sy"),
            Inst::Isb {
                option: BarrierOpt::Sy
            }
        );
    }

    #[test]
    fn error_unknown_barrier_option() {
        let err = parse_err("dmb bogus");
        assert!(err.contains("unknown dmb option"), "got: {}", err);
    }

    #[test]
    fn parse_brk_() {
        assert_eq!(parse_inst("brk #1"), Inst::Brk { imm16: 1 });
    }

    // ---- Labels and directives ----

    #[test]
    fn parse_label() {
        let stmts = parse_stmts("_main:");
        assert_eq!(stmts, vec![Stmt::Label("_main".into())]);
    }

    #[test]
    fn parse_local_label() {
        let stmts = parse_stmts(".Lloop:");
        assert_eq!(stmts, vec![Stmt::Label(".Lloop".into())]);
    }

    #[test]
    fn parse_chained_labels_on_one_line() {
        assert_eq!(
            parse_stmts(".Llocal: named: 1: ret"),
            vec![
                Stmt::Label(".Llocal".into()),
                Stmt::Label("named".into()),
                Stmt::Label(".Ltmp$1$1".into()),
                Stmt::Instruction(Inst::Ret { rn: X30 }),
            ]
        );
        assert_eq!(
            parse_stmts(".Llocal: named: 1: .byte 1, 2, 3"),
            vec![
                Stmt::Label(".Llocal".into()),
                Stmt::Label("named".into()),
                Stmt::Label(".Ltmp$1$1".into()),
                Stmt::Directive(Directive::Byte(vec![
                    Expr::Int(1),
                    Expr::Int(2),
                    Expr::Int(3),
                ])),
            ]
        );
    }

    #[test]
    fn reject_trailing_tokens_after_arm64_statements() {
        for (source, col, token) in [
            ("nop ret", 5, "ret"),
            ("named: nop ret", 12, "ret"),
            (".Llocal: nop ret", 14, "ret"),
            ("1: nop ret", 8, "ret"),
            (".byte 1 ret", 9, "ret"),
            (".text nop", 7, "nop"),
        ] {
            let error = parse(source).expect_err("trailing token unexpectedly parsed");
            assert_eq!(error.line, 1, "source: {source}");
            assert_eq!(error.col, col, "source: {source}");
            assert_eq!(
                error.msg,
                format!("unexpected trailing token: {token}"),
                "source: {source}"
            );
        }
    }

    #[test]
    fn parse_global_directive() {
        let stmts = parse_stmts(".global _main");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Global("_main".into()))]
        );
    }

    #[test]
    fn parse_extern_directive() {
        let stmts = parse_stmts(".extern _puts");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Extern("_puts".into()))]
        );
    }

    #[test]
    fn parse_comm_directive() {
        let stmts = parse_stmts(".comm _common, 24, 3");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Comm {
                name: "_common".into(),
                size: 24,
                align_pow2: 3,
            })]
        );
    }

    #[test]
    fn parse_private_extern_directive() {
        let stmts = parse_stmts(".private_extern _hidden");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::PrivateExtern("_hidden".into()))]
        );
    }

    #[test]
    fn parse_weak_reference_directive() {
        let stmts = parse_stmts(".weak_reference _puts");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::WeakReference("_puts".into()))]
        );
    }

    #[test]
    fn parse_weak_definition_directive() {
        let stmts = parse_stmts(".weak_definition _entry");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::WeakDefinition("_entry".into()))]
        );
    }

    #[test]
    fn parse_set_directive() {
        let stmts = parse_stmts(".set ABS1, 7");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Set("ABS1".into(), Expr::Int(7)))]
        );
    }

    #[test]
    fn parse_equ_directive() {
        let stmts = parse_stmts(".set ABS1, 7\n.equ ABS2, ABS1 + 5");
        assert_eq!(
            stmts,
            vec![
                Stmt::Directive(Directive::Set("ABS1".into(), Expr::Int(7))),
                Stmt::Directive(Directive::Set(
                    "ABS2".into(),
                    Expr::Add(
                        Box::new(Expr::Symbol("ABS1".into())),
                        Box::new(Expr::Int(5))
                    ),
                )),
            ]
        );
    }

    #[test]
    fn parse_asciz_directive() {
        let stmts = parse_stmts(".asciz \"Hello\\n\"");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Asciz(b"Hello\n\0".to_vec()))]
        );
    }

    #[test]
    fn parse_align_directive() {
        let stmts = parse_stmts(".p2align 4");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::P2Align {
                power: 4,
                fill: None,
                max_skip: None,
            })]
        );
    }

    #[test]
    fn parse_align_directive_expression() {
        let stmts = parse_stmts(".align 1 + 1");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Align {
                power: 2,
                fill: None,
                max_skip: None,
            })]
        );
    }

    #[test]
    fn parse_align_directive_with_fill_and_max_skip() {
        let stmts = parse_stmts(".p2align 4, 0xAA, 2");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::P2Align {
                power: 4,
                fill: Some(0xAA),
                max_skip: Some(2),
            })]
        );
    }

    #[test]
    fn parse_align_directive_with_omitted_fill() {
        let stmts = parse_stmts(".align 4,,2");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Align {
                power: 4,
                fill: None,
                max_skip: Some(2),
            })]
        );
    }

    #[test]
    fn parse_byte_directive() {
        let stmts = parse_stmts(".byte 0x41, 0x42, 0x43");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Byte(vec![
                Expr::Int(0x41),
                Expr::Int(0x42),
                Expr::Int(0x43),
            ]))]
        );
    }

    #[test]
    fn parse_short_directive() {
        let stmts = parse_stmts(".short 0x1234, 2");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Short(vec![
                Expr::Int(0x1234),
                Expr::Int(2)
            ]))]
        );
    }

    #[test]
    fn parse_word_directive_expression() {
        let stmts = parse_stmts(".word 1 + 2 - 3");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Word(vec![Expr::Sub(
                Box::new(Expr::Add(Box::new(Expr::Int(1)), Box::new(Expr::Int(2)))),
                Box::new(Expr::Int(3)),
            )]))]
        );
    }

    #[test]
    fn parse_quad_directive_parenthesized_expression() {
        let stmts = parse_stmts(".quad -(1 + 2)");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Quad(vec![Expr::UnaryMinus(
                Box::new(Expr::Add(Box::new(Expr::Int(1)), Box::new(Expr::Int(2)),))
            )]))]
        );
    }

    #[test]
    fn expression_depth_limit_accepts_the_boundary() {
        let source = format!(".quad {}1", "-".repeat(MAX_EXPRESSION_DEPTH));
        parse(&source).expect("boundary-depth expression unexpectedly rejected");
    }

    #[test]
    fn expression_depth_limit_rejects_deep_expression_shapes() {
        let unary = format!(".data\n.quad {}1", "-".repeat(MAX_EXPRESSION_DEPTH + 1));
        let parenthesized = format!(
            ".data\n.quad {}1{}",
            "(".repeat(MAX_EXPRESSION_DEPTH + 1),
            ")".repeat(MAX_EXPRESSION_DEPTH + 1)
        );
        let additive = format!(
            ".data\n.quad {}",
            vec!["1"; MAX_EXPRESSION_DEPTH + 2].join("+")
        );

        for (shape, source, expected_col) in [
            ("unary", unary, 263),
            ("parenthesized", parenthesized, 263),
            ("additive", additive, 520),
        ] {
            let error = parse(&source).expect_err("deep expression unexpectedly parsed");
            assert_eq!(error.line, 2, "wrong line for {shape} expression");
            assert_eq!(
                error.col, expected_col,
                "wrong column for {shape} expression"
            );
            assert_eq!(
                error.msg,
                format!("expression exceeds maximum depth of {MAX_EXPRESSION_DEPTH}"),
                "wrong diagnostic for {shape} expression"
            );
        }
    }

    #[test]
    fn parse_unsigned_quad_values_in_labels_comments_and_lists() {
        assert_eq!(
            parse_stmts(
                "bits: .quad 0xbff0000000000000, 0xffffffffffffffff // bit patterns\n\
                 .quad 18446744073709551615 ; decimal u64 max\n"
            ),
            vec![
                Stmt::Label("bits".into()),
                Stmt::Directive(Directive::Quad(vec![
                    Expr::Unsigned(0xbff0000000000000),
                    Expr::Unsigned(u64::MAX),
                ])),
                Stmt::Directive(Directive::Quad(vec![Expr::Unsigned(u64::MAX)])),
            ]
        );
    }

    #[test]
    fn signed_minimum_and_full_width_assignments_preserve_their_bits() {
        assert_eq!(
            parse_stmts(".quad -9223372036854775808"),
            vec![Stmt::Directive(Directive::Quad(vec![Expr::Int(i64::MIN)]))]
        );
        assert_eq!(
            parse_stmts(".set ALL_ONES, 0xffffffffffffffff"),
            vec![Stmt::Directive(Directive::Set(
                "ALL_ONES".into(),
                Expr::Int(-1),
            ))]
        );

        let error = parse(".quad -9223372036854775809").unwrap_err();
        assert_eq!((error.line, error.col), (1, 8));
        assert_eq!(
            error.msg,
            "unsigned integer literals above i64::MAX must be standalone .quad values"
        );
    }

    #[test]
    fn full_width_assignments_reject_compound_unsigned_expressions() {
        for source in [
            ".set X,0xffffffffffffffff + 1",
            ".equ X,1 + 0xffffffffffffffff",
        ] {
            let error = parse(source).unwrap_err();
            assert_eq!(
                error.msg,
                "unsigned integer literals above i64::MAX must be standalone absolute assignment values"
            );
        }
    }

    #[test]
    fn wide_unsigned_literals_are_restricted_to_standalone_quad_values() {
        let err = parse(".word 0xbff0000000000000").unwrap_err();
        assert_eq!(
            err.msg,
            ".word expression value 13830554455654793216 is out of range for 32-bit data"
        );

        for source in [
            ".quad 0xbff0000000000000 + 1",
            ".quad symbol + 0xbff0000000000000",
            ".quad -0xbff0000000000000",
            ".quad 0x10000000000000000",
            ".quad 18446744073709551616",
        ] {
            let err = parse(source).unwrap_err();
            assert!(
                err.msg.contains("i64 range")
                    || err.msg.contains("standalone .quad")
                    || err.msg.contains("invalid hex")
                    || err.msg.contains("invalid integer"),
                "{source}: {}",
                err.msg
            );
        }
    }

    #[test]
    fn compound_wide_quad_error_points_to_the_wide_literal() {
        let err = parse(".quad 0xffffffffffffffff + 1").unwrap_err();
        assert_eq!((err.line, err.col), (1, 7));
        assert_eq!(
            err.msg,
            "unsigned integer literals above i64::MAX must be standalone .quad values"
        );

        let err = parse(".quad symbol + 0xffffffffffffffff").unwrap_err();
        assert_eq!((err.line, err.col), (1, 16));
    }

    #[test]
    fn wide_fp_immediates_report_integer_range_errors() {
        for source in [
            "fmov d0, #0xffffffffffffffff",
            "fcmeq.2d v0, v1, #0xffffffffffffffff",
        ] {
            let err = parse(source).unwrap_err();
            assert!(err.msg.contains("exceeds the i64 range"), "{source}: {err}");
        }
    }

    #[test]
    fn parse_space_directive_expression() {
        let stmts = parse_stmts(".space (2 + 3)");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Space(5))]);
    }

    #[test]
    fn parse_zero_directive() {
        let stmts = parse_stmts(".zero 3");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Space(3))]);
    }

    #[test]
    fn parse_fill_directive() {
        let stmts = parse_stmts(".fill 2, 2, 0x3344");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Fill {
                repeat: 2,
                size: 2,
                value: 0x3344,
            })]
        );
    }

    #[test]
    fn parse_cstring_directive() {
        let stmts = parse_stmts(".cstring");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Section(
                "__TEXT".into(),
                "__cstring".into()
            ))]
        );
    }

    #[test]
    fn parse_zerofill_directive() {
        let stmts = parse_stmts(".zerofill __DATA, __bss, _scratch, 16, 4");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Zerofill {
                segment: "__DATA".into(),
                section: "__bss".into(),
                symbol: Some("_scratch".into()),
                size: 16,
                align_pow2: 4,
            })]
        );
    }

    #[test]
    fn parse_zerofill_without_symbol() {
        let stmts = parse_stmts(".zerofill __DATA, __bss, , 8, 2");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Zerofill {
                segment: "__DATA".into(),
                section: "__bss".into(),
                symbol: None,
                size: 8,
                align_pow2: 2,
            })]
        );
    }

    #[test]
    fn parse_tbss_directive() {
        let stmts = parse_stmts(".tbss _tls_counter$tlv$init, 4, 2");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Zerofill {
                segment: "__DATA".into(),
                section: "__thread_bss".into(),
                symbol: Some("_tls_counter$tlv$init".into()),
                size: 4,
                align_pow2: 2,
            })]
        );
    }

    #[test]
    fn parse_text_section_with_attrs() {
        let stmts = parse_stmts(".section __TEXT,__text,regular,pure_instructions");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Section(
                "__TEXT".into(),
                "__text".into()
            ))]
        );
    }

    #[test]
    fn parse_cstring_section_with_attrs() {
        let stmts = parse_stmts(".section __TEXT,__cstring,cstring_literals");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Section(
                "__TEXT".into(),
                "__cstring".into()
            ))]
        );
    }

    #[test]
    fn parse_thread_data_section_with_attrs() {
        let stmts = parse_stmts(".section __DATA,__thread_data,thread_local_regular");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Section(
                "__DATA".into(),
                "__thread_data".into()
            ))]
        );
    }

    #[test]
    fn parse_literal16_section_with_attrs() {
        let stmts = parse_stmts(".section __TEXT,__literal16,16byte_literals");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Section(
                "__TEXT".into(),
                "__literal16".into()
            ))]
        );
    }

    #[test]
    fn parse_thread_vars_section_with_attrs() {
        let stmts = parse_stmts(".section __DATA,__thread_vars,thread_local_variables");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Section(
                "__DATA".into(),
                "__thread_vars".into()
            ))]
        );
    }

    #[test]
    fn parse_section_with_unsupported_attrs_errors() {
        let err = parse_err(".section __TEXT,__text,regular,garbage");
        assert!(
            err.contains(
                "unsupported section attributes for __TEXT,__text: garbage (supported attrs: regular, pure_instructions)"
            ),
            "got: {}",
            err
        );
    }

    #[test]
    fn parse_const_section_with_regular_attr() {
        let stmts = parse_stmts(".section __TEXT,__const,regular");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Section(
                "__TEXT".into(),
                "__const".into()
            ))]
        );
    }

    #[test]
    fn parse_data_const_section_with_regular_attr() {
        let stmts = parse_stmts(".section __DATA,__const,regular");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Section(
                "__DATA".into(),
                "__const".into()
            ))]
        );
    }

    #[test]
    fn parse_text_section_pure_without_regular_errors() {
        let err = parse_err(".section __TEXT,__text,pure_instructions");
        assert!(
            err.contains("section __TEXT,__text requires 'regular' when using 'pure_instructions'"),
            "got: {}",
            err
        );
    }

    #[test]
    fn parse_unknown_directive_errors() {
        let err = parse_err(".unknown_directive");
        assert!(
            err.contains("unsupported directive '.unknown_directive'"),
            "got: {}",
            err
        );
    }

    #[test]
    fn parse_build_version_with_sdk_version() {
        let stmts = parse_stmts(".build_version macos, 11, 0 sdk_version 15, 5");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::BuildVersion(
                BuildVersionDirective {
                    platform: "macos".into(),
                    minos: VersionTriple {
                        major: 11,
                        minor: 0,
                        patch: 0
                    },
                    sdk: Some(VersionTriple {
                        major: 15,
                        minor: 5,
                        patch: 0
                    }),
                }
            ))]
        );
    }

    #[test]
    fn parse_cfi_startproc() {
        let stmts = parse_stmts(".cfi_startproc");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::CfiStartProc)]);
    }

    #[test]
    fn parse_cfi_def_cfa() {
        let stmts = parse_stmts(".cfi_def_cfa w29, 16");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::CfiDefCfa {
                register: W29,
                offset: 16,
            })]
        );
    }

    #[test]
    fn parse_cfi_register_31_spellings() {
        for spelling in ["sp", "wsp", "xzr", "wzr", "x31", "w31"] {
            let source = format!(
                ".cfi_def_cfa {spelling}, 16\n\
                 .cfi_def_cfa_register {spelling}\n\
                 .cfi_offset {spelling}, -8\n\
                 .cfi_restore {spelling}"
            );
            assert_eq!(
                parse_stmts(&source),
                vec![
                    Stmt::Directive(Directive::CfiDefCfa {
                        register: SP,
                        offset: 16,
                    }),
                    Stmt::Directive(Directive::CfiDefCfaRegister(SP)),
                    Stmt::Directive(Directive::CfiOffset {
                        register: SP,
                        offset: -8,
                    }),
                    Stmt::Directive(Directive::CfiRestore(SP)),
                ],
                "spelling: {spelling}"
            );
        }
    }

    #[test]
    fn parse_cfi_offset() {
        let stmts = parse_stmts(".cfi_offset w30, -8");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::CfiOffset {
                register: W30,
                offset: -8,
            })]
        );
    }

    #[test]
    fn parse_unsupported_cfi_directive_errors() {
        let err = parse(".cfi_escape 0x1").unwrap_err();
        assert!(
            err.msg.contains("unsupported CFI directive"),
            "got: {}",
            err.msg
        );
    }

    #[test]
    fn parse_build_version_without_sdk_version() {
        let stmts = parse_stmts(".build_version macos, 14, 1");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::BuildVersion(
                BuildVersionDirective {
                    platform: "macos".into(),
                    minos: VersionTriple {
                        major: 14,
                        minor: 1,
                        patch: 0
                    },
                    sdk: None,
                }
            ))]
        );
    }

    #[test]
    fn parse_linker_optimization_hint() {
        let stmts = parse_stmts(".loh AdrpAdd Lloh0, Lloh1");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::LinkerOptimizationHint(
                LinkerOptimizationHintDirective {
                    kind: "AdrpAdd".into(),
                    labels: vec!["Lloh0".into(), "Lloh1".into()],
                }
            ))]
        );
    }

    #[test]
    fn parse_linker_optimization_hint_requires_expected_label_count() {
        let err = parse(".loh AdrpAdd Lloh0").unwrap_err();
        assert_eq!(err.line, 1);
        assert_eq!(err.col, 19);
        assert_eq!(err.msg, ".loh AdrpAdd expects 2 labels, got 1");
    }

    #[test]
    fn parse_linker_optimization_hint_requires_commas() {
        let err = parse(".loh AdrpAdd Lloh0 Lloh1").unwrap_err();
        assert_eq!(err.line, 1);
        assert_eq!(err.col, 20);
        assert_eq!(err.msg, "expected ,, got Lloh1");
    }

    #[test]
    fn parse_linker_optimization_hint_unknown_kind_errors() {
        let err = parse(".loh UnknownKind Lloh0").unwrap_err();
        assert_eq!(err.line, 1);
        assert_eq!(err.col, 18);
        assert_eq!(
            err.msg,
            "unsupported .loh kind 'UnknownKind' (supported: AdrpAdd, AdrpLdr, AdrpLdrGot, AdrpLdrGotLdr)"
        );
    }

    // ---- Multi-line programs ----

    #[test]
    fn parse_hello_world() {
        let src = "\
.global _main
.align 4

_main:
    mov x0, #1
    mov x16, #4
    svc #0x80
    mov x0, #0
    mov x16, #1
    svc #0x80
";
        let stmts = parse_stmts(src);
        // Should have: global, align, label, 6 instructions
        let labels = stmts.iter().filter(|s| matches!(s, Stmt::Label(_))).count();
        let insts = stmts
            .iter()
            .filter(|s| matches!(s, Stmt::Instruction(_)))
            .count();
        assert_eq!(labels, 1);
        assert_eq!(insts, 6);
    }

    // ---- Error cases ----

    #[test]
    fn error_unknown_mnemonic() {
        let result = parse("blorp x0, x1, x2");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.msg.contains("unknown mnemonic"), "got: {}", err.msg);
    }

    #[test]
    fn error_bad_register() {
        let result = parse("add x0, x1, x99");
        assert!(result.is_err());
    }

    #[test]
    fn error_missing_comma() {
        let result = parse("add x0 x1 x2");
        assert!(result.is_err());
    }

    #[test]
    fn error_str_requires_bracketed_operand() {
        let result = parse("str x0, some_label");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.msg.contains("bracketed memory operand"),
            "got: {}",
            err.msg
        );
    }

    #[test]
    fn parse_symbolic_quad_expression() {
        let stmts = parse_stmts(".quad foo - 1");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Quad(vec![Expr::Sub(
                Box::new(Expr::Symbol("foo".into())),
                Box::new(Expr::Int(1)),
            )]))]
        );
    }

    #[test]
    fn parse_quad_got_expression() {
        assert_eq!(
            parse_stmts(".quad _puts@GOT"),
            vec![Stmt::Directive(Directive::Quad(vec![
                Expr::ModifiedSymbol {
                    symbol: "_puts".into(),
                    modifier: SymbolModifier::Got,
                }
            ]))]
        );
    }

    #[test]
    fn parse_word_got_pcrel_expression() {
        assert_eq!(
            parse_stmts(".long _puts@GOT - ."),
            vec![Stmt::Directive(Directive::Word(vec![Expr::Sub(
                Box::new(Expr::ModifiedSymbol {
                    symbol: "_puts".into(),
                    modifier: SymbolModifier::Got,
                }),
                Box::new(Expr::CurrentLocation),
            )]))]
        );
    }

    // ---- Case insensitivity ----

    #[test]
    fn parse_uppercase_add() {
        assert_eq!(
            parse_inst("ADD X0, X1, X2"),
            Inst::AddReg {
                rd: X0,
                rn: X1,
                rm: X2,
                sf: true
            }
        );
    }

    #[test]
    fn parse_uppercase_b_eq() {
        assert_eq!(
            parse_inst("B.EQ #8"),
            Inst::BCond {
                cond: Cond::EQ,
                offset: 8
            }
        );
    }

    #[test]
    fn parse_mixed_case_ldr() {
        assert_eq!(
            parse_inst("Ldr X0, [X1, #8]"),
            Inst::LdrImm64 {
                rt: X0,
                rn: X1,
                offset: 8
            }
        );
    }

    #[test]
    fn parse_uppercase_add_does_not_treat_x2_as_label() {
        // BUG 3 regression: uppercase X2 must be parsed as register, not label
        assert_eq!(
            parse_inst("ADD X0, X1, X2"),
            Inst::AddReg {
                rd: X0,
                rn: X1,
                rm: X2,
                sf: true
            }
        );
    }

    #[test]
    fn parse_uppercase_mov_sp() {
        assert_eq!(
            parse_inst("MOV X29, SP"),
            Inst::AddImm {
                rd: X29,
                rn: SP,
                imm12: 0,
                shift: false,
                sf: true
            }
        );
    }

    // ---- Test gap coverage (from audit) ----

    #[test]
    fn parse_negative_branch() {
        assert_eq!(parse_inst("b #-8"), Inst::B { offset: -8 });
    }

    #[test]
    fn parse_branch_expression() {
        assert_eq!(parse_inst("b #4 + 4"), Inst::B { offset: 8 });
    }

    #[test]
    fn parse_negative_bl() {
        assert_eq!(parse_inst("bl #-16"), Inst::Bl { offset: -16 });
    }

    #[test]
    fn parse_w_register_shift() {
        assert_eq!(
            parse_inst("lsl w0, w1, #3"),
            Inst::LslImm {
                rd: W0,
                rn: W1,
                amount: 3,
                sf: false
            }
        );
    }

    #[test]
    fn parse_w_register_lsr() {
        assert_eq!(
            parse_inst("lsr w5, w6, #8"),
            Inst::LsrImm {
                rd: W5,
                rn: W6,
                amount: 8,
                sf: false
            }
        );
    }

    #[test]
    fn parse_w_register_asr() {
        assert_eq!(
            parse_inst("asr w5, w6, #15"),
            Inst::AsrImm {
                rd: W5,
                rn: W6,
                amount: 15,
                sf: false
            }
        );
    }

    #[test]
    fn parse_mov_negative_imm() {
        // mov x0, #-1 → movn x0, #0
        assert_eq!(
            parse_inst("mov x0, #-1"),
            Inst::Movn {
                rd: X0,
                imm16: 0,
                shift: 0,
                sf: true
            }
        );
    }

    #[test]
    fn parse_mov_negative_42() {
        // mov x0, #-42 → movn x0, #41
        assert_eq!(
            parse_inst("mov x0, #-42"),
            Inst::Movn {
                rd: X0,
                imm16: 41,
                shift: 0,
                sf: true
            }
        );
    }

    #[test]
    fn parse_mov_negative_65537() {
        assert_eq!(
            parse_inst("mov x0, #-65537"),
            Inst::Movn {
                rd: X0,
                imm16: 1,
                shift: 16,
                sf: true
            }
        );
    }

    #[test]
    fn parse_mov_large_positive_shifted() {
        assert_eq!(
            parse_inst("mov x0, #0x12340000"),
            Inst::Movz {
                rd: X0,
                imm16: 0x1234,
                shift: 16,
                sf: true
            }
        );
    }

    #[test]
    fn parse_mov_wide_expression() {
        assert_eq!(
            parse_inst("movz x0, #1 + 1, lsl #4 + 12"),
            Inst::Movz {
                rd: X0,
                imm16: 2,
                shift: 16,
                sf: true
            }
        );
    }

    #[test]
    fn parse_immediate_absolute_symbol_after_set() {
        let stmts = parse_stmts(".set ABS1, 7\nmovz x0, #ABS1\n");
        assert_eq!(
            stmts[1],
            Stmt::Instruction(Inst::Movz {
                rd: X0,
                imm16: 7,
                shift: 0,
                sf: true
            })
        );
    }

    #[test]
    fn parse_parenthesized_immediate_after_hash() {
        assert_eq!(
            parse_inst("movz x0, #(1 + 2)"),
            Inst::Movz {
                rd: X0,
                imm16: 3,
                shift: 0,
                sf: true
            }
        );
    }

    #[test]
    fn parse_cbnz_w() {
        assert_eq!(
            parse_inst("cbnz w5, #8"),
            Inst::Cbnz {
                rt: W5,
                offset: 8,
                sf: false
            }
        );
    }

    #[test]
    fn parse_cbz_w() {
        assert_eq!(
            parse_inst("cbz w0, #12"),
            Inst::Cbz {
                rt: W0,
                offset: 12,
                sf: false
            }
        );
    }

    #[test]
    fn parse_memory_offset_expression() {
        assert_eq!(
            parse_inst("ldr x0, [x1, #4 + 4]"),
            Inst::LdrImm64 {
                rt: X0,
                rn: X1,
                offset: 8
            }
        );
    }

    #[test]
    fn parse_all_condition_codes() {
        // Exercise all 14 named condition codes
        for (name, cond) in [
            ("eq", Cond::EQ),
            ("ne", Cond::NE),
            ("cs", Cond::CS),
            ("cc", Cond::CC),
            ("mi", Cond::MI),
            ("pl", Cond::PL),
            ("vs", Cond::VS),
            ("vc", Cond::VC),
            ("hi", Cond::HI),
            ("ls", Cond::LS),
            ("ge", Cond::GE),
            ("lt", Cond::LT),
            ("gt", Cond::GT),
            ("le", Cond::LE),
        ] {
            let src = format!("b.{} #4", name);
            assert_eq!(
                parse_inst(&src),
                Inst::BCond { cond, offset: 4 },
                "failed for b.{}",
                name
            );
        }
    }

    #[test]
    fn parse_hs_lo_aliases() {
        assert_eq!(
            parse_inst("b.hs #4"),
            Inst::BCond {
                cond: Cond::CS,
                offset: 4
            }
        );
        assert_eq!(
            parse_inst("b.lo #4"),
            Inst::BCond {
                cond: Cond::CC,
                offset: 4
            }
        );
    }

    #[test]
    fn parse_b_label() {
        assert_eq!(
            parse_stmts("b done"),
            vec![Stmt::InstructionWithReloc(
                Inst::B { offset: 0 },
                LabelRef {
                    symbol: "done".into(),
                    kind: RelocKind::Branch26,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_b_eq_label() {
        assert_eq!(
            parse_stmts("b.eq done"),
            vec![Stmt::InstructionWithReloc(
                Inst::BCond {
                    cond: Cond::EQ,
                    offset: 0
                },
                LabelRef {
                    symbol: "done".into(),
                    kind: RelocKind::Branch19,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_dotted_symbol_label_and_page_refs() {
        assert_eq!(
            parse_stmts("l_.str:\nadrp x0, l_.str@PAGE\nadd x0, x0, l_.str@PAGEOFF\n"),
            vec![
                Stmt::Label("l_.str".into()),
                Stmt::InstructionWithReloc(
                    Inst::Adrp { rd: X0, imm: 0 },
                    LabelRef {
                        symbol: "l_.str".into(),
                        kind: RelocKind::Page21,
                        addend: 0
                    },
                ),
                Stmt::InstructionWithReloc(
                    Inst::AddImm {
                        rd: X0,
                        rn: X0,
                        imm12: 0,
                        shift: false,
                        sf: true
                    },
                    LabelRef {
                        symbol: "l_.str".into(),
                        kind: RelocKind::PageOff12,
                        addend: 0
                    },
                ),
            ]
        );
    }

    #[test]
    fn parse_adrp_gotpage() {
        assert_eq!(
            parse_stmts("adrp x8, _ext_global@GOTPAGE"),
            vec![Stmt::InstructionWithReloc(
                Inst::Adrp { rd: X8, imm: 0 },
                LabelRef {
                    symbol: "_ext_global".into(),
                    kind: RelocKind::GotLoadPage21,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_ldr_gotpageoff_memory_operand() {
        assert_eq!(
            parse_stmts("ldr x8, [x8, _ext_global@GOTPAGEOFF]"),
            vec![Stmt::InstructionWithReloc(
                Inst::LdrImm64 {
                    rt: X8,
                    rn: X8,
                    offset: 0
                },
                LabelRef {
                    symbol: "_ext_global".into(),
                    kind: RelocKind::GotLoadPageOff12,
                    addend: 0,
                },
            )]
        );
    }

    #[test]
    fn parse_adrp_tlvppage() {
        assert_eq!(
            parse_stmts("adrp x0, _tls_counter@TLVPPAGE"),
            vec![Stmt::InstructionWithReloc(
                Inst::Adrp { rd: X0, imm: 0 },
                LabelRef {
                    symbol: "_tls_counter".into(),
                    kind: RelocKind::TlvpLoadPage21,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_bl_symbol_addend() {
        assert_eq!(
            parse_stmts("bl _puts + 4"),
            vec![Stmt::InstructionWithReloc(
                Inst::Bl { offset: 0 },
                LabelRef {
                    symbol: "_puts".into(),
                    kind: RelocKind::Branch26,
                    addend: 4
                },
            )]
        );
    }

    #[test]
    fn parse_adrp_page_addend() {
        assert_eq!(
            parse_stmts("adrp x0, _data@PAGE + 0x24"),
            vec![Stmt::InstructionWithReloc(
                Inst::Adrp { rd: X0, imm: 0 },
                LabelRef {
                    symbol: "_data".into(),
                    kind: RelocKind::Page21,
                    addend: 0x24
                },
            )]
        );
    }

    #[test]
    fn parse_ldr_tlvppageoff_memory_operand() {
        assert_eq!(
            parse_stmts("ldr x0, [x0, _tls_counter@TLVPPAGEOFF]"),
            vec![Stmt::InstructionWithReloc(
                Inst::LdrImm64 {
                    rt: X0,
                    rn: X0,
                    offset: 0
                },
                LabelRef {
                    symbol: "_tls_counter".into(),
                    kind: RelocKind::TlvpLoadPageOff12,
                    addend: 0,
                },
            )]
        );
    }

    #[test]
    fn parse_ldr_pageoff_addend_memory_operand() {
        assert_eq!(
            parse_stmts("ldr x0, [x0, _data@PAGEOFF + 0x24]"),
            vec![Stmt::InstructionWithReloc(
                Inst::LdrImm64 {
                    rt: X0,
                    rn: X0,
                    offset: 0
                },
                LabelRef {
                    symbol: "_data".into(),
                    kind: RelocKind::PageOff12,
                    addend: 0x24
                },
            )]
        );
    }

    #[test]
    fn parse_ldr_pageoff_memory_operand() {
        assert_eq!(
            parse_stmts("ldr x0, [x1, value@PAGEOFF]"),
            vec![Stmt::InstructionWithReloc(
                Inst::LdrImm64 {
                    rt: X0,
                    rn: X1,
                    offset: 0
                },
                LabelRef {
                    symbol: "value".into(),
                    kind: RelocKind::PageOff12,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_adrp_rejects_unsupported_modifier() {
        let err = parse_err("adrp x0, _foo@GOTPAGEOFF");
        assert!(
            err.contains("unsupported relocation modifier"),
            "got: {}",
            err
        );
    }

    #[test]
    fn parse_b_named_local_label() {
        assert_eq!(
            parse_stmts("b .Ldone"),
            vec![Stmt::InstructionWithReloc(
                Inst::B { offset: 0 },
                LabelRef {
                    symbol: ".Ldone".into(),
                    kind: RelocKind::Branch26,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_cbz_label() {
        assert_eq!(
            parse_stmts("cbz x0, done"),
            vec![Stmt::InstructionWithReloc(
                Inst::Cbz {
                    rt: X0,
                    offset: 0,
                    sf: true
                },
                LabelRef {
                    symbol: "done".into(),
                    kind: RelocKind::Branch19,
                    addend: 0
                },
            )]
        );
    }

    #[test]
    fn parse_numeric_local_label_definition_and_backward_branch() {
        assert_eq!(
            parse_stmts("1:\nb 1b\n"),
            vec![
                Stmt::Label(".Ltmp$1$1".into()),
                Stmt::InstructionWithReloc(
                    Inst::B { offset: 0 },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::Branch26,
                        addend: 0
                    },
                ),
            ]
        );
    }

    #[test]
    fn parse_numeric_local_forward_reference() {
        assert_eq!(
            parse_stmts("b 2f\n2:\n"),
            vec![
                Stmt::InstructionWithReloc(
                    Inst::B { offset: 0 },
                    LabelRef {
                        symbol: ".Ltmp$2$1".into(),
                        kind: RelocKind::Branch26,
                        addend: 0
                    },
                ),
                Stmt::Label(".Ltmp$2$1".into()),
            ]
        );
    }

    #[test]
    fn parse_numeric_local_label_on_same_line() {
        assert_eq!(
            parse_stmts("1: cbz x0, 1b"),
            vec![
                Stmt::Label(".Ltmp$1$1".into()),
                Stmt::InstructionWithReloc(
                    Inst::Cbz {
                        rt: X0,
                        offset: 0,
                        sf: true
                    },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::Branch19,
                        addend: 0
                    },
                ),
            ]
        );
    }

    #[test]
    fn parse_numeric_local_in_expression() {
        assert_eq!(
            parse_stmts("1:\n.quad 1b + 4\n"),
            vec![
                Stmt::Label(".Ltmp$1$1".into()),
                Stmt::Directive(Directive::Quad(vec![Expr::Add(
                    Box::new(Expr::Symbol(".Ltmp$1$1".into())),
                    Box::new(Expr::Int(4)),
                )])),
            ]
        );
    }

    #[test]
    fn parse_numeric_local_adrp_and_add_operands() {
        assert_eq!(
            parse_stmts("adrp x0, 1f@PAGE\nadd x0, x0, 1f@PAGEOFF\n1:\n"),
            vec![
                Stmt::InstructionWithReloc(
                    Inst::Adrp { rd: X0, imm: 0 },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::Page21,
                        addend: 0
                    },
                ),
                Stmt::InstructionWithReloc(
                    Inst::AddImm {
                        rd: X0,
                        rn: X0,
                        imm12: 0,
                        shift: false,
                        sf: true
                    },
                    LabelRef {
                        symbol: ".Ltmp$1$1".into(),
                        kind: RelocKind::PageOff12,
                        addend: 0
                    },
                ),
                Stmt::Label(".Ltmp$1$1".into()),
            ]
        );
    }

    #[test]
    fn parse_numeric_local_requires_previous_definition_for_backward_ref() {
        let err = parse_err("b 1b\n");
        assert!(
            err.contains("numeric label '1' has no previous definition"),
            "got: {}",
            err
        );
    }
}
