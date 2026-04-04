//! ARM64 assembly parser.
//!
//! Parses tokenized assembly into structured statements: instructions (as `Inst`),
//! labels, and directives. Resolves instruction aliases (cmp, mov, tst, etc.)
//! to their canonical forms.

use crate::encode::Inst;
use crate::expr::{self, Expr};
use crate::lex::{Tok, Token, Lexer, LexError};
use crate::reg::*;

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
}

/// What kind of relocation is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocKind {
    /// ADRP — page-relative (ARM64_RELOC_PAGE21)
    Page21,
    /// ADD/LDR — page offset (ARM64_RELOC_PAGEOFF12)
    PageOff12,
    /// B/BL — branch (ARM64_RELOC_BRANCH26)
    Branch26,
    /// B.cond / CBZ / CBNZ — assembler-resolved 19-bit branch immediate
    Branch19,
}

/// Assembly directives.
#[derive(Debug, Clone, PartialEq)]
pub enum Directive {
    Text,
    Data,
    Global(String),
    PrivateExtern(String),
    WeakReference(String),
    WeakDefinition(String),
    Align(u32),
    P2Align(u32),
    Byte(Vec<u8>),
    Word(Vec<u32>),
    Quad(Vec<u64>),
    Ascii(Vec<u8>),
    Asciz(Vec<u8>),
    Space(u64),
    Section(String, String),
    SubsectionsViaSymbols,
    BuildVersion { platform: String, version: String },
    Ignored(String),
}

/// Parse error with source location.
#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: error: {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError { line: e.line, col: e.col, msg: e.msg }
    }
}

