//! ARM64 assembly lexer.
//!
//! Tokenizes assembly text into a stream of tokens: mnemonics, registers,
//! immediates, labels, directives, addressing punctuation, and comments.

use std::fmt;

/// A token with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: Tok,
    pub line: u32,
    pub col: u32,
}

/// Token kinds for ARM64 assembly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    /// Identifier: instruction mnemonic, label name, register name, directive arg.
    /// Examples: `add`, `_main`, `x0`, `lsl`
    Ident(String),

    /// Integer literal (after `#` or standalone in directives).
    /// Stored as i64 to handle negative immediates.
    Integer(i64),

    /// Floating-point literal used by instructions like `fmov d0, #3.5`.
    Float(String),

    /// String literal bytes (in .ascii/.asciz directives): `"hello\n"`
    StringLit(Vec<u8>),

    /// `#` — immediate prefix
    Hash,
    /// `,` — operand separator
    Comma,
    /// `+` — expression operator
    Plus,
    /// `-` — expression operator
    Minus,
    /// `:` — label suffix
    Colon,
    /// `(` — expression grouping
    LParen,
    /// `)` — expression grouping
    RParen,
    /// `[` — addressing mode open
    LBracket,
    /// `]` — addressing mode close
    RBracket,
    /// `{` — register list open
    LBrace,
    /// `}` — register list close
    RBrace,
    /// `!` — pre-index writeback marker
    Bang,
    /// `.` — directive prefix or label component
    Dot,
    /// `@` — relocation modifier (e.g., `@PAGE`, `@PAGEOFF`)
    At,
    /// `=` — literal pool load (`ldr x0, =label`)
    Equals,

    /// End of line (significant in assembly — terminates statements).
    Newline,
    /// End of file.
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Ident(s) => write!(f, "{}", s),
            Tok::Integer(n) => write!(f, "{}", n),
            Tok::Float(s) => write!(f, "{}", s),
            Tok::StringLit(bytes) => {
                write!(f, "\"{}\"", String::from_utf8_lossy(bytes).escape_default())
            }
            Tok::Hash => write!(f, "#"),
            Tok::Comma => write!(f, ","),
            Tok::Plus => write!(f, "+"),
            Tok::Minus => write!(f, "-"),
            Tok::Colon => write!(f, ":"),
            Tok::LParen => write!(f, "("),
            Tok::RParen => write!(f, ")"),
            Tok::LBracket => write!(f, "["),
            Tok::RBracket => write!(f, "]"),
            Tok::LBrace => write!(f, "{{"),
            Tok::RBrace => write!(f, "}}"),
            Tok::Bang => write!(f, "!"),
            Tok::Dot => write!(f, "."),
            Tok::At => write!(f, "@"),
            Tok::Equals => write!(f, "="),
            Tok::Newline => write!(f, "\\n"),
            Tok::Eof => write!(f, "EOF"),
        }
    }
}

