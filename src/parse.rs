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
    let mut p = Parser::new(&tokens);
    p.parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    absolute_symbols: BTreeMap<String, i64>,
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

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self {
            tokens,
            pos: 0,
            absolute_symbols: BTreeMap::new(),
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
                let size = self.parse_const_expr("common size expression")? as u64;
                let align_pow2 = if self.eat(&Tok::Comma) {
                    self.parse_const_expr("common alignment expression")? as u8
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
                let expr = self.parse_expr()?;
                if let Ok(value) = expr::eval_with_symbols(&expr, &self.absolute_symbols) {
                    self.absolute_symbols.insert(sym.clone(), value);
                } else {
                    self.absolute_symbols.remove(&sym);
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
            ".byte" => Directive::Byte(self.parse_expr_list()?),
            ".short" => Directive::Short(self.parse_expr_list()?),
            ".word" | ".long" => Directive::Word(self.parse_expr_list()?),
            ".quad" => Directive::Quad(self.parse_expr_list()?),
            ".ascii" => {
                if let Tok::StringLit(s) = self.peek().clone() {
                    self.advance();
                    Directive::Ascii(s.into_bytes())
                } else {
                    return Err(self.err("expected string after .ascii".into()));
                }
            }
            ".asciz" | ".string" => {
                if let Tok::StringLit(s) = self.peek().clone() {
                    self.advance();
                    let mut bytes = s.into_bytes();
                    bytes.push(0); // null terminator
                    Directive::Asciz(bytes)
                } else {
                    return Err(self.err("expected string after .asciz".into()));
                }
            }
            ".space" | ".skip" => {
                let n = self.parse_const_expr("space expression")? as u64;
                Directive::Space(n)
            }
            ".zero" => {
                let n = self.parse_const_expr("zero expression")? as u64;
                Directive::Space(n)
            }
            ".fill" => {
                let repeat = self.parse_const_expr("fill repeat expression")? as u64;
                self.expect(&Tok::Comma)?;
                let size = self.parse_const_expr("fill size expression")? as u8;
                let value = if self.eat(&Tok::Comma) {
                    self.parse_const_expr("fill value expression")? as u64
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
                let size = self.parse_const_expr("zerofill size expression")? as u64;
                let align_pow2 = if self.eat(&Tok::Comma) {
                    self.parse_const_expr("zerofill alignment expression")? as u32
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
                // Skip any additional section attributes (e.g., regular,pure_instructions)
                while self.eat(&Tok::Comma) {
                    while !self.at_end_of_stmt() && self.peek() != &Tok::Comma {
                        self.advance();
                    }
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
                    let keyword = self.expect_ident()?;
                    if !keyword.eq_ignore_ascii_case("sdk_version") {
                        return Err(self.err(format!(
                            "expected sdk_version after .build_version, got {}",
                            keyword
                        )));
                    }
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
                let mut labels = Vec::new();
                if !self.at_end_of_stmt() {
                    labels.push(self.expect_ident()?);
                    while !self.at_end_of_stmt() {
                        self.expect(&Tok::Comma)?;
                        labels.push(self.expect_ident()?);
                    }
                }
                if let Some(expected) = linker_optimization_hint_label_count(&kind) {
                    if labels.len() != expected {
                        return Err(self.err(format!(
                            ".loh {} expects {} label{}, got {}",
                            kind,
                            expected,
                            if expected == 1 { "" } else { "s" },
                            labels.len()
                        )));
                    }
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

    fn parse_expr_list(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut vals = vec![self.parse_expr()?];
        while self.eat(&Tok::Comma) {
            vals.push(self.parse_expr()?);
        }
        Ok(vals)
    }

    fn parse_alignment_directive_args(
        &mut self,
    ) -> Result<(u32, Option<u8>, Option<u64>), ParseError> {
        let power = self.parse_const_expr("alignment expression")? as u32;
        let mut fill = None;
        let mut max_skip = None;

        if self.eat(&Tok::Comma) {
            if !self.at_end_of_stmt() && self.peek() != &Tok::Comma {
                fill = Some(self.parse_const_expr("alignment fill expression")? as u8);
            }
            if self.eat(&Tok::Comma) {
                max_skip = Some(self.parse_const_expr("alignment max-skip expression")? as u64);
            }
        }

        Ok((power, fill, max_skip))
    }

    fn parse_cfi_register(&mut self) -> Result<GpReg, ParseError> {
        let (register, _, kind) = self.parse_gp_reg_with_size_kind()?;
        if matches!(kind, GpRegKind::Zr) {
            return Err(self.err("CFI directives do not accept the zero register".into()));
        }
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

    fn starts_const_expr(&self) -> bool {
        if self.numeric_label_ref_at(self.pos).is_some() {
            return false;
        }
        matches!(self.peek(), Tok::Integer(_) | Tok::Minus | Tok::LParen)
    }

    fn starts_immediate_expr(&self) -> bool {
        self.peek() == &Tok::Hash || self.starts_const_expr()
    }

    fn parse_immediate_const_expr(&mut self, context: &str) -> Result<i64, ParseError> {
        self.eat(&Tok::Hash);
        self.parse_const_expr(context)
    }

    fn parse_logical_immediate_value(&mut self, sf: bool) -> Result<u64, ParseError> {
        let imm = self.parse_immediate_const_expr("logical immediate")?;
        let raw = if sf {
            imm as u64
        } else {
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
        self.parse_add_sub_expr()
    }

    fn parse_add_sub_expr(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_unary_expr()?;
        loop {
            if self.eat(&Tok::Plus) {
                let rhs = self.parse_unary_expr()?;
                expr = Expr::Add(Box::new(expr), Box::new(rhs));
            } else if self.eat(&Tok::Minus) {
                let rhs = self.parse_unary_expr()?;
                expr = Expr::Sub(Box::new(expr), Box::new(rhs));
            } else {
                break;
            }
        }
        Ok(expr)
    }

    fn parse_unary_expr(&mut self) -> Result<Expr, ParseError> {
        if self.eat(&Tok::Minus) {
            Ok(Expr::UnaryMinus(Box::new(self.parse_unary_expr()?)))
        } else {
            self.parse_primary_expr()
        }
    }

    fn parse_primary_expr(&mut self) -> Result<Expr, ParseError> {
        if let Some(symbol) = self.parse_numeric_label_ref()? {
            return Ok(Expr::Symbol(symbol));
        }
        match self.peek().clone() {
            Tok::Integer(value) => {
                self.advance();
                Ok(Expr::Int(value))
            }
            Tok::Ident(symbol) => {
                self.advance();
                if self.eat(&Tok::At) {
                    let modifier = self.expect_ident()?;
                    let upper = modifier.to_ascii_uppercase();
                    match upper.as_str() {
                        "GOT" => Ok(Expr::ModifiedSymbol {
                            symbol,
                            modifier: SymbolModifier::Got,
                        }),
                        _ => Err(self.err(format!(
                            "unsupported relocation modifier '@{}' in expression",
                            modifier
                        ))),
                    }
                } else {
                    Ok(Expr::Symbol(symbol))
                }
            }
            Tok::Dot => {
                self.advance();
                Ok(Expr::CurrentLocation)
            }
            Tok::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
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
        match self.peek() {
            Tok::Ident(name) => !looks_like_gp_register_name(name),
            _ => false,
        }
    }

    fn starts_non_register_literal_reference(&self) -> bool {
        if self.numeric_label_ref_at(self.pos).is_some() {
            return true;
        }
        match self.peek() {
            Tok::Ident(name) => {
                !looks_like_gp_register_name(name) && !looks_like_fp_register_name(name)
            }
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
        if mnemonic == "ldapr" {
            return self.parse_ldapr();
        }
        if mnemonic == "stlr" {
            return self.parse_stlr();
        }
        if mnemonic == "ldaddal" {
            return self.parse_ldaddal();
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
            "orr" => self.parse_logic("orr"),
            "eor" => self.parse_logic("eor"),
            "ands" => self.parse_logic("ands"),
            "neg" => self.parse_neg(),
            "mvn" => self.parse_mvn(),
            "tst" => self.parse_tst(),

            // Move
            "csel" => self.parse_csel(),
            "csinc" => self.parse_csinc(),
            "csinv" => self.parse_csinv(),
            "csneg" => self.parse_csneg(),
            "cset" => self.parse_cset(),
            "csetm" => self.parse_csetm(),
            "cinc" => self.parse_cinc(),
            "cinv" => self.parse_cinv(),
            "cneg" => self.parse_cneg(),
            "mov" => self.parse_mov(),
            "movz" => self.parse_mov_wide("movz"),
            "movk" => self.parse_mov_wide("movk"),
            "movn" => self.parse_mov_wide("movn"),

            // Shifts
            "lsl" => self.parse_shift("lsl"),
            "lsr" => self.parse_shift("lsr"),
            "asr" => self.parse_shift("asr"),
            "ubfiz" => self.parse_bitfield_alias("ubfiz"),
            "bfi" => self.parse_bitfield_alias("bfi"),
            "bfxil" => self.parse_bitfield_alias("bfxil"),

            // Branches
            "ret" => self.parse_ret(),
            "br" => {
                let rn = self.parse_gp_reg()?;
                Ok(Inst::Br { rn })
            }
            "blr" => {
                let rn = self.parse_gp_reg()?;
                Ok(Inst::Blr { rn })
            }

            // Load/store
            "ldrb" => self.parse_ldrb_h("ldrb"),
            "ldrh" => self.parse_ldrb_h("ldrh"),
            "stp" => self.parse_ldp_stp(false),
            "ldp" => self.parse_ldp_stp(true),

            // FP arithmetic (double)
            "fadd" => self.parse_fp_arith("fadd"),
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
            "fcvtzs" => self.parse_fcvtzs(),
            "scvtf" => self.parse_scvtf(),
            "fmov" => self.parse_fmov(),

            // System
            "svc" => {
                let imm = self.parse_immediate_const_expr("svc immediate")? as u16;
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
                let imm = self.parse_immediate_const_expr("brk immediate")? as u16;
                Ok(Inst::Brk { imm16: imm })
            }

            _ => Err(self.err(format!("unknown mnemonic: {}", mnemonic))),
        }?;
        Ok(Stmt::Instruction(inst))
    }

    // ---- Register parsing helpers ----

    fn parse_gp_reg(&mut self) -> Result<GpReg, ParseError> {
        let name = self.expect_ident()?;
        parse_gp_reg_name(&name)
            .ok_or_else(|| self.err(format!("expected GP register, got '{}'", name)))
    }

    /// Returns (register, is_64bit).
    fn parse_gp_reg_with_size(&mut self) -> Result<(GpReg, bool), ParseError> {
        let (reg, is_64bit, _kind) = self.parse_gp_reg_with_size_kind()?;
        Ok((reg, is_64bit))
    }

    fn parse_gp_reg_with_size_kind(&mut self) -> Result<(GpReg, bool, GpRegKind), ParseError> {
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        if lower == "sp" || lower == "xzr" {
            let kind = if lower == "sp" {
                GpRegKind::Sp
            } else {
                GpRegKind::Zr
            };
            return Ok((parse_gp_reg_name(&lower).unwrap(), true, kind));
        }
        if lower == "wzr" {
            return Ok((WZR, false, GpRegKind::Zr));
        }
        if lower.starts_with('x') {
            let reg = parse_gp_reg_name(&lower)
                .ok_or_else(|| self.err(format!("bad register '{}'", name)))?;
            Ok((reg, true, GpRegKind::Reg))
        } else if lower.starts_with('w') {
            let reg = parse_gp_reg_name(&lower)
                .ok_or_else(|| self.err(format!("bad register '{}'", name)))?;
            Ok((reg, false, GpRegKind::Reg))
        } else {
            Err(self.err(format!("expected GP register, got '{}'", name)))
        }
    }

    fn parse_fp_reg_with_size(&mut self) -> Result<(FpReg, bool), ParseError> {
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        if lower.starts_with('d') {
            let reg = parse_fp_reg_name(&lower)
                .ok_or_else(|| self.err(format!("bad FP register '{}'", name)))?;
            Ok((reg, true)) // double
        } else if lower.starts_with('s') {
            let reg = parse_fp_reg_name(&lower)
                .ok_or_else(|| self.err(format!("bad FP register '{}'", name)))?;
            Ok((reg, false)) // single
        } else {
            Err(self.err(format!("expected FP register, got '{}'", name)))
        }
    }

    fn parse_atomic_data_reg(&mut self, context: &str) -> Result<(GpReg, bool), ParseError> {
        let (reg, sf, kind) = self.parse_gp_reg_with_size_kind()?;
        if kind == GpRegKind::Sp {
            return Err(self.err(format!(
                "{} does not allow SP as a data register",
                context
            )));
        }
        Ok((reg, sf))
    }

    fn parse_atomic_base_reg(&mut self, context: &str) -> Result<GpReg, ParseError> {
        self.expect(&Tok::LBracket)?;
        let (rn, sf, kind) = self.parse_gp_reg_with_size_kind()?;
        if !sf || kind == GpRegKind::Zr {
            return Err(self.err(format!(
                "{} expects an [Xn] or [sp] base register",
                context
            )));
        }
        if !self.eat(&Tok::RBracket) {
            return Err(self.err(format!(
                "{} expects a simple [Xn] memory operand",
                context
            )));
        }
        if self.eat(&Tok::Bang) {
            return Err(self.err(format!(
                "{} does not support pre-index addressing",
                context
            )));
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

    fn parse_ldaddal(&mut self) -> Result<Stmt, ParseError> {
        let (rs, sf) = self.parse_atomic_data_reg("ldaddal")?;
        self.expect(&Tok::Comma)?;
        let (rt, rt_sf) = self.parse_atomic_data_reg("ldaddal")?;
        if sf != rt_sf {
            return Err(self.err(
                "ldaddal requires source and destination registers of the same width".into(),
            ));
        }
        self.expect(&Tok::Comma)?;
        let rn = self.parse_atomic_base_reg("ldaddal")?;
        Ok(Stmt::Instruction(if sf {
            Inst::Ldaddal64 { rs, rt, rn }
        } else {
            Inst::Ldaddal32 { rs, rt, rn }
        }))
    }

    fn parse_add_sub(&mut self, is_sub: bool, sets_flags: bool) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _, rn_kind) = self.parse_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;

        if self.starts_immediate_expr() {
            let imm = self.parse_immediate_const_expr("add/sub immediate")? as u16;
            let shift = self.parse_optional_lsl12()?;
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
            let (rm, rm_is_64bit) = self.parse_gp_reg_with_size()?;
            let modifier = self.parse_optional_add_sub_modifier(sf, rm_is_64bit)?;
            self.validate_add_sub_extended_base_reg(rn_kind, modifier)?;
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

    /// ADD that can also handle label@PAGEOFF references (for ADRP+ADD pairs).
    fn parse_add_sub_stmt(&mut self, is_sub: bool, sets_flags: bool) -> Result<Stmt, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _, rn_kind) = self.parse_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;

        // Check for label@PAGEOFF (identifier or numeric local reference followed by @).
        if self.starts_non_register_symbol_reference() {
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
        let inst = self.parse_add_sub_operand(rd, rn, rn_kind, sf, is_sub, sets_flags)?;
        Ok(Stmt::Instruction(inst))
    }

    /// Parse the third operand of add/sub (immediate or register).
    fn parse_add_sub_operand(
        &mut self,
        rd: GpReg,
        rn: GpReg,
        rn_kind: GpRegKind,
        sf: bool,
        is_sub: bool,
        sets_flags: bool,
    ) -> Result<Inst, ParseError> {
        if self.starts_immediate_expr() {
            let imm = self.parse_immediate_const_expr("add/sub immediate")? as u16;
            let shift = self.parse_optional_lsl12()?;
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
            let (rm, rm_is_64bit) = self.parse_gp_reg_with_size()?;
            let modifier = self.parse_optional_add_sub_modifier(sf, rm_is_64bit)?;
            self.validate_add_sub_extended_base_reg(rn_kind, modifier)?;
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
        let (rn, sf, rn_kind) = self.parse_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let imm = self.parse_immediate_const_expr("cmp immediate")? as u16;
            let shift = self.parse_optional_lsl12()?;
            Ok(Inst::SubsImm {
                rd: XZR,
                rn,
                imm12: imm,
                shift,
                sf,
            })
        } else {
            let (rm, rm_is_64bit) = self.parse_gp_reg_with_size()?;
            let modifier = self.parse_optional_add_sub_modifier(sf, rm_is_64bit)?;
            self.validate_add_sub_extended_base_reg(rn_kind, modifier)?;
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
        let (rn, sf, rn_kind) = self.parse_gp_reg_with_size_kind()?;
        self.expect(&Tok::Comma)?;
        let (rm, rm_is_64bit) = self.parse_gp_reg_with_size()?;
        let modifier = self.parse_optional_add_sub_modifier(sf, rm_is_64bit)?;
        self.validate_add_sub_extended_base_reg(rn_kind, modifier)?;
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
        let (rn, sf) = self.parse_gp_reg_with_size()?;
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
            let (rm, _) = self.parse_gp_reg_with_size()?;
            Ok(Inst::AndsReg {
                rd: XZR,
                rn,
                rm,
                sf,
            })
        }
    }

    fn parse_neg(&mut self) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, rm_is_64bit) = self.parse_gp_reg_with_size()?;
        let modifier = self.parse_optional_add_sub_modifier(sf, rm_is_64bit)?;
        self.validate_add_sub_extended_base_reg(GpRegKind::Zr, modifier)?;
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
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_gp_reg_with_size()?;
        Ok(Inst::OrnReg {
            rd,
            rn: XZR,
            rm,
            sf,
        })
    }

    fn parse_cond_select(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_gp_reg_with_size()?;
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
        let (rd, sf) = self.parse_gp_reg_with_size()?;
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
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
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
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_gp_reg_with_size()?;
        Ok(match mnemonic {
            "mul" => Inst::Mul { rd, rn, rm, sf },
            "sdiv" => Inst::Sdiv { rd, rn, rm, sf },
            "udiv" => Inst::Udiv { rd, rn, rm, sf },
            _ => unreachable!(),
        })
    }

    fn parse_madd_sub(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (ra, _) = self.parse_gp_reg_with_size()?;
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
        let (rd, rd_is_64bit) = self.parse_gp_reg_with_size()?;
        if !rd_is_64bit {
            return Err(self.err("umull destination must be an X register".into()));
        }
        self.expect(&Tok::Comma)?;
        let (rn, rn_is_64bit) = self.parse_gp_reg_with_size()?;
        if rn_is_64bit {
            return Err(self.err("umull sources must be W registers".into()));
        }
        self.expect(&Tok::Comma)?;
        let (rm, rm_is_64bit) = self.parse_gp_reg_with_size()?;
        if rm_is_64bit {
            return Err(self.err("umull sources must be W registers".into()));
        }
        Ok(Inst::Umull { rd, rn, rm })
    }

    fn parse_logic(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
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
            let (rm, _) = self.parse_gp_reg_with_size()?;
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
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
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
            let (rm, _) = self.parse_gp_reg_with_size()?;
            if rm == SP || rd == SP {
                // MOV involving SP → ADD Xd, Xn, #0 (SP can't be used in ORR shifted reg)
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

    fn parse_mov_wide(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let imm = self.parse_immediate_const_expr("mov wide immediate")? as u16;
        let shift = self.parse_optional_lsl_amount()?;
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
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let amount = self.parse_immediate_const_expr("shift amount")? as u8;
        Ok(match mnemonic {
            "lsl" => Inst::LslImm { rd, rn, amount, sf },
            "lsr" => Inst::LsrImm { rd, rn, amount, sf },
            "asr" => Inst::AsrImm { rd, rn, amount, sf },
            _ => unreachable!(),
        })
    }

    fn parse_bitfield_alias(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let lsb = self.parse_immediate_const_expr("bitfield lsb")? as u8;
        self.expect(&Tok::Comma)?;
        let width = self.parse_immediate_const_expr("bitfield width")? as u8;
        self.validate_bitfield_alias_args(mnemonic, sf, lsb, width)?;
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
            let offset = self.parse_immediate_const_expr("branch offset")? as i32;
            Ok(Stmt::Instruction(Inst::B { offset }))
        } else {
            let label = self.parse_label_reference()?;
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
            let offset = self.parse_immediate_const_expr("branch offset")? as i32;
            Ok(Stmt::Instruction(Inst::Bl { offset }))
        } else {
            let label = self.parse_label_reference()?;
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
            let offset = self.parse_immediate_const_expr("branch offset")? as i32;
            Ok(Stmt::Instruction(Inst::BCond { cond, offset }))
        } else {
            let label = self.parse_label_reference()?;
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
        let (rt, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let offset = self.parse_immediate_const_expr("cbz/cbnz offset")? as i32;
            let inst = if is_nz {
                Inst::Cbnz { rt, offset, sf }
            } else {
                Inst::Cbz { rt, offset, sf }
            };
            Ok(Stmt::Instruction(inst))
        } else {
            let label = self.parse_label_reference()?;
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
        let (rt, sf) = self.parse_gp_reg_with_size()?;
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
            let offset = self.parse_immediate_const_expr("tbz/tbnz offset")? as i32;
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
            let label = self.parse_label_reference()?;
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
            let rn = self.parse_gp_reg()?;
            Ok(Inst::Ret { rn })
        }
    }

    fn parse_adr(&mut self) -> Result<Stmt, ParseError> {
        let rd = self.parse_gp_reg()?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let imm = self.parse_immediate_const_expr("adr immediate")? as i32;
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
        let rd = self.parse_gp_reg()?;
        self.expect(&Tok::Comma)?;
        if self.starts_immediate_expr() {
            let imm = self.parse_immediate_const_expr("adrp immediate")? as i32;
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
        offset: i16,
        force_unscaled: bool,
    ) -> Inst {
        if force_unscaled || offset < 0 {
            match (is_load, sf) {
                (true, true) => Inst::Ldur64 { rt, rn, offset },
                (false, true) => Inst::Stur64 { rt, rn, offset },
                (true, false) => Inst::Ldur32 { rt, rn, offset },
                (false, false) => Inst::Stur32 { rt, rn, offset },
            }
        } else {
            match (is_load, sf) {
                (true, true) => Inst::LdrImm64 {
                    rt,
                    rn,
                    offset: offset as u16,
                },
                (false, true) => Inst::StrImm64 {
                    rt,
                    rn,
                    offset: offset as u16,
                },
                (true, false) => Inst::LdrImm32 {
                    rt,
                    rn,
                    offset: offset as u16,
                },
                (false, false) => Inst::StrImm32 {
                    rt,
                    rn,
                    offset: offset as u16,
                },
            }
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

    fn parse_ldur_stur(&mut self, is_load: bool) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        let offset = if self.eat(&Tok::RBracket) {
            0
        } else {
            self.expect(&Tok::Comma)?;
            let offset = self.parse_immediate_const_expr("memory offset")? as i16;
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
            self.gp_mem_offset_inst(is_load, sf, rt, rn, offset, true),
        ))
    }

    fn parse_ldr_str_gp(&mut self, is_load: bool) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;

        if is_load && self.starts_immediate_expr() {
            let offset = self.parse_immediate_const_expr("ldr literal offset")? as i32;
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
        let (rn, _) = self.parse_gp_reg_with_size()?;

        if self.eat(&Tok::RBracket) {
            // [Xn] or [Xn], #off (post-index)
            if self.eat(&Tok::Comma) {
                let offset = self.parse_immediate_const_expr("post-index offset")? as i16;
                return Ok(Stmt::Instruction(
                    self.gp_mem_post_inst(is_load, sf, rt, rn, offset),
                ));
            }
            return Ok(Stmt::Instruction(
                self.gp_mem_offset_inst(is_load, sf, rt, rn, 0, false),
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

        let offset = self.parse_immediate_const_expr("memory offset")? as i16;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            return Ok(Stmt::Instruction(
                self.gp_mem_pre_inst(is_load, sf, rt, rn, offset),
            ));
        }

        Ok(Stmt::Instruction(
            self.gp_mem_offset_inst(is_load, sf, rt, rn, offset, false),
        ))
    }

    fn parse_ldr_str_fp(&mut self, is_load: bool) -> Result<Stmt, ParseError> {
        let (rt, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;

        if is_load && self.starts_immediate_expr() {
            let offset = self.parse_immediate_const_expr("ldr literal offset")? as i32;
            let inst = if is_double {
                Inst::LdrFpLit64 { rt, offset }
            } else {
                Inst::LdrFpLit32 { rt, offset }
            };
            return Ok(Stmt::Instruction(inst));
        }

        if is_load && self.starts_non_register_literal_reference() {
            let label = self.parse_label_reference()?;
            let addend = self.parse_optional_symbol_addend()?;
            let inst = if is_double {
                Inst::LdrFpLit64 { rt, offset: 0 }
            } else {
                Inst::LdrFpLit32 { rt, offset: 0 }
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
        let (rn, _) = self.parse_gp_reg_with_size()?;

        if self.eat(&Tok::RBracket) {
            if self.eat(&Tok::Comma) {
                let offset = self.parse_immediate_const_expr("post-index offset")? as i16;
                let inst = match (is_load, is_double) {
                    (true, true) => Inst::LdrFpPost64 { rt, rn, offset },
                    (false, true) => Inst::StrFpPost64 { rt, rn, offset },
                    (true, false) => Inst::LdrFpPost32 { rt, rn, offset },
                    (false, false) => Inst::StrFpPost32 { rt, rn, offset },
                };
                return Ok(Stmt::Instruction(inst));
            }
            let inst = match (is_load, is_double) {
                (true, true) => Inst::LdrFpImm64 { rt, rn, offset: 0 },
                (false, true) => Inst::StrFpImm64 { rt, rn, offset: 0 },
                (true, false) => Inst::LdrFpImm32 { rt, rn, offset: 0 },
                (false, false) => Inst::StrFpImm32 { rt, rn, offset: 0 },
            };
            return Ok(Stmt::Instruction(inst));
        }

        self.expect(&Tok::Comma)?;
        if self.starts_register_like_operand() {
            let (rm, extend, shift) =
                self.parse_reg_offset_operand(if is_double { 3 } else { 2 })?;
            self.expect(&Tok::RBracket)?;
            let inst = match (is_load, is_double) {
                (true, true) => Inst::LdrFpReg64 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
                (false, true) => Inst::StrFpReg64 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
                (true, false) => Inst::LdrFpReg32 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
                (false, false) => Inst::StrFpReg32 {
                    rt,
                    rn,
                    rm,
                    extend,
                    shift,
                },
            };
            return Ok(Stmt::Instruction(inst));
        }

        let offset = self.parse_immediate_const_expr("memory offset")?;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            let inst = match (is_load, is_double) {
                (true, true) => Inst::LdrFpPre64 {
                    rt,
                    rn,
                    offset: offset as i16,
                },
                (false, true) => Inst::StrFpPre64 {
                    rt,
                    rn,
                    offset: offset as i16,
                },
                (true, false) => Inst::LdrFpPre32 {
                    rt,
                    rn,
                    offset: offset as i16,
                },
                (false, false) => Inst::StrFpPre32 {
                    rt,
                    rn,
                    offset: offset as i16,
                },
            };
            return Ok(Stmt::Instruction(inst));
        }

        let inst = match (is_load, is_double) {
            (true, true) => Inst::LdrFpImm64 {
                rt,
                rn,
                offset: offset as u16,
            },
            (false, true) => Inst::StrFpImm64 {
                rt,
                rn,
                offset: offset as u16,
            },
            (true, false) => Inst::LdrFpImm32 {
                rt,
                rn,
                offset: offset as u16,
            },
            (false, false) => Inst::StrFpImm32 {
                rt,
                rn,
                offset: offset as u16,
            },
        };
        Ok(Stmt::Instruction(inst))
    }

    fn parse_ldrb_h(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rt = self.parse_gp_reg()?;
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let rn = self.parse_gp_reg()?;
        let offset = if self.eat(&Tok::Comma) {
            if self.starts_register_like_operand() {
                let scale = match mnemonic {
                    "ldrb" => 0,
                    "ldrh" => 1,
                    _ => unreachable!(),
                };
                let (rm, extend, shift) = self.parse_reg_offset_operand(scale)?;
                self.expect(&Tok::RBracket)?;
                return Ok(match mnemonic {
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
                    _ => unreachable!(),
                });
            }
            self.parse_immediate_const_expr("memory offset")?
        } else {
            0
        };
        self.expect(&Tok::RBracket)?;
        Ok(match mnemonic {
            "ldrb" => Inst::Ldrb {
                rt,
                rn,
                offset: offset as u16,
            },
            "ldrh" => Inst::Ldrh {
                rt,
                rn,
                offset: offset as u16,
            },
            _ => unreachable!(),
        })
    }

    fn parse_ldrsw(&mut self) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_gp_reg_with_size()?;
        if !sf {
            return Err(self.err("ldrsw destination must be an x-register".into()));
        }
        self.expect(&Tok::Comma)?;

        if self.starts_immediate_expr() {
            let offset = self.parse_immediate_const_expr("ldrsw literal offset")? as i32;
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
        let rn = self.parse_gp_reg()?;
        let offset = if self.eat(&Tok::Comma) {
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
            self.parse_immediate_const_expr("memory offset")?
        } else {
            0
        };
        self.expect(&Tok::RBracket)?;
        Ok(Stmt::Instruction(Inst::Ldrsw {
            rt,
            rn,
            offset: offset as u16,
        }))
    }

    fn parse_ldp_stp(&mut self, is_load: bool) -> Result<Inst, ParseError> {
        if self.starts_fp_register_like_operand() {
            return self.parse_ldp_stp_fp(is_load);
        }
        self.parse_ldp_stp_gp(is_load)
    }

    fn parse_ldp_stp_gp(&mut self, is_load: bool) -> Result<Inst, ParseError> {
        let (rt1, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rt2, second_sf) = self.parse_gp_reg_with_size()?;
        if sf != second_sf {
            return Err(self.err("ldp/stp register pair must use matching register widths".into()));
        }
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;

        if self.eat(&Tok::RBracket) {
            if self.eat(&Tok::Comma) {
                let offset = self.parse_immediate_const_expr("pair post-index offset")? as i16;
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
        let offset = self.parse_immediate_const_expr("pair offset")? as i16;
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
        let (rt1, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rt2, second_is_double) = self.parse_fp_reg_with_size()?;
        if is_double != second_is_double {
            return Err(
                self.err("ldp/stp FP register pair must use matching register widths".into())
            );
        }
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;

        if self.eat(&Tok::RBracket) {
            if self.eat(&Tok::Comma) {
                let offset = self.parse_immediate_const_expr("pair post-index offset")? as i16;
                return Ok(match (is_load, is_double) {
                    (true, true) => Inst::LdpFpPost64 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (false, true) => Inst::StpFpPost64 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (true, false) => Inst::LdpFpPost32 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                    (false, false) => Inst::StpFpPost32 {
                        rt1,
                        rt2,
                        rn,
                        offset,
                    },
                });
            }

            return Ok(match (is_load, is_double) {
                (true, true) => Inst::LdpFpOff64 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (false, true) => Inst::StpFpOff64 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (true, false) => Inst::LdpFpOff32 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
                (false, false) => Inst::StpFpOff32 {
                    rt1,
                    rt2,
                    rn,
                    offset: 0,
                },
            });
        }

        self.expect(&Tok::Comma)?;
        let offset = self.parse_immediate_const_expr("pair offset")? as i16;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            return Ok(match (is_load, is_double) {
                (true, true) => Inst::LdpFpPre64 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (false, true) => Inst::StpFpPre64 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (true, false) => Inst::LdpFpPre32 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
                (false, false) => Inst::StpFpPre32 {
                    rt1,
                    rt2,
                    rn,
                    offset,
                },
            });
        }

        Ok(match (is_load, is_double) {
            (true, true) => Inst::LdpFpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (false, true) => Inst::StpFpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (true, false) => Inst::LdpFpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            },
            (false, false) => Inst::StpFpOff32 {
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
        let (rn, _) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_fp_reg_with_size()?;
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

    fn parse_fp_unary(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_fp_reg_with_size()?;
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
        let (rm, _) = self.parse_fp_reg_with_size()?;
        if is_double {
            Ok(Inst::FcmpD { rn, rm })
        } else {
            Ok(Inst::FcmpS { rn, rm })
        }
    }

    fn parse_fcsel(&mut self) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_fp_reg_with_size()?;
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
        let (rn, _) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (ra, _) = self.parse_fp_reg_with_size()?;
        if is_double {
            Ok(Inst::FmaddD { rd, rn, rm, ra })
        } else {
            Ok(Inst::FmaddS { rd, rn, rm, ra })
        }
    }

    fn parse_fcvtzs(&mut self) -> Result<Inst, ParseError> {
        let rd = self.parse_gp_reg()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_fp_reg_with_size()?;
        Ok(Inst::FcvtzsD { rd, rn })
    }

    fn parse_scvtf(&mut self) -> Result<Inst, ParseError> {
        let (rd, _) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let rn = self.parse_gp_reg()?;
        Ok(Inst::ScvtfD { rd, rn })
    }

    fn parse_fmov(&mut self) -> Result<Inst, ParseError> {
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        self.expect(&Tok::Comma)?;

        if lower.starts_with('d') || lower.starts_with('s') {
            let rd = parse_fp_reg_name(&lower)
                .ok_or_else(|| self.err(format!("bad FP reg '{}'", name)))?;
            let is_double = lower.starts_with('d');
            match self.peek() {
                Tok::Integer(_) | Tok::Float(_) => {
                    let imm8 = self.parse_fp_modified_immediate(is_double)?;
                    if is_double {
                        Ok(Inst::FmovImmD { rd, imm8 })
                    } else {
                        Ok(Inst::FmovImmS { rd, imm8 })
                    }
                }
                _ => {
                    // FMOV Dd, Xn (GP → FP)
                    let rn = self.parse_gp_reg()?;
                    Ok(Inst::FmovToD { rd, rn })
                }
            }
        } else {
            // FMOV Xd, Dn (FP → GP)
            let rd = parse_gp_reg_name(&lower)
                .ok_or_else(|| self.err(format!("bad GP reg '{}'", name)))?;
            let (rn, _) = self.parse_fp_reg_with_size()?;
            Ok(Inst::FmovFromD { rd, rn })
        }
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
            other => {
                return Err(self.err(format!(
                    "expected floating-point immediate, got {}",
                    other
                )))
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

    fn parse_optional_lsl12(&mut self) -> Result<bool, ParseError> {
        // Check for ", lsl #12" suffix
        if self.peek() == &Tok::Comma {
            // Peek ahead to see if it's "lsl"
            if self.pos + 1 < self.tokens.len() {
                if let Tok::Ident(ref s) = self.tokens[self.pos + 1].kind {
                    if s.to_lowercase() == "lsl" {
                        self.advance(); // comma
                        self.advance(); // lsl
                        let amount = self.parse_immediate_const_expr("lsl amount")?;
                        if amount == 12 {
                            return Ok(true);
                        }
                        return Err(self.err(format!("expected lsl #12, got lsl #{}", amount)));
                    }
                }
            }
        }
        Ok(false)
    }

    fn parse_optional_lsl_amount(&mut self) -> Result<u8, ParseError> {
        if self.eat(&Tok::Comma) {
            let s = self.expect_ident()?;
            if s.to_lowercase() != "lsl" {
                return Err(self.err(format!("expected 'lsl', got '{}'", s)));
            }
            let amount = self.parse_immediate_const_expr("lsl amount")? as u8;
            Ok(amount)
        } else {
            Ok(0)
        }
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
        rm_is_64bit: bool,
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
            "uxtw" | "uxtx" | "sxtw" | "sxtx" => {
                let extend = match name.as_str() {
                    "uxtw" => {
                        if rm_is_64bit {
                            return Err(self.err(
                                "uxtw add/sub extensions require a w-register operand".into(),
                            ));
                        }
                        RegExtend::Uxtw
                    }
                    "uxtx" => {
                        if !rm_is_64bit {
                            return Err(self.err(
                                "uxtx add/sub extensions require an x-register operand".into(),
                            ));
                        }
                        RegExtend::Uxtx
                    }
                    "sxtw" => {
                        if rm_is_64bit {
                            return Err(self.err(
                                "sxtw add/sub extensions require a w-register operand".into(),
                            ));
                        }
                        RegExtend::Sxtw
                    }
                    "sxtx" => {
                        if !rm_is_64bit {
                            return Err(self.err(
                                "sxtx add/sub extensions require an x-register operand".into(),
                            ));
                        }
                        RegExtend::Sxtx
                    }
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
                "expected add/sub modifier (lsl/lsr/asr/uxtw/uxtx/sxtw/sxtx), got '{}'",
                name
            ))),
        }
    }

    fn validate_add_sub_extended_base_reg(
        &self,
        rn_kind: GpRegKind,
        modifier: Option<AddSubModifier>,
    ) -> Result<(), ParseError> {
        if matches!(modifier, Some(AddSubModifier::Extend(..))) && rn_kind == GpRegKind::Zr {
            return Err(self.err(
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
        lsb: u8,
        width: u8,
    ) -> Result<(), ParseError> {
        let bits = if sf { 64u8 } else { 32u8 };
        if width == 0 {
            return Err(self.err(format!("{} width must be at least 1", mnemonic)));
        }
        if lsb >= bits {
            return Err(self.err(format!(
                "{} lsb {} is out of range for {}-bit register",
                mnemonic, lsb, bits
            )));
        }
        if width > bits - lsb {
            return Err(self.err(format!(
                "{} width {} with lsb {} exceeds {}-bit register width",
                mnemonic, width, lsb, bits
            )));
        }
        Ok(())
    }

    fn starts_register_like_operand(&self) -> bool {
        matches!(self.peek(), Tok::Ident(name) if looks_like_gp_register_name(name))
    }

    fn starts_fp_register_like_operand(&self) -> bool {
        matches!(self.peek(), Tok::Ident(name) if looks_like_fp_register_name(name))
    }

    fn parse_reg_offset_operand(
        &mut self,
        scale: u8,
    ) -> Result<(GpReg, AddrExtend, bool), ParseError> {
        let (rm, is_64bit) = self.parse_gp_reg_with_size()?;
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
                self.parse_immediate_const_expr("register offset shift")? as u8
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

fn parse_gp_reg_name(name: &str) -> Option<GpReg> {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "sp" => Some(SP),
        "xzr" | "wzr" => Some(XZR),
        _ => {
            let (prefix, num_str) = if lower.starts_with('x') || lower.starts_with('w') {
                (&lower[..1], &lower[1..])
            } else {
                return None;
            };
            let num: u8 = num_str.parse().ok()?;
            if num > 30 {
                return None;
            }
            let _ = prefix; // both x and w map to the same encoding
            Some(GpReg::new(num))
        }
    }
}

fn looks_like_gp_register_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "sp"
        || lower == "xzr"
        || lower == "wzr"
        || lower.starts_with('x')
        || lower.starts_with('w')
}

fn parse_fp_reg_name(name: &str) -> Option<FpReg> {
    let lower = name.to_lowercase();
    let (prefix, num_str) =
        if lower.starts_with('d') || lower.starts_with('s') || lower.starts_with('q') {
            (&lower[..1], &lower[1..])
        } else {
            return None;
        };
    let num: u8 = num_str.parse().ok()?;
    if num > 31 {
        return None;
    }
    let _ = prefix;
    Some(FpReg::new(num))
}

fn looks_like_fp_register_name(name: &str) -> bool {
    parse_fp_reg_name(name).is_some()
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

fn parse_index_shift(amount: u8, scale: u8) -> Result<bool, &'static str> {
    match amount {
        0 => Ok(false),
        value if value == scale => Ok(true),
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
    fn parse_ldapr_w() {
        assert_eq!(
            parse_inst("ldapr w8, [x9]"),
            Inst::Ldapr32 { rt: W8, rn: X9 }
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
            Inst::FcvtzsD { rd: X0, rn: D1 }
        );
    }

    #[test]
    fn parse_scvtf_() {
        assert_eq!(parse_inst("scvtf d0, x1"), Inst::ScvtfD { rd: D0, rn: X1 });
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
        let stmts = parse_stmts(".equ ABS2, ABS1 + 5");
        assert_eq!(
            stmts,
            vec![Stmt::Directive(Directive::Set(
                "ABS2".into(),
                Expr::Add(
                    Box::new(Expr::Symbol("ABS1".into())),
                    Box::new(Expr::Int(5))
                ),
            ))]
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