/// Parse assembly source text into a list of statements.
pub fn parse(src: &str) -> Result<Vec<Stmt>, ParseError> {
    let tokens = Lexer::tokenize(src)?;
    let mut p = Parser::new(&tokens);
    p.parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Tok {
        if self.pos < self.tokens.len() { &self.tokens[self.pos].kind } else { &Tok::Eof }
    }

    fn cur(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn advance(&mut self) -> &Token {
        let t = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() { self.pos += 1; }
        t
    }

    fn expect_ident(&mut self) -> Result<String, ParseError> {
        match self.peek().clone() {
            Tok::Ident(s) => { self.advance(); Ok(s) }
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
        if self.peek() == kind { self.advance(); true } else { false }
    }

    fn err(&self, msg: String) -> ParseError {
        let t = self.cur();
        ParseError { line: t.line, col: t.col, msg }
    }

    fn at_end_of_stmt(&self) -> bool {
        matches!(self.peek(), Tok::Newline | Tok::Eof)
    }

    fn skip_newlines(&mut self) {
        while self.peek() == &Tok::Newline { self.advance(); }
    }

    fn parse_program(&mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        self.skip_newlines();
        while self.peek() != &Tok::Eof {
            self.parse_line(&mut stmts)?;
            self.skip_newlines();
        }
        Ok(stmts)
    }

    fn parse_line(&mut self, stmts: &mut Vec<Stmt>) -> Result<(), ParseError> {
        // A line can be: label, directive, instruction, or empty.
        match self.peek().clone() {
            Tok::Ident(ref name) if name.starts_with('.') => {
                // Could be directive or local label.
                let name = name.clone();
                self.advance();
                if self.eat(&Tok::Colon) {
                    // Local label (.Lxxx:)
                    stmts.push(Stmt::Label(name));
                } else {
                    // Directive
                    stmts.push(self.parse_directive(&name)?);
                }
            }
            Tok::Ident(_) => {
                let name = if let Tok::Ident(s) = self.peek().clone() { s } else { unreachable!() };
                self.advance();
                if self.eat(&Tok::Colon) {
                    // Label
                    stmts.push(Stmt::Label(name));
                    // There might be an instruction on the same line.
                    if !self.at_end_of_stmt() {
                        self.parse_line(stmts)?;
                    }
                } else {
                    // Instruction mnemonic — might have a condition suffix.
                    let mnemonic = self.resolve_mnemonic(&name)?;
                    stmts.push(self.parse_instruction(&mnemonic)?);
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
        if lower == "b" {
            // Check for .cond suffix (e.g., B.EQ, b.ne)
            if let Tok::Ident(ref cond) = self.peek().clone() {
                if cond.starts_with('.') {
                    let full = format!("b{}", cond.to_lowercase());
                    self.advance();
                    return Ok(full);
                }
            }
        }
        Ok(lower)
    }

    fn parse_directive(&mut self, name: &str) -> Result<Stmt, ParseError> {
        let dir = match name {
            ".text" => Directive::Text,
            ".data" => Directive::Data,
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
            ".align" => {
                let n = self.parse_const_expr("alignment expression")? as u32;
                Directive::Align(n)
            }
            ".p2align" => {
                let n = self.parse_const_expr("alignment expression")? as u32;
                Directive::P2Align(n)
            }
            ".byte" => {
                let vals = self.parse_const_expr_list("byte expression")?;
                Directive::Byte(vals.into_iter().map(|v| v as u8).collect())
            }
            ".word" | ".long" => {
                let vals = self.parse_const_expr_list("word expression")?;
                Directive::Word(vals.into_iter().map(|v| v as u32).collect())
            }
            ".quad" => {
                let vals = self.parse_const_expr_list("quad expression")?;
                Directive::Quad(vals.into_iter().map(|v| v as u64).collect())
            }
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
                let platform = self.expect_ident()?;
                self.expect(&Tok::Comma)?;
                // Version can be complex — just collect tokens until newline.
                let mut ver = String::new();
                while !self.at_end_of_stmt() {
                    ver.push_str(&format!("{}", self.peek()));
                    self.advance();
                }
                Directive::BuildVersion { platform, version: ver }
            }
            _ => {
                // Unknown directive — skip to end of line.
                while !self.at_end_of_stmt() { self.advance(); }
                Directive::Ignored(name.to_string())
            }
        };
        Ok(Stmt::Directive(dir))
    }

    fn parse_const_expr_list(&mut self, context: &str) -> Result<Vec<i64>, ParseError> {
        let mut vals = vec![self.parse_const_expr(context)?];
        while self.eat(&Tok::Comma) {
            vals.push(self.parse_const_expr(context)?);
        }
        Ok(vals)
    }

    fn parse_const_expr(&mut self, context: &str) -> Result<i64, ParseError> {
        let expr = self.parse_expr()?;
        expr::eval_pure(&expr).map_err(|err| {
            self.err(format!("{} must be a pure constant expression: {}", context, err))
        })
    }

    fn starts_const_expr(&self) -> bool {
        matches!(self.peek(), Tok::Integer(_) | Tok::Minus | Tok::LParen)
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
        match self.peek().clone() {
            Tok::Integer(value) => {
                self.advance();
                Ok(Expr::Int(value))
            }
            Tok::Ident(symbol) => {
                self.advance();
                Ok(Expr::Symbol(symbol))
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
            "sdiv" => self.parse_3reg("sdiv"),
            "udiv" => self.parse_3reg("udiv"),

            // Logic
            "and" => self.parse_logic("and"),
            "orr" => self.parse_logic("orr"),
            "eor" => self.parse_logic("eor"),
            "ands" => self.parse_logic("ands"),
            "tst" => self.parse_tst(),

            // Move
            "mov" => self.parse_mov(),
            "movz" => self.parse_mov_wide("movz"),
            "movk" => self.parse_mov_wide("movk"),
            "movn" => self.parse_mov_wide("movn"),

            // Shifts
            "lsl" => self.parse_shift("lsl"),
            "lsr" => self.parse_shift("lsr"),
            "asr" => self.parse_shift("asr"),

            // Branches
            "ret" => self.parse_ret(),
            "br" => { let rn = self.parse_gp_reg()?; Ok(Inst::Br { rn }) }
            "blr" => { let rn = self.parse_gp_reg()?; Ok(Inst::Blr { rn }) }

            // Load/store
            "ldr" => self.parse_ldr_str(true),
            "str" => self.parse_ldr_str(false),
            "ldrb" => self.parse_ldrb_h("ldrb"),
            "ldrh" => self.parse_ldrb_h("ldrh"),
            "ldrsw" => self.parse_ldrb_h("ldrsw"),
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
            "fmadd" => self.parse_fmadd(),

            // FP conversion
            "fcvtzs" => self.parse_fcvtzs(),
            "scvtf" => self.parse_scvtf(),
            "fmov" => self.parse_fmov(),

            // System
            "svc" => {
                let imm = self.parse_const_expr("svc immediate")? as u16;
                Ok(Inst::Svc { imm16: imm })
            }
            "nop" => Ok(Inst::Nop),
            "brk" => {
                let imm = self.parse_const_expr("brk immediate")? as u16;
                Ok(Inst::Brk { imm16: imm })
            }

            _ => Err(self.err(format!("unknown mnemonic: {}", mnemonic))),
        }?;
        Ok(Stmt::Instruction(inst))
    }

    // ---- Register parsing helpers ----

    fn parse_gp_reg(&mut self) -> Result<GpReg, ParseError> {
        let name = self.expect_ident()?;
        parse_gp_reg_name(&name).ok_or_else(|| self.err(format!("expected GP register, got '{}'", name)))
    }

    /// Returns (register, is_64bit).
    fn parse_gp_reg_with_size(&mut self) -> Result<(GpReg, bool), ParseError> {
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        if lower == "sp" || lower == "xzr" {
            return Ok((parse_gp_reg_name(&lower).unwrap(), true));
        }
        if lower == "wzr" {
            return Ok((WZR, false));
        }
        if lower.starts_with('x') {
            let reg = parse_gp_reg_name(&lower).ok_or_else(|| self.err(format!("bad register '{}'", name)))?;
            Ok((reg, true))
        } else if lower.starts_with('w') {
            let reg = parse_gp_reg_name(&lower).ok_or_else(|| self.err(format!("bad register '{}'", name)))?;
            Ok((reg, false))
        } else {
            Err(self.err(format!("expected GP register, got '{}'", name)))
        }
    }

    fn parse_fp_reg_with_size(&mut self) -> Result<(FpReg, bool), ParseError> {
        let name = self.expect_ident()?;
        let lower = name.to_lowercase();
        if lower.starts_with('d') {
            let reg = parse_fp_reg_name(&lower).ok_or_else(|| self.err(format!("bad FP register '{}'", name)))?;
            Ok((reg, true)) // double
        } else if lower.starts_with('s') {
            let reg = parse_fp_reg_name(&lower).ok_or_else(|| self.err(format!("bad FP register '{}'", name)))?;
            Ok((reg, false)) // single
        } else {
            Err(self.err(format!("expected FP register, got '{}'", name)))
        }
    }

    // ---- Instruction-specific parsers ----

    fn parse_add_sub(&mut self, is_sub: bool, sets_flags: bool) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;

        if self.starts_const_expr() {
            let imm = self.parse_const_expr("add/sub immediate")? as u16;
            let shift = self.parse_optional_lsl12()?;
            Ok(match (is_sub, sets_flags) {
                (false, false) => Inst::AddImm { rd, rn, imm12: imm, shift, sf },
                (true, false)  => Inst::SubImm { rd, rn, imm12: imm, shift, sf },
                (false, true)  => Inst::AddsImm { rd, rn, imm12: imm, shift, sf },
                (true, true)   => Inst::SubsImm { rd, rn, imm12: imm, shift, sf },
            })
        } else {
            let (rm, _) = self.parse_gp_reg_with_size()?;
            Ok(match (is_sub, sets_flags) {
                (false, false) => Inst::AddReg { rd, rn, rm, sf },
                (true, false)  => Inst::SubReg { rd, rn, rm, sf },
                (false, true)  => Inst::AddsReg { rd, rn, rm, sf },
                (true, true)   => Inst::SubsReg { rd, rn, rm, sf },
            })
        }
    }

    /// ADD that can also handle label@PAGEOFF references (for ADRP+ADD pairs).
    fn parse_add_sub_stmt(&mut self, is_sub: bool, sets_flags: bool) -> Result<Stmt, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;

        // Check for label@PAGEOFF (identifier followed by @)
        if let Tok::Ident(ref name) = self.peek().clone() {
            let lower = name.to_lowercase();
            if !lower.starts_with('x') && !lower.starts_with('w') && lower != "sp" && lower != "xzr" && lower != "wzr" {
                let label = name.clone();
                self.advance();
                let kind = if self.eat(&Tok::At) {
                    let modifier = self.expect_ident()?;
                    match modifier.to_uppercase().as_str() {
                        "PAGEOFF" => RelocKind::PageOff12,
                        "PAGE" => RelocKind::Page21,
                        _ => RelocKind::PageOff12,
                    }
                } else {
                    RelocKind::PageOff12
                };
                let inst = Inst::AddImm { rd, rn, imm12: 0, shift: false, sf };
                return Ok(Stmt::InstructionWithReloc(inst, LabelRef { symbol: label, kind }));
            }
        }

        // Normal add/sub (immediate or register).
        let inst = self.parse_add_sub_operand(rd, rn, sf, is_sub, sets_flags)?;
        Ok(Stmt::Instruction(inst))
    }

    /// Parse the third operand of add/sub (immediate or register).
    fn parse_add_sub_operand(&mut self, rd: GpReg, rn: GpReg, sf: bool, is_sub: bool, sets_flags: bool) -> Result<Inst, ParseError> {
        if self.starts_const_expr() {
            let imm = self.parse_const_expr("add/sub immediate")? as u16;
            let shift = self.parse_optional_lsl12()?;
            Ok(match (is_sub, sets_flags) {
                (false, false) => Inst::AddImm { rd, rn, imm12: imm, shift, sf },
                (true, false)  => Inst::SubImm { rd, rn, imm12: imm, shift, sf },
                (false, true)  => Inst::AddsImm { rd, rn, imm12: imm, shift, sf },
                (true, true)   => Inst::SubsImm { rd, rn, imm12: imm, shift, sf },
            })
        } else {
            let (rm, _) = self.parse_gp_reg_with_size()?;
            Ok(match (is_sub, sets_flags) {
                (false, false) => Inst::AddReg { rd, rn, rm, sf },
                (true, false)  => Inst::SubReg { rd, rn, rm, sf },
                (false, true)  => Inst::AddsReg { rd, rn, rm, sf },
                (true, true)   => Inst::SubsReg { rd, rn, rm, sf },
            })
        }
    }

    fn parse_cmp(&mut self) -> Result<Inst, ParseError> {
        let (rn, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        if self.starts_const_expr() {
            let imm = self.parse_const_expr("cmp immediate")? as u16;
            let shift = self.parse_optional_lsl12()?;
            Ok(Inst::SubsImm { rd: XZR, rn, imm12: imm, shift, sf })
        } else {
            let (rm, _) = self.parse_gp_reg_with_size()?;
            Ok(Inst::SubsReg { rd: XZR, rn, rm, sf })
        }
    }

    fn parse_cmn(&mut self) -> Result<Inst, ParseError> {
        let (rn, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_gp_reg_with_size()?;
        Ok(Inst::AddsReg { rd: XZR, rn, rm, sf })
    }

    fn parse_tst(&mut self) -> Result<Inst, ParseError> {
        let (rn, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_gp_reg_with_size()?;
        Ok(Inst::AndsReg { rd: XZR, rn, rm, sf })
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

    fn parse_logic(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rm, _) = self.parse_gp_reg_with_size()?;
        Ok(match mnemonic {
            "and" => Inst::AndReg { rd, rn, rm, sf },
            "orr" => Inst::OrrReg { rd, rn, rm, sf },
            "eor" => Inst::EorReg { rd, rn, rm, sf },
            "ands" => Inst::AndsReg { rd, rn, rm, sf },
            _ => unreachable!(),
        })
    }

    fn parse_mov(&mut self) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        if self.starts_const_expr() {
            let imm = self.parse_const_expr("mov immediate")?;
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
                Ok(Inst::AddImm { rd, rn: rm, imm12: 0, shift: false, sf })
            } else {
                // MOV Xd, Xm → ORR Xd, XZR, Xm
                Ok(Inst::OrrReg { rd, rn: XZR, rm, sf })
            }
        }
    }

    fn parse_mov_wide(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let imm = self.parse_const_expr("mov wide immediate")? as u16;
        let shift = self.parse_optional_lsl_amount()?;
        Ok(match mnemonic {
            "movz" => Inst::Movz { rd, imm16: imm, shift, sf },
            "movk" => Inst::Movk { rd, imm16: imm, shift, sf },
            "movn" => Inst::Movn { rd, imm16: imm, shift, sf },
            _ => unreachable!(),
        })
    }

    fn parse_shift(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let amount = self.parse_const_expr("shift amount")? as u8;
        Ok(match mnemonic {
            "lsl" => Inst::LslImm { rd, rn, amount, sf },
            "lsr" => Inst::LsrImm { rd, rn, amount, sf },
            "asr" => Inst::AsrImm { rd, rn, amount, sf },
            _ => unreachable!(),
        })
    }

    fn parse_b(&mut self) -> Result<Stmt, ParseError> {
        if self.starts_const_expr() {
            let offset = self.parse_const_expr("branch offset")? as i32;
            Ok(Stmt::Instruction(Inst::B { offset }))
        } else {
            let label = self.expect_ident()?;
            Ok(Stmt::InstructionWithReloc(
                Inst::B { offset: 0 },
                LabelRef { symbol: label, kind: RelocKind::Branch26 },
            ))
        }
    }

    fn parse_bl(&mut self) -> Result<Stmt, ParseError> {
        if self.starts_const_expr() {
            let offset = self.parse_const_expr("branch offset")? as i32;
            Ok(Stmt::Instruction(Inst::Bl { offset }))
        } else {
            let label = self.expect_ident()?;
            Ok(Stmt::InstructionWithReloc(
                Inst::Bl { offset: 0 },
                LabelRef { symbol: label, kind: RelocKind::Branch26 },
            ))
        }
    }

    fn parse_bcond(&mut self, cond_str: &str) -> Result<Stmt, ParseError> {
        let cond = parse_condition(cond_str)
            .ok_or_else(|| self.err(format!("unknown condition: {}", cond_str)))?;
        if self.starts_const_expr() {
            let offset = self.parse_const_expr("branch offset")? as i32;
            Ok(Stmt::Instruction(Inst::BCond { cond, offset }))
        } else {
            let label = self.expect_ident()?;
            Ok(Stmt::InstructionWithReloc(
                Inst::BCond { cond, offset: 0 },
                LabelRef { symbol: label, kind: RelocKind::Branch19 },
            ))
        }
    }

    fn parse_cbz(&mut self, is_nz: bool) -> Result<Stmt, ParseError> {
        let (rt, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        if self.starts_const_expr() {
            let offset = self.parse_const_expr("cbz/cbnz offset")? as i32;
            let inst = if is_nz {
                Inst::Cbnz { rt, offset, sf }
            } else {
                Inst::Cbz { rt, offset, sf }
            };
            Ok(Stmt::Instruction(inst))
        } else {
            let label = self.expect_ident()?;
            let inst = if is_nz {
                Inst::Cbnz { rt, offset: 0, sf }
            } else {
                Inst::Cbz { rt, offset: 0, sf }
            };
            Ok(Stmt::InstructionWithReloc(
                inst,
                LabelRef { symbol: label, kind: RelocKind::Branch19 },
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

    fn parse_adrp(&mut self) -> Result<Stmt, ParseError> {
        let rd = self.parse_gp_reg()?;
        self.expect(&Tok::Comma)?;
        if self.starts_const_expr() {
            let imm = self.parse_const_expr("adrp immediate")? as i32;
            Ok(Stmt::Instruction(Inst::Adrp { rd, imm }))
        } else {
            let label = self.expect_ident()?;
            let kind = if self.eat(&Tok::At) {
                let modifier = self.expect_ident()?;
                match modifier.to_uppercase().as_str() {
                    "PAGE" => RelocKind::Page21,
                    "PAGEOFF" => RelocKind::PageOff12,
                    _ => RelocKind::Page21,
                }
            } else {
                RelocKind::Page21
            };
            Ok(Stmt::InstructionWithReloc(
                Inst::Adrp { rd, imm: 0 },
                LabelRef { symbol: label, kind },
            ))
        }
    }

    fn parse_ldr_str(&mut self, is_load: bool) -> Result<Inst, ParseError> {
        let (rt, sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;

        // Detect label references for LDR (literal pool loads like `ldr x0, =label`).
        // LDR-literal has a different encoding than base+offset — not yet supported.
        if let Tok::Ident(_) = self.peek() {
            if !matches!(self.peek(), Tok::Ident(ref s) if {
                let lo = s.to_lowercase();
                lo == "sp" || lo == "xzr" || lo == "wzr" || lo.starts_with('x') || lo.starts_with('w')
            }) {
                return Err(self.err(
                    "LDR/STR with label reference not yet supported; use ADRP+ADD+LDR pattern instead".into()
                ));
            }
        }

        self.expect(&Tok::LBracket)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;

        if self.eat(&Tok::RBracket) {
            // [Xn] or [Xn], #off (post-index)
            if self.eat(&Tok::Comma) {
                let offset = self.parse_const_expr("post-index offset")? as i16;
                return Ok(if sf {
                    if is_load { Inst::LdrPost64 { rt, rn, offset } }
                    else { Inst::StrPost64 { rt, rn, offset } }
                } else {
                    // 32-bit post-index not yet in Inst — use 64-bit for now.
                    if is_load { Inst::LdrPost64 { rt, rn, offset } }
                    else { Inst::StrPost64 { rt, rn, offset } }
                });
            }
            // [Xn] with no offset → unsigned offset 0
            return Ok(if sf {
                if is_load { Inst::LdrImm64 { rt, rn, offset: 0 } }
                else { Inst::StrImm64 { rt, rn, offset: 0 } }
            } else {
                if is_load { Inst::LdrImm32 { rt, rn, offset: 0 } }
                else { Inst::StrImm32 { rt, rn, offset: 0 } }
            });
        }

        self.expect(&Tok::Comma)?;
        // Could be: register offset or immediate offset
        let offset = self.parse_const_expr("memory offset")?;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            // Pre-index: [Xn, #off]!
            return Ok(if is_load {
                Inst::LdrPre64 { rt, rn, offset: offset as i16 }
            } else {
                Inst::StrPre64 { rt, rn, offset: offset as i16 }
            });
        }

        // Unsigned offset: [Xn, #off]
        Ok(if sf {
            if is_load { Inst::LdrImm64 { rt, rn, offset: offset as u16 } }
            else { Inst::StrImm64 { rt, rn, offset: offset as u16 } }
        } else {
            if is_load { Inst::LdrImm32 { rt, rn, offset: offset as u16 } }
            else { Inst::StrImm32 { rt, rn, offset: offset as u16 } }
        })
    }

    fn parse_ldrb_h(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let rt = self.parse_gp_reg()?;
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let rn = self.parse_gp_reg()?;
        let offset = if self.eat(&Tok::Comma) {
            self.parse_const_expr("memory offset")?
        } else {
            0
        };
        self.expect(&Tok::RBracket)?;
        Ok(match mnemonic {
            "ldrb" => Inst::Ldrb { rt, rn, offset: offset as u16 },
            "ldrh" => Inst::Ldrh { rt, rn, offset: offset as u16 },
            "ldrsw" => Inst::Ldrsw { rt, rn, offset: offset as u16 },
            _ => unreachable!(),
        })
    }

    fn parse_ldp_stp(&mut self, is_load: bool) -> Result<Inst, ParseError> {
        let (rt1, _sf) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rt2, _) = self.parse_gp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        self.expect(&Tok::LBracket)?;
        let (rn, _) = self.parse_gp_reg_with_size()?;

        if self.eat(&Tok::RBracket) {
            // Post-index: [Xn], #off
            self.expect(&Tok::Comma)?;
            let offset = self.parse_const_expr("pair post-index offset")? as i16;
            return Ok(if is_load {
                Inst::LdpPost64 { rt1, rt2, rn, offset }
            } else {
                Inst::StpPost64 { rt1, rt2, rn, offset }
            });
        }

        self.expect(&Tok::Comma)?;
        let offset = self.parse_const_expr("pair offset")? as i16;
        self.expect(&Tok::RBracket)?;

        if self.eat(&Tok::Bang) {
            // Pre-index
            return Ok(if is_load {
                Inst::LdpPre64 { rt1, rt2, rn, offset }
            } else {
                Inst::StpPre64 { rt1, rt2, rn, offset }
            });
        }

        // Signed offset
        Ok(if is_load {
            Inst::LdpOff64 { rt1, rt2, rn, offset }
        } else {
            Inst::StpOff64 { rt1, rt2, rn, offset }
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
            ("fadd", true)  => Inst::FaddD { rd, rn, rm },
            ("fadd", false) => Inst::FaddS { rd, rn, rm },
            ("fsub", true)  => Inst::FsubD { rd, rn, rm },
            ("fsub", false) => Inst::FsubS { rd, rn, rm },
            ("fmul", true)  => Inst::FmulD { rd, rn, rm },
            ("fmul", false) => Inst::FmulS { rd, rn, rm },
            ("fdiv", true)  => Inst::FdivD { rd, rn, rm },
            ("fdiv", false) => Inst::FdivS { rd, rn, rm },
            _ => unreachable!(),
        })
    }

    fn parse_fp_unary(&mut self, mnemonic: &str) -> Result<Inst, ParseError> {
        let (rd, is_double) = self.parse_fp_reg_with_size()?;
        self.expect(&Tok::Comma)?;
        let (rn, _) = self.parse_fp_reg_with_size()?;
        Ok(match (mnemonic, is_double) {
            ("fneg", true)  => Inst::FnegD  { rd, rn },
            ("fneg", false) => Inst::FnegS  { rd, rn },
            ("fabs", true)  => Inst::FabsD  { rd, rn },
            ("fabs", false) => Inst::FabsS  { rd, rn },
            ("fsqrt", true)  => Inst::FsqrtD { rd, rn },
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
            // FMOV Dd, Xn (GP → FP)
            let rd = parse_fp_reg_name(&lower).ok_or_else(|| self.err(format!("bad FP reg '{}'", name)))?;
            let rn = self.parse_gp_reg()?;
            Ok(Inst::FmovToD { rd, rn })
        } else {
            // FMOV Xd, Dn (FP → GP)
            let rd = parse_gp_reg_name(&lower).ok_or_else(|| self.err(format!("bad GP reg '{}'", name)))?;
            let (rn, _) = self.parse_fp_reg_with_size()?;
            Ok(Inst::FmovFromD { rd, rn })
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
                        let amount = self.parse_const_expr("lsl amount")?;
                        if amount == 12 { return Ok(true); }
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
            let amount = self.parse_const_expr("lsl amount")? as u8;
            Ok(amount)
        } else {
            Ok(0)
        }
    }
}

// ---- Name resolution helpers ----

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
            if num > 30 { return None; }
            let _ = prefix; // both x and w map to the same encoding
            Some(GpReg::new(num))
        }
    }
}

fn parse_fp_reg_name(name: &str) -> Option<FpReg> {
    let lower = name.to_lowercase();
    let (prefix, num_str) = if lower.starts_with('d') || lower.starts_with('s') || lower.starts_with('q') {
        (&lower[..1], &lower[1..])
    } else {
        return None;
    };
    let num: u8 = num_str.parse().ok()?;
    if num > 31 { return None; }
    let _ = prefix;
    Some(FpReg::new(num))
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

fn mov_alias_imm(rd: GpReg, imm: i64, sf: bool) -> Option<Inst> {
    let mask = if sf { u64::MAX } else { u32::MAX as u64 };
    let shifts: &[u8] = if sf { &[0, 16, 32, 48] } else { &[0, 16] };
    let value = (imm as u64) & mask;
    let inverted = (!value) & mask;

    for &shift in shifts {
        let shift_bits = shift as u32;
        let movz_imm = ((value >> shift_bits) & 0xFFFF) as u16;
        if value == ((movz_imm as u64) << shift_bits) {
            return Some(Inst::Movz { rd, imm16: movz_imm, shift, sf });
        }

        let movn_imm = ((inverted >> shift_bits) & 0xFFFF) as u16;
        if inverted == ((movn_imm as u64) << shift_bits) {
            return Some(Inst::Movn { rd, imm16: movn_imm, shift, sf });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_inst(src: &str) -> Inst {
        let stmts = parse(src).unwrap();
        stmts.into_iter().find_map(|s| {
            if let Stmt::Instruction(i) = s { Some(i) } else { None }
        }).unwrap()
    }

    fn parse_stmts(src: &str) -> Vec<Stmt> {
        parse(src).unwrap()
    }

    // ---- Data processing ----

    #[test]
    fn parse_add_reg() {
        assert_eq!(parse_inst("add x0, x1, x2"), Inst::AddReg { rd: X0, rn: X1, rm: X2, sf: true });
    }

    #[test]
    fn parse_add_w_reg() {
        assert_eq!(parse_inst("add w3, w4, w5"), Inst::AddReg { rd: W3, rn: W4, rm: W5, sf: false });
    }

    #[test]
    fn parse_sub_imm() {
        assert_eq!(parse_inst("sub x0, x1, #42"), Inst::SubImm { rd: X0, rn: X1, imm12: 42, shift: false, sf: true });
    }

    #[test]
    fn parse_add_imm_lsl12() {
        assert_eq!(parse_inst("add x0, x1, #42, lsl #12"), Inst::AddImm { rd: X0, rn: X1, imm12: 42, shift: true, sf: true });
    }

    #[test]
    fn parse_add_immediate_expression() {
        assert_eq!(parse_inst("add x0, x1, #1 + 2"), Inst::AddImm { rd: X0, rn: X1, imm12: 3, shift: false, sf: true });
    }

    #[test]
    fn parse_cmp_reg() {
        assert_eq!(parse_inst("cmp x0, x1"), Inst::SubsReg { rd: XZR, rn: X0, rm: X1, sf: true });
    }

    #[test]
    fn parse_cmp_imm() {
        assert_eq!(parse_inst("cmp x5, #255"), Inst::SubsImm { rd: XZR, rn: X5, imm12: 255, shift: false, sf: true });
    }

    #[test]
    fn parse_tst_() {
        assert_eq!(parse_inst("tst x0, x1"), Inst::AndsReg { rd: XZR, rn: X0, rm: X1, sf: true });
    }

    #[test]
    fn parse_mul_() {
        assert_eq!(parse_inst("mul x6, x7, x8"), Inst::Mul { rd: X6, rn: X7, rm: X8, sf: true });
    }

    #[test]
    fn parse_and_() {
        assert_eq!(parse_inst("and x3, x4, x5"), Inst::AndReg { rd: X3, rn: X4, rm: X5, sf: true });
    }

    // ---- Move ----

    #[test]
    fn parse_mov_imm() {
        assert_eq!(parse_inst("mov x0, #42"), Inst::Movz { rd: X0, imm16: 42, shift: 0, sf: true });
    }

    #[test]
    fn parse_mov_reg() {
        assert_eq!(parse_inst("mov x0, x1"), Inst::OrrReg { rd: X0, rn: XZR, rm: X1, sf: true });
    }

    #[test]
    fn parse_movz_shift() {
        assert_eq!(parse_inst("movz x0, #0x1234, lsl #16"), Inst::Movz { rd: X0, imm16: 0x1234, shift: 16, sf: true });
    }

    // ---- Shifts ----

    #[test]
    fn parse_lsl_() {
        assert_eq!(parse_inst("lsl x0, x1, #3"), Inst::LslImm { rd: X0, rn: X1, amount: 3, sf: true });
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
        assert_eq!(parse_inst("b.eq #8"), Inst::BCond { cond: Cond::EQ, offset: 8 });
    }

    #[test]
    fn parse_b_ne() {
        assert_eq!(parse_inst("b.ne #12"), Inst::BCond { cond: Cond::NE, offset: 12 });
    }

    #[test]
    fn parse_b_ge() {
        assert_eq!(parse_inst("b.ge #16"), Inst::BCond { cond: Cond::GE, offset: 16 });
    }

    #[test]
    fn parse_cbz_() {
        assert_eq!(parse_inst("cbz x0, #8"), Inst::Cbz { rt: X0, offset: 8, sf: true });
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
        assert_eq!(parse_inst("ldr x0, [x1]"), Inst::LdrImm64 { rt: X0, rn: X1, offset: 0 });
    }

    #[test]
    fn parse_ldr_offset() {
        assert_eq!(parse_inst("ldr x0, [x1, #8]"), Inst::LdrImm64 { rt: X0, rn: X1, offset: 8 });
    }

    #[test]
    fn parse_str_offset() {
        assert_eq!(parse_inst("str x2, [x3, #16]"), Inst::StrImm64 { rt: X2, rn: X3, offset: 16 });
    }

    #[test]
    fn parse_ldr_w() {
        assert_eq!(parse_inst("ldr w4, [x5, #4]"), Inst::LdrImm32 { rt: W4, rn: X5, offset: 4 });
    }

    #[test]
    fn parse_ldrb_() {
        assert_eq!(parse_inst("ldrb w0, [x1, #3]"), Inst::Ldrb { rt: W0, rn: X1, offset: 3 });
    }

    #[test]
    fn parse_stp_pre() {
        assert_eq!(parse_inst("stp x29, x30, [sp, #-16]!"),
            Inst::StpPre64 { rt1: X29, rt2: X30, rn: SP, offset: -16 });
    }

    #[test]
    fn parse_ldp_post() {
        assert_eq!(parse_inst("ldp x29, x30, [sp], #16"),
            Inst::LdpPost64 { rt1: X29, rt2: X30, rn: SP, offset: 16 });
    }

    #[test]
    fn parse_stp_post() {
        assert_eq!(parse_inst("stp x29, x30, [sp], #16"),
            Inst::StpPost64 { rt1: X29, rt2: X30, rn: SP, offset: 16 });
    }

    #[test]
    fn parse_ldp_pre() {
        assert_eq!(parse_inst("ldp x29, x30, [sp, #-16]!"),
            Inst::LdpPre64 { rt1: X29, rt2: X30, rn: SP, offset: -16 });
    }

    #[test]
    fn parse_ldp_off() {
        assert_eq!(parse_inst("ldp x19, x20, [sp, #16]"),
            Inst::LdpOff64 { rt1: X19, rt2: X20, rn: SP, offset: 16 });
    }

    // ---- FP ----

    #[test]
    fn parse_fadd_d() {
        assert_eq!(parse_inst("fadd d0, d1, d2"), Inst::FaddD { rd: D0, rn: D1, rm: D2 });
    }

    #[test]
    fn parse_fadd_s() {
        assert_eq!(parse_inst("fadd s0, s1, s2"), Inst::FaddS { rd: S0, rn: S1, rm: S2 });
    }

    #[test]
    fn parse_fcmp_() {
        assert_eq!(parse_inst("fcmp d0, d1"), Inst::FcmpD { rn: D0, rm: D1 });
    }

    #[test]
    fn parse_fmadd_() {
        assert_eq!(parse_inst("fmadd d0, d1, d2, d3"), Inst::FmaddD { rd: D0, rn: D1, rm: D2, ra: D3 });
    }

    #[test]
    fn parse_fcvtzs_() {
        assert_eq!(parse_inst("fcvtzs x0, d1"), Inst::FcvtzsD { rd: X0, rn: D1 });
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
    fn parse_fmov_from_fp() {
        assert_eq!(parse_inst("fmov x0, d1"), Inst::FmovFromD { rd: X0, rn: D1 });
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
        assert_eq!(parse_inst("fmadd s0, s1, s2, s3"), Inst::FmaddS { rd: S0, rn: S1, rm: S2, ra: S3 });
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
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Global("_main".into()))]);
    }

    #[test]
    fn parse_private_extern_directive() {
        let stmts = parse_stmts(".private_extern _hidden");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::PrivateExtern("_hidden".into()))]);
    }

    #[test]
    fn parse_weak_reference_directive() {
        let stmts = parse_stmts(".weak_reference _puts");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::WeakReference("_puts".into()))]);
    }

    #[test]
    fn parse_weak_definition_directive() {
        let stmts = parse_stmts(".weak_definition _entry");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::WeakDefinition("_entry".into()))]);
    }

    #[test]
    fn parse_asciz_directive() {
        let stmts = parse_stmts(".asciz \"Hello\\n\"");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Asciz(b"Hello\n\0".to_vec()))]);
    }

    #[test]
    fn parse_align_directive() {
        let stmts = parse_stmts(".p2align 4");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::P2Align(4))]);
    }

    #[test]
    fn parse_align_directive_expression() {
        let stmts = parse_stmts(".align 1 + 1");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Align(2))]);
    }

    #[test]
    fn parse_byte_directive() {
        let stmts = parse_stmts(".byte 0x41, 0x42, 0x43");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Byte(vec![0x41, 0x42, 0x43]))]);
    }

    #[test]
    fn parse_word_directive_expression() {
        let stmts = parse_stmts(".word 1 + 2 - 3");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Word(vec![0]))]);
    }

    #[test]
    fn parse_quad_directive_parenthesized_expression() {
        let stmts = parse_stmts(".quad -(1 + 2)");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Quad(vec![u64::MAX - 2]))]);
    }

    #[test]
    fn parse_space_directive_expression() {
        let stmts = parse_stmts(".space (2 + 3)");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Space(5))]);
    }

    #[test]
    fn parse_unknown_directive_is_ignored() {
        let stmts = parse_stmts(".cfi_startproc");
        assert_eq!(stmts, vec![Stmt::Directive(Directive::Ignored(".cfi_startproc".into()))]);
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
        let insts = stmts.iter().filter(|s| matches!(s, Stmt::Instruction(_))).count();
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
    fn error_ldr_literal_not_supported() {
        let result = parse("ldr x0, some_label");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.msg.contains("not yet supported"), "got: {}", err.msg);
    }

    #[test]
    fn error_symbolic_directive_expression_requires_constant() {
        let result = parse(".quad foo - 1");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.msg.contains("pure constant expression"), "got: {}", err.msg);
    }

    // ---- Case insensitivity ----

    #[test]
    fn parse_uppercase_add() {
        assert_eq!(parse_inst("ADD X0, X1, X2"), Inst::AddReg { rd: X0, rn: X1, rm: X2, sf: true });
    }

    #[test]
    fn parse_uppercase_b_eq() {
        assert_eq!(parse_inst("B.EQ #8"), Inst::BCond { cond: Cond::EQ, offset: 8 });
    }

    #[test]
    fn parse_mixed_case_ldr() {
        assert_eq!(parse_inst("Ldr X0, [X1, #8]"), Inst::LdrImm64 { rt: X0, rn: X1, offset: 8 });
    }

    #[test]
    fn parse_uppercase_add_does_not_treat_x2_as_label() {
        // BUG 3 regression: uppercase X2 must be parsed as register, not label
        assert_eq!(parse_inst("ADD X0, X1, X2"), Inst::AddReg { rd: X0, rn: X1, rm: X2, sf: true });
    }

    #[test]
    fn parse_uppercase_mov_sp() {
        assert_eq!(parse_inst("MOV X29, SP"), Inst::AddImm { rd: X29, rn: SP, imm12: 0, shift: false, sf: true });
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
        assert_eq!(parse_inst("lsl w0, w1, #3"), Inst::LslImm { rd: W0, rn: W1, amount: 3, sf: false });
    }

    #[test]
    fn parse_w_register_lsr() {
        assert_eq!(parse_inst("lsr w5, w6, #8"), Inst::LsrImm { rd: W5, rn: W6, amount: 8, sf: false });
    }

    #[test]
    fn parse_w_register_asr() {
        assert_eq!(parse_inst("asr w5, w6, #15"), Inst::AsrImm { rd: W5, rn: W6, amount: 15, sf: false });
    }

    #[test]
    fn parse_mov_negative_imm() {
        // mov x0, #-1 → movn x0, #0
        assert_eq!(parse_inst("mov x0, #-1"), Inst::Movn { rd: X0, imm16: 0, shift: 0, sf: true });
    }

    #[test]
    fn parse_mov_negative_42() {
        // mov x0, #-42 → movn x0, #41
        assert_eq!(parse_inst("mov x0, #-42"), Inst::Movn { rd: X0, imm16: 41, shift: 0, sf: true });
    }

    #[test]
    fn parse_mov_negative_65537() {
        assert_eq!(parse_inst("mov x0, #-65537"), Inst::Movn { rd: X0, imm16: 1, shift: 16, sf: true });
    }

    #[test]
    fn parse_mov_large_positive_shifted() {
        assert_eq!(parse_inst("mov x0, #0x12340000"), Inst::Movz { rd: X0, imm16: 0x1234, shift: 16, sf: true });
    }

    #[test]
    fn parse_mov_wide_expression() {
        assert_eq!(parse_inst("movz x0, #1 + 1, lsl #4 + 12"), Inst::Movz { rd: X0, imm16: 2, shift: 16, sf: true });
    }

    #[test]
    fn parse_cbnz_w() {
        assert_eq!(parse_inst("cbnz w5, #8"), Inst::Cbnz { rt: W5, offset: 8, sf: false });
    }

    #[test]
    fn parse_cbz_w() {
        assert_eq!(parse_inst("cbz w0, #12"), Inst::Cbz { rt: W0, offset: 12, sf: false });
    }

    #[test]
    fn parse_memory_offset_expression() {
        assert_eq!(parse_inst("ldr x0, [x1, #4 + 4]"), Inst::LdrImm64 { rt: X0, rn: X1, offset: 8 });
    }

    #[test]
    fn parse_all_condition_codes() {
        // Exercise all 14 named condition codes
        for (name, cond) in [
            ("eq", Cond::EQ), ("ne", Cond::NE), ("cs", Cond::CS), ("cc", Cond::CC),
            ("mi", Cond::MI), ("pl", Cond::PL), ("vs", Cond::VS), ("vc", Cond::VC),
            ("hi", Cond::HI), ("ls", Cond::LS), ("ge", Cond::GE), ("lt", Cond::LT),
            ("gt", Cond::GT), ("le", Cond::LE),
        ] {
            let src = format!("b.{} #4", name);
            assert_eq!(parse_inst(&src), Inst::BCond { cond, offset: 4 }, "failed for b.{}", name);
        }
    }

    #[test]
    fn parse_hs_lo_aliases() {
        assert_eq!(parse_inst("b.hs #4"), Inst::BCond { cond: Cond::CS, offset: 4 });
        assert_eq!(parse_inst("b.lo #4"), Inst::BCond { cond: Cond::CC, offset: 4 });
    }

    #[test]
    fn parse_b_label() {
        assert_eq!(
            parse_stmts("b done"),
            vec![Stmt::InstructionWithReloc(
                Inst::B { offset: 0 },
                LabelRef { symbol: "done".into(), kind: RelocKind::Branch26 },
            )]
        );
    }

    #[test]
    fn parse_b_eq_label() {
        assert_eq!(
            parse_stmts("b.eq done"),
            vec![Stmt::InstructionWithReloc(
                Inst::BCond { cond: Cond::EQ, offset: 0 },
                LabelRef { symbol: "done".into(), kind: RelocKind::Branch19 },
            )]
        );
    }

    #[test]
    fn parse_cbz_label() {
        assert_eq!(
            parse_stmts("cbz x0, done"),
            vec![Stmt::InstructionWithReloc(
                Inst::Cbz { rt: X0, offset: 0, sf: true },
                LabelRef { symbol: "done".into(), kind: RelocKind::Branch19 },
            )]
        );
    }
}