/// Lexer state.
pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: u32,
    col: u32,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src: src.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    /// Tokenize the entire input into a Vec of tokens.
    pub fn tokenize(src: &str) -> Result<Vec<Token>, LexError> {
        let mut lexer = Lexer::new(src);
        let mut tokens = Vec::new();
        loop {
            let tok = lexer.next_token()?;
            let is_eof = tok.kind == Tok::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> u8 {
        if self.pos < self.src.len() {
            self.src[self.pos]
        } else {
            0
        }
    }

    fn peek2(&self) -> u8 {
        if self.pos + 1 < self.src.len() {
            self.src[self.pos + 1]
        } else {
            0
        }
    }

    fn advance(&mut self) -> u8 {
        let ch = self.peek();
        if ch == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        self.pos += 1;
        ch
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.src.len() {
            match self.peek() {
                b' ' | b'\t' | b'\r' => {
                    self.advance();
                }
                _ => break,
            }
        }
    }

    fn skip_line_comment(&mut self) {
        while self.pos < self.src.len() && self.peek() != b'\n' {
            self.advance();
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), LexError> {
        let start_line = self.line;
        let start_col = self.col;
        self.advance(); // skip /
        self.advance(); // skip *
        loop {
            if self.pos >= self.src.len() {
                return Err(LexError {
                    line: start_line,
                    col: start_col,
                    msg: "unterminated block comment".into(),
                });
            }
            if self.peek() == b'*' && self.peek2() == b'/' {
                self.advance();
                self.advance();
                return Ok(());
            }
            self.advance();
        }
    }

    fn make_tok(&self, kind: Tok, line: u32, col: u32) -> Token {
        Token { kind, line, col }
    }

    pub fn next_token(&mut self) -> Result<Token, LexError> {
        // Skip whitespace (but not newlines — they're significant).
        self.skip_whitespace();

        let line = self.line;
        let col = self.col;

        if self.pos >= self.src.len() {
            return Ok(self.make_tok(Tok::Eof, line, col));
        }

        let ch = self.peek();

        // Comments.
        if ch == b'/' {
            if self.peek2() == b'/' {
                self.skip_line_comment();
                // Don't emit the comment — just continue to newline or EOF.
                return self.next_token();
            }
            if self.peek2() == b'*' {
                self.skip_block_comment()?;
                return self.next_token();
            }
        }
        // Also handle ; as line comment (common in some assemblers)
        if ch == b';' {
            self.skip_line_comment();
            return self.next_token();
        }

        // Single-character tokens.
        match ch {
            b'\n' => {
                self.advance();
                return Ok(self.make_tok(Tok::Newline, line, col));
            }
            b',' => {
                self.advance();
                return Ok(self.make_tok(Tok::Comma, line, col));
            }
            b'+' => {
                self.advance();
                return Ok(self.make_tok(Tok::Plus, line, col));
            }
            b'-' => {
                self.advance();
                return Ok(self.make_tok(Tok::Minus, line, col));
            }
            b':' => {
                self.advance();
                return Ok(self.make_tok(Tok::Colon, line, col));
            }
            b'(' => {
                self.advance();
                return Ok(self.make_tok(Tok::LParen, line, col));
            }
            b')' => {
                self.advance();
                return Ok(self.make_tok(Tok::RParen, line, col));
            }
            b'[' => {
                self.advance();
                return Ok(self.make_tok(Tok::LBracket, line, col));
            }
            b']' => {
                self.advance();
                return Ok(self.make_tok(Tok::RBracket, line, col));
            }
            b'{' => {
                self.advance();
                return Ok(self.make_tok(Tok::LBrace, line, col));
            }
            b'}' => {
                self.advance();
                return Ok(self.make_tok(Tok::RBrace, line, col));
            }
            b'!' => {
                self.advance();
                return Ok(self.make_tok(Tok::Bang, line, col));
            }
            b'@' => {
                self.advance();
                return Ok(self.make_tok(Tok::At, line, col));
            }
            b'=' => {
                self.advance();
                return Ok(self.make_tok(Tok::Equals, line, col));
            }
            _ => {}
        }

        // Hash (immediate prefix). Might be followed by a negative sign or hex.
        if ch == b'#' {
            self.advance();
            self.skip_whitespace();
            // Parse the immediate value inline.
            if self.pos < self.src.len() && (self.peek().is_ascii_digit() || self.peek() == b'-') {
                let kind = self.read_number_literal()?;
                return Ok(self.make_tok(kind, line, col));
            }
            // Bare # (shouldn't happen in valid assembly, but return it)
            return Ok(self.make_tok(Tok::Hash, line, col));
        }

        // Names that begin with '.' remain special so directives and
        // local labels keep their existing token shape.
        if ch == b'.' {
            self.advance();
            if self.pos < self.src.len() && is_ident_start(self.peek()) {
                let name = self.read_ident_body();
                return Ok(self.make_tok(Tok::Ident(format!(".{}", name)), line, col));
            }
            return Ok(self.make_tok(Tok::Dot, line, col));
        }

        // String literal.
        if ch == b'"' {
            let s = self.read_string_literal()?;
            return Ok(self.make_tok(Tok::StringLit(s), line, col));
        }

        // Number (standalone — can appear in directives like `.word 42`).
        if ch.is_ascii_digit() {
            let kind = self.read_number_literal()?;
            return Ok(self.make_tok(kind, line, col));
        }

        // Identifier (mnemonic, register, label, etc.).
        if is_ident_start(ch) {
            let name = self.read_ident_body();
            return Ok(self.make_tok(Tok::Ident(name), line, col));
        }

        Err(LexError {
            line,
            col,
            msg: format!("unexpected character: '{}'", ch as char),
        })
    }

    fn read_number_literal(&mut self) -> Result<Tok, LexError> {
        let line = self.line;
        let col = self.col;
        let start = self.pos;
        let negative = if self.peek() == b'-' {
            self.advance();
            true
        } else {
            false
        };

        if self.peek() == b'0' && (self.peek2() == b'x' || self.peek2() == b'X') {
            // Hex.
            self.advance(); // 0
            self.advance(); // x
            let start = self.pos;
            while self.pos < self.src.len() && self.peek().is_ascii_hexdigit() {
                self.advance();
            }
            if self.pos == start {
                return Err(LexError {
                    line,
                    col,
                    msg: "expected hex digits after 0x".into(),
                });
            }
            let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
            let val = u64::from_str_radix(s, 16).map_err(|e| LexError {
                line,
                col,
                msg: format!("invalid hex: {}", e),
            })?;
            Ok(Tok::Integer(to_i64(val, negative, line, col)?))
        } else {
            // Decimal.
            let digits_start = self.pos;
            while self.pos < self.src.len() && self.peek().is_ascii_digit() {
                self.advance();
            }
            if self.pos < self.src.len() && self.peek() == b'.' && self.peek2().is_ascii_digit() {
                self.advance();
                while self.pos < self.src.len() && self.peek().is_ascii_digit() {
                    self.advance();
                }
                let s = std::str::from_utf8(&self.src[start..self.pos]).unwrap();
                return Ok(Tok::Float(s.into()));
            }

            let s = std::str::from_utf8(&self.src[digits_start..self.pos]).unwrap();
            let val: u64 = s.parse().map_err(|e| LexError {
                line,
                col,
                msg: format!("invalid integer: {}", e),
            })?;
            Ok(Tok::Integer(to_i64(val, negative, line, col)?))
        }
    }

    fn read_ident_body(&mut self) -> String {
        let start = self.pos;
        while self.pos < self.src.len() && is_ident_cont(self.peek()) {
            self.advance();
        }
        String::from_utf8_lossy(&self.src[start..self.pos]).into_owned()
    }

    fn read_string_literal(&mut self) -> Result<Vec<u8>, LexError> {
        let line = self.line;
        let col = self.col;
        self.advance(); // skip opening "
        let mut buf = Vec::new();
        loop {
            if self.pos >= self.src.len() || self.peek() == b'\n' {
                return Err(LexError {
                    line,
                    col,
                    msg: "unterminated string literal".into(),
                });
            }
            let ch = self.advance();
            if ch == b'"' {
                break;
            }
            if ch == b'\\' {
                if self.pos >= self.src.len() || self.peek() == b'\n' {
                    return Err(LexError {
                        line,
                        col,
                        msg: "unterminated string escape".into(),
                    });
                }
                let esc = self.advance();
                match esc {
                    b'a' => buf.push(0x07),
                    b'b' => buf.push(0x08),
                    b'f' => buf.push(0x0c),
                    b'n' => buf.push(b'\n'),
                    b't' => buf.push(b'\t'),
                    b'r' => buf.push(b'\r'),
                    b'v' => buf.push(0x0b),
                    b'0'..=b'7' => {
                        let mut value = (esc - b'0') as u16;
                        for _ in 0..2 {
                            if self.pos < self.src.len() && matches!(self.peek(), b'0'..=b'7') {
                                value = (value << 3) | (self.advance() - b'0') as u16;
                            } else {
                                break;
                            }
                        }
                        buf.push(value as u8);
                    }
                    b'x' => {
                        let start = self.pos;
                        let mut value = 0u16;
                        while self.pos < self.src.len() && self.peek().is_ascii_hexdigit() {
                            value = (value << 4)
                                | (self.advance() as char).to_digit(16).unwrap() as u16;
                        }
                        if self.pos == start {
                            return Err(LexError {
                                line,
                                col,
                                msg: "expected hex digits after \\x in string literal".into(),
                            });
                        }
                        buf.push(value as u8);
                    }
                    b'\\' => buf.push(b'\\'),
                    b'"' => buf.push(b'"'),
                    _ => {
                        buf.push(b'\\');
                        buf.push(esc);
                    }
                }
            } else {
                buf.push(ch);
            }
        }
        Ok(buf)
    }
}

fn is_ident_start(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

fn is_ident_cont(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'$' || ch == b'.'
}

/// Convert a u64 magnitude + sign to i64, with overflow checking.
fn to_i64(val: u64, negative: bool, line: u32, col: u32) -> Result<i64, LexError> {
    if negative {
        let min_mag = (i64::MAX as u64) + 1; // 2^63
        if val > min_mag {
            return Err(LexError {
                line,
                col,
                msg: format!("integer -{} overflows i64", val),
            });
        }
        if val == min_mag {
            return Ok(i64::MIN); // special case: -(2^63) can't be computed via negation
        }
        Ok(-(val as i64))
    } else {
        if val > i64::MAX as u64 {
            return Err(LexError {
                line,
                col,
                msg: format!("integer {} overflows i64", val),
            });
        }
        Ok(val as i64)
    }
}

/// A lexer error with source location.
#[derive(Debug, Clone)]
pub struct LexError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: error: {}", self.line, self.col, self.msg)
    }
}

impl std::error::Error for LexError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        Lexer::tokenize(src)
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    fn tok_kinds(src: &str) -> Vec<Tok> {
        toks(src).into_iter().filter(|t| *t != Tok::Eof).collect()
    }

    // ---- Basic token recognition ----

    #[test]
    fn empty_input() {
        assert_eq!(toks(""), vec![Tok::Eof]);
    }

    #[test]
    fn just_newlines() {
        assert_eq!(tok_kinds("\n\n"), vec![Tok::Newline, Tok::Newline]);
    }

    #[test]
    fn single_ident() {
        assert_eq!(tok_kinds("add"), vec![Tok::Ident("add".into())]);
    }

    #[test]
    fn registers() {
        assert_eq!(
            tok_kinds("x0 x30 sp xzr w15 d0 s31"),
            vec![
                Tok::Ident("x0".into()),
                Tok::Ident("x30".into()),
                Tok::Ident("sp".into()),
                Tok::Ident("xzr".into()),
                Tok::Ident("w15".into()),
                Tok::Ident("d0".into()),
                Tok::Ident("s31".into()),
            ]
        );
    }

    #[test]
    fn immediate_decimal() {
        assert_eq!(tok_kinds("#42"), vec![Tok::Integer(42)]);
    }

    #[test]
    fn braces_tokenize() {
        assert_eq!(
            tok_kinds("{ v0, v1 }"),
            vec![
                Tok::LBrace,
                Tok::Ident("v0".into()),
                Tok::Comma,
                Tok::Ident("v1".into()),
                Tok::RBrace,
            ]
        );
    }

    #[test]
    fn immediate_negative() {
        assert_eq!(tok_kinds("#-16"), vec![Tok::Integer(-16)]);
    }

    #[test]
    fn immediate_hex() {
        assert_eq!(tok_kinds("#0xFF"), vec![Tok::Integer(255)]);
    }

    #[test]
    fn immediate_hex_large() {
        assert_eq!(tok_kinds("#0xBEEF"), vec![Tok::Integer(0xBEEF)]);
    }

    #[test]
    fn immediate_float() {
        assert_eq!(
            tok_kinds("#3.50000000"),
            vec![Tok::Float("3.50000000".into())]
        );
    }

    // ---- Punctuation ----

    #[test]
    fn comma() {
        assert_eq!(
            tok_kinds("x0, x1, x2"),
            vec![
                Tok::Ident("x0".into()),
                Tok::Comma,
                Tok::Ident("x1".into()),
                Tok::Comma,
                Tok::Ident("x2".into()),
            ]
        );
    }

    #[test]
    fn expression_tokens() {
        assert_eq!(
            tok_kinds("1 + foo - (2)"),
            vec![
                Tok::Integer(1),
                Tok::Plus,
                Tok::Ident("foo".into()),
                Tok::Minus,
                Tok::LParen,
                Tok::Integer(2),
                Tok::RParen,
            ]
        );
    }

    #[test]
    fn brackets() {
        assert_eq!(
            tok_kinds("[x0]"),
            vec![Tok::LBracket, Tok::Ident("x0".into()), Tok::RBracket,]
        );
    }

    #[test]
    fn brackets_with_offset() {
        assert_eq!(
            tok_kinds("[x1, #16]"),
            vec![
                Tok::LBracket,
                Tok::Ident("x1".into()),
                Tok::Comma,
                Tok::Integer(16),
                Tok::RBracket,
            ]
        );
    }

    #[test]
    fn pre_index() {
        assert_eq!(
            tok_kinds("[x1, #-16]!"),
            vec![
                Tok::LBracket,
                Tok::Ident("x1".into()),
                Tok::Comma,
                Tok::Integer(-16),
                Tok::RBracket,
                Tok::Bang,
            ]
        );
    }

    #[test]
    fn label() {
        assert_eq!(
            tok_kinds("_main:"),
            vec![Tok::Ident("_main".into()), Tok::Colon,]
        );
    }

    #[test]
    fn local_label() {
        assert_eq!(
            tok_kinds(".Lloop:"),
            vec![Tok::Ident(".Lloop".into()), Tok::Colon,]
        );
    }

    #[test]
    fn embedded_dot_symbol_label() {
        assert_eq!(
            tok_kinds("l_.str:"),
            vec![Tok::Ident("l_.str".into()), Tok::Colon,]
        );
    }

    // ---- Directives ----

    #[test]
    fn directive_global() {
        assert_eq!(
            tok_kinds(".global _main"),
            vec![Tok::Ident(".global".into()), Tok::Ident("_main".into()),]
        );
    }

    #[test]
    fn directive_globl() {
        assert_eq!(
            tok_kinds(".globl _main"),
            vec![Tok::Ident(".globl".into()), Tok::Ident("_main".into()),]
        );
    }

    #[test]
    fn directive_text() {
        assert_eq!(tok_kinds(".text"), vec![Tok::Ident(".text".into())]);
    }

    #[test]
    fn directive_data_content() {
        assert_eq!(
            tok_kinds(".asciz \"Hello, World!\\n\""),
            vec![
                Tok::Ident(".asciz".into()),
                Tok::StringLit(b"Hello, World!\n".to_vec()),
            ]
        );
    }

    #[test]
    fn directive_ascii_octal_and_c_escapes() {
        assert_eq!(
            tok_kinds(".ascii \"\\b\\t\\n\\013\\f\\r\\016\\017\""),
            vec![
                Tok::Ident(".ascii".into()),
                Tok::StringLit(vec![0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f]),
            ]
        );
    }

    #[test]
    fn directive_byte() {
        assert_eq!(
            tok_kinds(".byte 0x41, 0x42"),
            vec![
                Tok::Ident(".byte".into()),
                Tok::Integer(0x41),
                Tok::Comma,
                Tok::Integer(0x42),
            ]
        );
    }

    #[test]
    fn directive_negative_number_uses_minus_token() {
        assert_eq!(
            tok_kinds(".word -1"),
            vec![Tok::Ident(".word".into()), Tok::Minus, Tok::Integer(1),]
        );
    }

    #[test]
    fn directive_p2align() {
        assert_eq!(
            tok_kinds(".p2align 4"),
            vec![Tok::Ident(".p2align".into()), Tok::Integer(4),]
        );
    }

    // ---- Comments ----

    #[test]
    fn line_comment() {
        assert_eq!(
            tok_kinds("add x0, x1, x2 // this is a comment\n"),
            vec![
                Tok::Ident("add".into()),
                Tok::Ident("x0".into()),
                Tok::Comma,
                Tok::Ident("x1".into()),
                Tok::Comma,
                Tok::Ident("x2".into()),
                Tok::Newline,
            ]
        );
    }

    #[test]
    fn block_comment() {
        assert_eq!(
            tok_kinds("add /* comment */ x0, x1, x2"),
            vec![
                Tok::Ident("add".into()),
                Tok::Ident("x0".into()),
                Tok::Comma,
                Tok::Ident("x1".into()),
                Tok::Comma,
                Tok::Ident("x2".into()),
            ]
        );
    }

    #[test]
    fn semicolon_comment() {
        assert_eq!(
            tok_kinds("nop ; comment\n"),
            vec![Tok::Ident("nop".into()), Tok::Newline,]
        );
    }

    // ---- Relocation modifiers ----

    #[test]
    fn relocation_page() {
        assert_eq!(
            tok_kinds("msg@PAGE"),
            vec![Tok::Ident("msg".into()), Tok::At, Tok::Ident("PAGE".into()),]
        );
    }

    #[test]
    fn relocation_pageoff() {
        assert_eq!(
            tok_kinds("msg@PAGEOFF"),
            vec![
                Tok::Ident("msg".into()),
                Tok::At,
                Tok::Ident("PAGEOFF".into()),
            ]
        );
    }

    // ---- Full instruction lines ----

    #[test]
    fn full_add_instruction() {
        assert_eq!(
            tok_kinds("add x0, x1, x2\n"),
            vec![
                Tok::Ident("add".into()),
                Tok::Ident("x0".into()),
                Tok::Comma,
                Tok::Ident("x1".into()),
                Tok::Comma,
                Tok::Ident("x2".into()),
                Tok::Newline,
            ]
        );
    }

    #[test]
    fn full_ldr_with_offset() {
        assert_eq!(
            tok_kinds("ldr x0, [x1, #8]\n"),
            vec![
                Tok::Ident("ldr".into()),
                Tok::Ident("x0".into()),
                Tok::Comma,
                Tok::LBracket,
                Tok::Ident("x1".into()),
                Tok::Comma,
                Tok::Integer(8),
                Tok::RBracket,
                Tok::Newline,
            ]
        );
    }

    #[test]
    fn full_stp_pre_index() {
        assert_eq!(
            tok_kinds("stp x29, x30, [sp, #-16]!\n"),
            vec![
                Tok::Ident("stp".into()),
                Tok::Ident("x29".into()),
                Tok::Comma,
                Tok::Ident("x30".into()),
                Tok::Comma,
                Tok::LBracket,
                Tok::Ident("sp".into()),
                Tok::Comma,
                Tok::Integer(-16),
                Tok::RBracket,
                Tok::Bang,
                Tok::Newline,
            ]
        );
    }

    #[test]
    fn full_movz_with_shift() {
        assert_eq!(
            tok_kinds("movz x0, #0x1234, lsl #16\n"),
            vec![
                Tok::Ident("movz".into()),
                Tok::Ident("x0".into()),
                Tok::Comma,
                Tok::Integer(0x1234),
                Tok::Comma,
                Tok::Ident("lsl".into()),
                Tok::Integer(16),
                Tok::Newline,
            ]
        );
    }

    #[test]
    fn full_adrp_with_relocation() {
        assert_eq!(
            tok_kinds("adrp x1, msg@PAGE\n"),
            vec![
                Tok::Ident("adrp".into()),
                Tok::Ident("x1".into()),
                Tok::Comma,
                Tok::Ident("msg".into()),
                Tok::At,
                Tok::Ident("PAGE".into()),
                Tok::Newline,
            ]
        );
    }

    #[test]
    fn labeled_instruction() {
        assert_eq!(
            tok_kinds("_main:\n    stp x29, x30, [sp, #-16]!\n"),
            vec![
                Tok::Ident("_main".into()),
                Tok::Colon,
                Tok::Newline,
                Tok::Ident("stp".into()),
                Tok::Ident("x29".into()),
                Tok::Comma,
                Tok::Ident("x30".into()),
                Tok::Comma,
                Tok::LBracket,
                Tok::Ident("sp".into()),
                Tok::Comma,
                Tok::Integer(-16),
                Tok::RBracket,
                Tok::Bang,
                Tok::Newline,
            ]
        );
    }

    // ---- Multi-line program ----

    #[test]
    fn hello_world_fragment() {
        let src = "\
.global _main
.align 4

_main:
    mov x0, #1
    adrp x1, msg@PAGE
    add x1, x1, msg@PAGEOFF
    mov x2, #14
    mov x16, #4
    svc #0x80
";
        let tokens = Lexer::tokenize(src).unwrap();
        // Should parse without errors; just verify count is reasonable.
        let non_trivial: Vec<_> = tokens
            .iter()
            .filter(|t| !matches!(t.kind, Tok::Newline | Tok::Eof))
            .collect();
        assert!(
            non_trivial.len() > 30,
            "expected 30+ tokens, got {}",
            non_trivial.len()
        );
        // First token should be .global
        assert_eq!(tokens[0].kind, Tok::Ident(".global".into()));
    }

    // ---- Error cases ----

    #[test]
    fn unterminated_string() {
        let result = Lexer::tokenize(".asciz \"unterminated\n");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.msg.contains("unterminated"), "got: {}", err.msg);
    }

    #[test]
    fn unterminated_block_comment() {
        let result = Lexer::tokenize("add /* never closed");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.msg.contains("unterminated"), "got: {}", err.msg);
    }

    #[test]
    fn bad_hex() {
        let result = Lexer::tokenize("#0x");
        assert!(result.is_err());
    }

    #[test]
    fn overflow_positive_integer() {
        // i64::MAX + 1 should error
        let result = Lexer::tokenize("#9223372036854775808");
        assert!(result.is_err());
        assert!(result.unwrap_err().msg.contains("overflows"));
    }

    #[test]
    fn overflow_negative_hex() {
        // Larger than i64::MIN magnitude should error
        let result = Lexer::tokenize("#-0x8000000000000001");
        assert!(result.is_err());
        assert!(result.unwrap_err().msg.contains("overflows"));
    }

    #[test]
    fn i64_min_is_valid() {
        // -9223372036854775808 = i64::MIN, should be accepted
        let result = Lexer::tokenize("#-9223372036854775808");
        assert!(result.is_ok());
        assert_eq!(
            tok_kinds("#-9223372036854775808"),
            vec![Tok::Integer(i64::MIN)]
        );
    }

    // ---- Source locations ----

    #[test]
    fn token_locations() {
        let tokens = Lexer::tokenize("add x0, x1\n  sub x2, x3\n").unwrap();
        // "add" is at line 1, col 1
        assert_eq!(tokens[0].line, 1);
        assert_eq!(tokens[0].col, 1);
        // "sub" is at line 2, col 3
        let sub_tok = tokens
            .iter()
            .find(|t| t.kind == Tok::Ident("sub".into()))
            .unwrap();
        assert_eq!(sub_tok.line, 2);
        assert_eq!(sub_tok.col, 3);
    }

    // ---- Post-index addressing ----

    #[test]
    fn post_index_tokens() {
        // ldp x29, x30, [sp], #16
        assert_eq!(
            tok_kinds("[sp], #16"),
            vec![
                Tok::LBracket,
                Tok::Ident("sp".into()),
                Tok::RBracket,
                Tok::Comma,
                Tok::Integer(16),
            ]
        );
    }

    // ---- Conditional branch ----

    #[test]
    fn b_dot_eq() {
        let kinds = tok_kinds("b.eq #8");
        assert_eq!(kinds, vec![Tok::Ident("b.eq".into()), Tok::Integer(8),]);
    }
}
