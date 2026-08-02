//! AT&T-subset parser for the x86_64 dialect the armfortas backend
//! emits (x14 deliverable 2).
//!
//! Self-contained line-oriented tokenizer rather than a reuse of
//! `lex.rs`: the shared lexer is ARM64-flavored (`#` immediates, no
//! `%`/`$`/`*` punctuation) and extending it would touch the
//! Mach-O-path surface for no gain. Diagnostics mirror the house
//! style: line, column, message; `render_with_source` adds the
//! offending line plus a caret.
//!
//! Contract (afs-as README): unsupported forms FAIL with a located
//! diagnostic — nothing assembles silently. Suffix-vs-register width
//! consistency is enforced at encode time, where the mnemonic tables
//! know each form's widths; the parser preserves mnemonics verbatim.

use std::fmt;

use super::{MemOperand, Operand, Reg};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X86ParseError {
    pub line: u32,
    pub col: u32,
    pub msg: String,
}

impl X86ParseError {
    fn new(line: u32, col: u32, msg: impl Into<String>) -> Self {
        Self {
            line,
            col,
            msg: msg.into(),
        }
    }

    /// House-style rendering: `line:col: message`, the source line,
    /// and a caret under the column.
    pub fn render_with_source(&self, src: &str) -> String {
        let text = src.lines().nth(self.line as usize - 1).unwrap_or("");
        let caret = " ".repeat(self.col.saturating_sub(1) as usize) + "^";
        format!(
            "{}:{}: {}\n{}\n{}",
            self.line, self.col, self.msg, text, caret
        )
    }
}

impl fmt::Display for X86ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.line, self.col, self.msg)
    }
}

/// One parsed statement with its 1-based source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub line: u32,
    pub col: u32,
    pub stmt: Stmt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Label(String),
    Insn {
        mnemonic: String,
        operands: Vec<Operand>,
    },
    Directive(Directive),
}

/// `.quad`/`.long`-style data item: number or symbol (+/- constant).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataItem {
    Num(i64),
    Sym { name: String, addend: i64 },
}

/// `.size` argument: absolute or the idiomatic `.-sym`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeArg {
    Const(u64),
    DotMinus(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymKind {
    Function,
    Object,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Directive {
    /// Switch to an ELF section and one of its assembler-only subsection
    /// streams. Bare `.text`, `.data`, `.bss`, and `.section` switches use
    /// subsection zero.
    Section {
        name: String,
        subsection: u32,
    },
    Globl(String),
    Extern(String),
    Local(String),
    Weak(String),
    Type {
        sym: String,
        kind: SymKind,
    },
    Size {
        sym: String,
        arg: SizeArg,
    },
    P2Align {
        pow: u32,
        /// Explicit fill byte. `None` selects section-specific default fill.
        fill: Option<u8>,
        /// `.p2align N,,M`: skip the alignment when padding would exceed M.
        max_skip: Option<u64>,
    },
    Byte(Vec<DataItem>),
    Short(Vec<DataItem>),
    Long(Vec<DataItem>),
    Quad(Vec<DataItem>),
    Ascii(Vec<Vec<u8>>),
    Asciz(Vec<Vec<u8>>),
    Space {
        size: u64,
        fill: u8,
    },
    Zero(u64),
    Comm {
        sym: String,
        size: u64,
        /// `None` selects the ELF destination default. GNU `as` also treats
        /// an explicit zero alignment as this omitted form.
        align: Option<u64>,
    },
    File(String),
    NoteGnuStack {
        executable: bool,
    },
}

pub fn parse(src: &str) -> Result<Vec<Located>, X86ParseError> {
    parse_bytes(src.as_bytes())
}

/// Parse raw assembler bytes. Grammar outside comments and string literals
/// must still be valid text, but literal payloads are preserved byte-for-byte.
pub fn parse_bytes(src: &[u8]) -> Result<Vec<Located>, X86ParseError> {
    let mut out = Vec::new();
    for (idx, raw) in src.split(|byte| *byte == b'\n').enumerate() {
        let line_no = idx as u32 + 1;
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let mut line = strip_comment_bytes(raw);
        loop {
            line = trim_ascii_start(line);
            if line.is_empty() {
                break;
            }
            // Labels: `name:` possibly followed by more on the line.
            if let Some((label, rest)) = split_label_bytes(line) {
                out.push(Located {
                    line: line_no,
                    col: col_of_bytes(raw, line),
                    stmt: Stmt::Label(
                        std::str::from_utf8(label)
                            .expect("label scanner only accepts ASCII")
                            .to_string(),
                    ),
                });
                line = rest;
                continue;
            }
            let col = col_of_bytes(raw, line);
            let stmt = if let Some(rest) = line.strip_prefix(b".") {
                parse_directive_bytes(rest, line_no, col)?
            } else {
                let line = decode_grammar(line, line_no, col, "instruction")?;
                parse_insn(line, line_no, col)?
            };
            out.push(Located {
                line: line_no,
                col,
                stmt,
            });
            break;
        }
    }
    Ok(out)
}

fn col_of_bytes(raw: &[u8], rest: &[u8]) -> u32 {
    (raw.len() - rest.len()) as u32 + 1
}

fn trim_ascii_start(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    bytes
}

fn trim_ascii_end(mut bytes: &[u8]) -> &[u8] {
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    trim_ascii_end(trim_ascii_start(bytes))
}

fn decode_grammar<'a>(
    bytes: &'a [u8],
    line: u32,
    col: u32,
    context: &str,
) -> Result<&'a str, X86ParseError> {
    std::str::from_utf8(bytes).map_err(|error| {
        X86ParseError::new(
            line,
            col.saturating_add(error.valid_up_to() as u32),
            format!("non-UTF-8 byte in {context}"),
        )
    })
}

fn strip_comment_bytes(line: &[u8]) -> &[u8] {
    // `#` starts a comment outside string literals. The backend never
    // emits `#` inside operands (AT&T immediates use `$`).
    let mut in_str = false;
    let mut esc = false;
    for (i, &byte) in line.iter().enumerate() {
        match byte {
            b'\\' if in_str => esc = !esc,
            b'"' if !esc => in_str = !in_str,
            b'#' if !in_str => return &line[..i],
            _ => esc = false,
        }
    }
    line
}

fn split_label_bytes(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let mut end = 0;
    while end < line.len() {
        let c = line[end] as char;
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$' {
            end += 1;
        } else {
            break;
        }
    }
    if end == 0 || end >= line.len() || line[end] != b':' {
        return None;
    }
    Some((&line[..end], &line[end + 1..]))
}

// -------------------------------------------------------------------
// Instructions
// -------------------------------------------------------------------

fn parse_insn(line: &str, line_no: u32, col: u32) -> Result<Stmt, X86ParseError> {
    let line = line.trim();
    let (mnemonic, rest) = match line.find(char::is_whitespace) {
        Some(i) => (&line[..i], line[i..].trim()),
        None => (line, ""),
    };
    if mnemonic.is_empty() {
        return Err(X86ParseError::new(line_no, col, "empty instruction"));
    }
    let mut operands = Vec::new();
    if !rest.is_empty() {
        for piece in split_operands(rest) {
            let piece = piece.trim();
            if piece.is_empty() {
                return Err(X86ParseError::new(line_no, col, "empty operand"));
            }
            operands.push(parse_operand(piece, line_no, col)?);
        }
    }
    Ok(Stmt::Insn {
        mnemonic: mnemonic.to_string(),
        operands,
    })
}

/// Split on top-level commas — commas inside `(...)` belong to the
/// memory operand.
fn split_operands(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

fn parse_operand(s: &str, line: u32, col: u32) -> Result<Operand, X86ParseError> {
    let err = |msg: String| X86ParseError::new(line, col, msg);

    if let Some(rest) = s.strip_prefix('*') {
        // Indirect branch target: register or memory.
        let inner = parse_operand(rest.trim(), line, col)?;
        return match inner {
            Operand::Reg(r) => Ok(Operand::IndirectReg(r)),
            Operand::Mem(m) => Ok(Operand::IndirectMem(m)),
            _ => Err(err(format!("invalid indirect operand '{}'", s))),
        };
    }
    if let Some(rest) = s.strip_prefix('%') {
        return Reg::parse(rest)
            .map(Operand::Reg)
            .ok_or_else(|| err(format!("unknown register '%{}'", rest)));
    }
    if let Some(rest) = s.strip_prefix('$') {
        let v = parse_int(rest)
            .ok_or_else(|| err(format!("unsupported immediate '${}' (numeric only)", rest)))?;
        return Ok(Operand::Imm(v));
    }
    if s.contains('(') {
        return parse_mem(s, line, col).map(Operand::Mem);
    }
    // Bare symbol (branch target) or bare integer displacement-less
    // absolute — the backend emits only symbols here.
    if parse_int(s).is_some() {
        return Err(err(format!(
            "bare integer operand '{}' unsupported (expected register, $imm, memory, or symbol)",
            s
        )));
    }
    if is_symbolish(s) {
        return Ok(Operand::Sym(s.to_string()));
    }
    Err(err(format!("unrecognized operand '{}'", s)))
}

fn parse_mem(s: &str, line: u32, col: u32) -> Result<MemOperand, X86ParseError> {
    let err = |msg: String| X86ParseError::new(line, col, msg);
    let open = s.find('(').unwrap();
    let close = s
        .rfind(')')
        .ok_or_else(|| err(format!("unterminated memory operand '{}'", s)))?;
    if close != s.len() - 1 {
        return Err(err(format!("trailing junk after ')' in '{}'", s)));
    }
    let disp_part = s[..open].trim();
    let inner = &s[open + 1..close];

    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() > 3 {
        return Err(err(format!("too many components in '{}'", s)));
    }

    // RIP-relative: `sym(%rip)` / `sym+4(%rip)` / `.LC0(%rip)`.
    if parts.len() == 1 && parts[0] == "%rip" {
        let (sym, addend) = split_sym_addend(disp_part)
            .ok_or_else(|| err(format!("bad RIP-relative displacement '{}'", disp_part)))?;
        return Ok(MemOperand {
            disp: addend,
            rip_sym: Some(sym),
            base: None,
            index: None,
            scale: 1,
        });
    }

    let disp = if disp_part.is_empty() {
        0
    } else {
        parse_int(disp_part)
            .ok_or_else(|| err(format!("non-numeric displacement '{}'", disp_part)))?
    };

    let reg_of = |t: &str| -> Result<Option<Reg>, X86ParseError> {
        if t.is_empty() {
            return Ok(None);
        }
        let name = t
            .strip_prefix('%')
            .ok_or_else(|| err(format!("expected register, got '{}'", t)))?;
        Reg::parse(name)
            .map(Some)
            .ok_or_else(|| err(format!("unknown register '%{}'", name)))
    };

    let base = reg_of(parts[0])?;
    let index = if parts.len() >= 2 {
        reg_of(parts[1])?
    } else {
        None
    };
    let scale = if parts.len() == 3 {
        match parts[2] {
            "1" => 1,
            "2" => 2,
            "4" => 4,
            "8" => 8,
            other => return Err(err(format!("invalid scale '{}'", other))),
        }
    } else {
        1
    };
    if parts.len() == 3 && index.is_none() {
        return Err(err(format!(
            "memory operand '{}' has a scale but no index",
            s
        )));
    }
    if parts.len() >= 2 && index.is_none() && !parts[1].is_empty() {
        return Err(err(format!("bad index in '{}'", s)));
    }
    if base.is_none() && index.is_none() {
        return Err(err(format!(
            "memory operand '{}' has neither base nor index",
            s
        )));
    }
    Ok(MemOperand {
        disp,
        rip_sym: None,
        base,
        index,
        scale,
    })
}

fn split_sym_addend(s: &str) -> Option<(String, i64)> {
    if s.is_empty() {
        return None;
    }
    // sym, sym+N, sym-N
    for (i, c) in s.char_indices().skip(1) {
        if c == '+' || c == '-' {
            let sym = &s[..i];
            if !is_symbolish(sym) {
                return None;
            }
            let addend = parse_int(&s[i..])?;
            return Some((sym.to_string(), addend));
        }
    }
    is_symbolish(s).then(|| (s.to_string(), 0))
}

fn is_symbolish(s: &str) -> bool {
    // No '@': `foo@PLT` / `foo@GOTPCREL` must NOT parse as a symbol
    // literally named "foo@PLT" — that would silently create the wrong
    // symbol. When the backend ever emits modifiers, they get parsed
    // into an explicit reloc-kind field; until then they fail loudly.
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '$')
        && !s.starts_with(|c: char| c.is_ascii_digit())
}

fn parse_int(s: &str) -> Option<i64> {
    let s = s.trim();
    let (neg, rest) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s),
    };
    // Parse the magnitude as u64 so `-9223372036854775808` (i64::MIN,
    // whose magnitude overflows i64) and full-range hex both work;
    // fold to i64 bits.
    let mag = if let Some(hex) = rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()?
    } else {
        rest.parse::<u64>().ok()?
    };
    if neg {
        if mag > 1u64 << 63 {
            return None;
        }
        Some((mag as i64).wrapping_neg())
    } else {
        Some(mag as i64)
    }
}

fn parse_subsection(args: &str) -> Result<u32, String> {
    const MAX_SUBSECTION: i64 = 8192;

    let args = args.trim();
    if args.is_empty() {
        return Ok(0);
    }
    let value = parse_int(args).ok_or_else(|| {
        format!("subsection must be an integer literal from 0 to {MAX_SUBSECTION}, got '{args}'")
    })?;
    if !(0..=MAX_SUBSECTION).contains(&value) {
        return Err(format!(
            "subsection must be an integer literal from 0 to {MAX_SUBSECTION}, got '{args}'"
        ));
    }
    Ok(value as u32)
}

// -------------------------------------------------------------------
// Directives
// -------------------------------------------------------------------

fn parse_directive_bytes(rest: &[u8], line: u32, col: u32) -> Result<Stmt, X86ParseError> {
    let name_end = rest
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(rest.len());
    let name = decode_grammar(&rest[..name_end], line, col + 1, "directive name")?;
    let args = trim_ascii_start(&rest[name_end..]);
    let err = |msg: String| X86ParseError::new(line, col, msg);

    let directive = match name {
        "ascii" => Directive::Ascii(parse_string_operands_bytes(args).map_err(&err)?),
        "asciz" | "string" => Directive::Asciz(parse_string_operands_bytes(args).map_err(&err)?),
        _ => {
            let rest = decode_grammar(rest, line, col + 1, "directive")?;
            return parse_directive(rest, line, col);
        }
    };
    Ok(Stmt::Directive(directive))
}

fn parse_directive(rest: &str, line: u32, col: u32) -> Result<Stmt, X86ParseError> {
    let err = |msg: String| X86ParseError::new(line, col, msg);
    let (name, args) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    let one_sym = |args: &str| -> Result<String, X86ParseError> {
        let s = args.trim();
        if is_symbolish(s) {
            Ok(s.to_string())
        } else {
            Err(err(format!("expected symbol, got '{}'", s)))
        }
    };
    let data_items = |args: &str| -> Result<Vec<DataItem>, X86ParseError> {
        args.split(',')
            .map(|p| {
                let p = p.trim();
                if let Some(v) = parse_int(p) {
                    return Ok(DataItem::Num(v));
                }
                if let Some((sym, addend)) = split_sym_addend(p) {
                    return Ok(DataItem::Sym { name: sym, addend });
                }
                Err(err(format!("bad data item '{}'", p)))
            })
            .collect()
    };

    let d = match name {
        "text" | "data" | "bss" => Directive::Section {
            name: format!(".{name}"),
            subsection: parse_subsection(args).map_err(&err)?,
        },
        "section" => {
            let mut fields = args.split(',').map(str::trim);
            let sec = fields.next().unwrap_or("").to_string();
            if sec == ".note.GNU-stack" {
                let executable = match fields.next() {
                    None | Some("\"\"") => false,
                    Some("\"x\"") => true,
                    Some(flags) => {
                        return Err(err(format!(
                            "unsupported .note.GNU-stack flags '{}'",
                            flags
                        )))
                    }
                };
                if let Some(kind) = fields.next() {
                    if kind != "@progbits" && kind != "%progbits" {
                        return Err(err(format!("unsupported .note.GNU-stack type '{}'", kind)));
                    }
                }
                if let Some(extra) = fields.next() {
                    return Err(err(format!(
                        "unexpected .note.GNU-stack argument '{}'",
                        extra
                    )));
                }
                Directive::NoteGnuStack { executable }
            } else if sec == ".rodata" || sec == ".text" || sec == ".data" || sec == ".bss" {
                Directive::Section {
                    name: sec,
                    subsection: 0,
                }
            } else {
                return Err(err(format!("unsupported section '{}'", sec)));
            }
        }
        "globl" | "global" => Directive::Globl(one_sym(args)?),
        "extern" => Directive::Extern(one_sym(args)?),
        "local" => Directive::Local(one_sym(args)?),
        "weak" => Directive::Weak(one_sym(args)?),
        "type" => {
            let mut it = args.split(',').map(str::trim);
            let sym = one_sym(it.next().unwrap_or(""))?;
            let kind = match it.next() {
                Some("@function") => SymKind::Function,
                Some("@object") => SymKind::Object,
                other => return Err(err(format!("unsupported .type kind {:?}", other))),
            };
            Directive::Type { sym, kind }
        }
        "size" => {
            let mut it = args.splitn(2, ',').map(str::trim);
            let sym = one_sym(it.next().unwrap_or(""))?;
            let arg_s = it.next().unwrap_or("");
            let arg = if let Some(rest) = arg_s.strip_prefix(".-") {
                SizeArg::DotMinus(rest.trim().to_string())
            } else if let Some(v) = parse_int(arg_s) {
                SizeArg::Const(v as u64)
            } else {
                return Err(err(format!("unsupported .size argument '{}'", arg_s)));
            };
            Directive::Size { sym, arg }
        }
        "p2align" => {
            // `.p2align pow[,fill[,max_skip]]`. GNU as truncates an explicit
            // fill expression to one byte. A trailing empty fill means zero,
            // while an empty placeholder before max-skip keeps default fill.
            let mut parts = args.split(',');
            let pow = parts.next().and_then(parse_int_opt);
            let fill_arg = parts.next();
            let max_skip_arg = parts.next();
            let fill = match fill_arg {
                None => None,
                Some(s) if s.trim().is_empty() && max_skip_arg.is_some() => None,
                Some(s) if s.trim().is_empty() => Some(0),
                Some(s) => Some(
                    parse_int_opt(s)
                        .ok_or_else(|| err(format!("bad .p2align fill '{}'", s.trim())))?
                        as u8,
                ),
            };
            let max_skip = match max_skip_arg {
                None => None,
                Some(s) if s.trim().is_empty() => None,
                Some(s) => Some(
                    parse_int_opt(s)
                        .and_then(|m| u64::try_from(m).ok())
                        .ok_or_else(|| err(format!("bad .p2align max-skip '{}'", s.trim())))?,
                ),
            };
            if parts.next().is_some() {
                return Err(err(format!("bad .p2align '{}'", args)));
            }
            match pow {
                Some(v) if (0..=16).contains(&v) => Directive::P2Align {
                    pow: v as u32,
                    fill,
                    max_skip,
                },
                _ => return Err(err(format!("bad .p2align '{}'", args))),
            }
        }
        "byte" => Directive::Byte(data_items(args)?),
        "short" | "word" | "value" => Directive::Short(data_items(args)?),
        "long" => Directive::Long(data_items(args)?),
        "quad" => Directive::Quad(data_items(args)?),
        "ascii" => Directive::Ascii(parse_string_operands(args).map_err(&err)?),
        "asciz" | "string" => Directive::Asciz(parse_string_operands(args).map_err(&err)?),
        "space" | "skip" => {
            let mut parts = args.split(',').map(str::trim);
            let v = parse_int(parts.next().unwrap_or(""))
                .ok_or_else(|| err(format!("bad {} size '{}'", name, args)))?;
            if v < 0 {
                return Err(err(format!("negative {} size", name)));
            }
            let fill = match parts.next().filter(|s| !s.is_empty()) {
                Some(fill) => parse_int(fill)
                    .ok_or_else(|| err(format!("bad {} fill '{}'", name, args)))?
                    as u8,
                None => 0,
            };
            if parts.any(|s| !s.is_empty()) {
                return Err(err(format!("bad {} operands '{}'", name, args)));
            }
            Directive::Space {
                size: v as u64,
                fill,
            }
        }
        "zero" => {
            let mut parts = args.split(',').map(str::trim);
            let v = parse_int(parts.next().unwrap_or(""))
                .ok_or_else(|| err(format!("bad {} size '{}'", name, args)))?;
            if v < 0 {
                return Err(err(format!("negative {} size", name)));
            }
            let fill = match parts.next() {
                Some("") | None => 0,
                Some(fill) => parse_int(fill)
                    .ok_or_else(|| err(format!("bad {} fill '{}'", name, args)))?
                    as u8,
            };
            if parts.next().is_some() {
                return Err(err(format!("bad {} operands '{}'", name, args)));
            }
            if fill == 0 {
                Directive::Zero(v as u64)
            } else {
                Directive::Space {
                    size: v as u64,
                    fill,
                }
            }
        }
        "comm" => {
            let mut it = args.split(',').map(str::trim);
            let sym = one_sym(it.next().unwrap_or(""))?;
            let size = parse_int(it.next().unwrap_or(""))
                .filter(|v| *v >= 0)
                .ok_or_else(|| err(format!("bad .comm size in '{}'", args)))?
                as u64;
            let align = match it.next() {
                None => None,
                Some(a) => match parse_int(a) {
                    Some(0) => None,
                    Some(value) if value > 0 => Some(value as u64),
                    _ => return Err(err(format!("bad .comm align in '{}'", args))),
                },
            };
            Directive::Comm { sym, size, align }
        }
        "file" => Directive::File(parse_file_name(args).map_err(&err)?),
        other => {
            return Err(err(format!(
                "unsupported directive '.{}' — the x86 dialect grows only with corpus evidence",
                other
            )))
        }
    };
    Ok(Stmt::Directive(d))
}

fn parse_int_opt(s: &str) -> Option<i64> {
    parse_int(s)
}

/// Parse the string operand groups accepted by GNU as. Commas terminate
/// groups, while adjacent literals belong to one group and therefore share
/// one terminator under `.asciz`/`.string`. Empty comma fields are ignored.
fn parse_string_operands(s: &str) -> Result<Vec<Vec<u8>>, String> {
    parse_string_operands_bytes(s.as_bytes())
}

fn parse_file_name(s: &str) -> Result<String, String> {
    let source = trim_ascii(s.as_bytes());
    let (literal, rest) = split_leading_string_literal(source)
        .map_err(|_| ".file requires one quoted file name".to_string())?;
    if !trim_ascii(rest).is_empty() {
        return Err(".file requires one quoted file name".into());
    }
    String::from_utf8(parse_string_lit_bytes(literal)?)
        .map_err(|_| ".file name is not valid UTF-8".into())
}

fn split_leading_string_literal(s: &[u8]) -> Result<(&[u8], &[u8]), String> {
    if !s.starts_with(b"\"") {
        return Err(format!(
            "expected string literal, got '{}'",
            String::from_utf8_lossy(s)
        ));
    }

    let mut escaped = false;
    for (index, &byte) in s.iter().enumerate().skip(1) {
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            let end = index + 1;
            return Ok((&s[..end], &s[end..]));
        }
    }
    Err("unterminated string literal".into())
}

fn parse_string_operands_bytes(s: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let mut rest = trim_ascii(s);
    let mut operands = Vec::new();
    let mut current = Vec::new();
    let mut have_current = false;

    while !rest.is_empty() {
        rest = trim_ascii_start(rest);
        if let Some(after_comma) = rest.strip_prefix(b",") {
            if have_current {
                operands.push(std::mem::take(&mut current));
                have_current = false;
            }
            rest = after_comma;
            continue;
        }
        let (literal, after_literal) = split_leading_string_literal(rest)?;
        current.extend_from_slice(&parse_string_lit_bytes(literal)?);
        have_current = true;
        rest = trim_ascii_start(after_literal);
        if !rest.is_empty() && !rest.starts_with(b",") && !rest.starts_with(b"\"") {
            return Err(format!(
                "expected ',' or string literal, got '{}'",
                String::from_utf8_lossy(rest)
            ));
        }
    }

    if have_current {
        operands.push(current);
    }
    Ok(operands)
}

/// Decode one double-quoted literal with the gas escapes the backend emits.
fn parse_string_lit_bytes(s: &[u8]) -> Result<Vec<u8>, String> {
    let s = trim_ascii(s);
    let inner = s
        .strip_prefix(b"\"")
        .and_then(|bytes| bytes.strip_suffix(b"\""))
        .ok_or_else(|| {
            format!(
                "expected string literal, got '{}'",
                String::from_utf8_lossy(s)
            )
        })?;
    let mut out = Vec::with_capacity(inner.len());
    let mut index = 0;
    while index < inner.len() {
        let byte = inner[index];
        index += 1;
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        if index == inner.len() {
            return Err("dangling backslash".into());
        }
        let escape = inner[index];
        index += 1;
        match escape {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'f' => out.push(0x0c),
            b'b' => out.push(0x08),
            // GNU as does not treat `\a` as BEL in x86 string directives;
            // it drops the backslash and emits the literal `a`.
            b'a' => out.push(b'a'),
            b'v' => out.push(0x0b),
            b'\\' => out.push(b'\\'),
            b'"' => out.push(b'"'),
            b'\'' => out.push(b'\''),
            // Octal: 1-3 octal digits (gas), low byte. `\0` is just the
            // one-digit case.
            digit @ b'0'..=b'7' => {
                let mut value = u32::from(digit - b'0');
                for _ in 0..2 {
                    if index < inner.len() && matches!(inner[index], b'0'..=b'7') {
                        value = value * 8 + u32::from(inner[index] - b'0');
                        index += 1;
                    } else {
                        break;
                    }
                }
                out.push((value & 0xff) as u8);
            }
            // Hex: `\x` then one or more hex digits (gas), low byte.
            b'x' | b'X' => {
                let mut value: u32 = 0;
                let mut any = false;
                while index < inner.len() {
                    let Some(hex) = (inner[index] as char).to_digit(16) else {
                        break;
                    };
                    value = value.wrapping_mul(16).wrapping_add(hex);
                    any = true;
                    index += 1;
                }
                if !any {
                    return Err("\\x used with no following hex digits".into());
                }
                out.push((value & 0xff) as u8);
            }
            other => return Err(format!("unsupported escape '\\{}'", other.escape_ascii())),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x86::{MemOperand, Operand};

    fn one_insn(src: &str) -> (String, Vec<Operand>) {
        let stmts = parse(src).expect(src);
        match &stmts[0].stmt {
            Stmt::Insn { mnemonic, operands } => (mnemonic.clone(), operands.clone()),
            other => panic!("expected insn, got {:?}", other),
        }
    }

    #[test]
    fn operand_shapes_beyond_corpus() {
        // no-base scaled index
        let (_, ops) = one_insn("movl (,%rbx,4), %eax\n");
        assert_eq!(
            ops[0],
            Operand::Mem(MemOperand {
                disp: 0,
                rip_sym: None,
                base: None,
                index: Reg::parse("rbx"),
                scale: 4,
            })
        );
        // full form with disp
        let (_, ops) = one_insn("movq -8(%rbp,%rcx,8), %rax\n");
        match &ops[0] {
            Operand::Mem(m) => {
                assert_eq!((m.disp, m.scale), (-8, 8));
                assert_eq!(m.base, Reg::parse("rbp"));
                assert_eq!(m.index, Reg::parse("rcx"));
            }
            other => panic!("{:?}", other),
        }
        // rip with addend
        let (_, ops) = one_insn("leaq tbl+16(%rip), %rdx\n");
        assert_eq!(ops[0], Operand::Mem(MemOperand::rip("tbl", 16)));
        // indirect branch forms
        let (_, ops) = one_insn("callq *%r11\n");
        assert!(matches!(ops[0], Operand::IndirectReg(_)));
        let (_, ops) = one_insn("jmp *16(%rax)\n");
        assert!(matches!(ops[0], Operand::IndirectMem(_)));
        // i64::MIN immediate
        let (_, ops) = one_insn("movabsq $-9223372036854775808, %rax\n");
        assert_eq!(ops[0], Operand::Imm(i64::MIN));
        // bare symbol branch
        let (m, ops) = one_insn("call afs_error_stop\n");
        assert_eq!(m, "call");
        assert_eq!(ops[0], Operand::Sym("afs_error_stop".into()));
    }

    #[test]
    fn labels_and_multiple_per_line() {
        let stmts = parse(".L1:\n.text\nf: ret\n").unwrap();
        assert_eq!(stmts[0].stmt, Stmt::Label(".L1".into()));
        assert_eq!(stmts[2].stmt, Stmt::Label("f".into()));
        assert!(matches!(&stmts[3].stmt, Stmt::Insn { mnemonic, .. } if mnemonic == "ret"));
    }

    #[test]
    fn directive_forms() {
        let stmts = parse(
            ".comm blk_,1024,32\n.size f,.-f\n.quad tbl+8\n.asciz \"hi\\n\"\n.p2align 4\n.extern ext\n",
        )
        .unwrap();
        assert_eq!(
            stmts[0].stmt,
            Stmt::Directive(Directive::Comm {
                sym: "blk_".into(),
                size: 1024,
                align: Some(32)
            })
        );
        assert_eq!(
            stmts[1].stmt,
            Stmt::Directive(Directive::Size {
                sym: "f".into(),
                arg: SizeArg::DotMinus("f".into())
            })
        );
        assert_eq!(
            stmts[2].stmt,
            Stmt::Directive(Directive::Quad(vec![DataItem::Sym {
                name: "tbl".into(),
                addend: 8
            }]))
        );
        assert_eq!(
            stmts[3].stmt,
            Stmt::Directive(Directive::Asciz(vec![b"hi\n".to_vec()]))
        );
        assert_eq!(
            stmts[5].stmt,
            Stmt::Directive(Directive::Extern("ext".into()))
        );
    }

    #[test]
    fn raw_bytes_survive_string_literals_and_comments() {
        let stmts = parse_bytes(b".ascii \"\xff\"\n# ignored \xfe\n").unwrap();
        assert_eq!(
            stmts,
            [Located {
                line: 1,
                col: 1,
                stmt: Stmt::Directive(Directive::Ascii(vec![vec![0xff]])),
            }]
        );

        let error = parse_bytes(b"ret\n\xff\n").unwrap_err();
        assert_eq!((error.line, error.col), (2, 1));
        assert_eq!(error.msg, "non-UTF-8 byte in instruction");
    }

    #[test]
    fn section_switches_preserve_documented_subsection_numbers() {
        let stmts = parse(".text\n.data 2\n.bss 0x3\n.text 8192\n").unwrap();
        for (statement, name, subsection) in [
            (&stmts[0].stmt, ".text", 0),
            (&stmts[1].stmt, ".data", 2),
            (&stmts[2].stmt, ".bss", 3),
            (&stmts[3].stmt, ".text", 8192),
        ] {
            assert_eq!(
                statement,
                &Stmt::Directive(Directive::Section {
                    name: name.into(),
                    subsection,
                })
            );
        }
    }

    #[test]
    fn invalid_subsection_arguments_fail_loudly() {
        for argument in ["-1", "8193", "1+2", "1,2", "name"] {
            let source = format!(".text {argument}\n");
            let error = parse(&source).expect_err("invalid subsection must be rejected");
            assert_eq!((error.line, error.col), (1, 1));
            assert_eq!(
                error.msg,
                format!("subsection must be an integer literal from 0 to 8192, got '{argument}'")
            );
        }
    }

    #[test]
    fn gnu_stack_section_flags_are_preserved() {
        let stmts = parse(
            ".section .note.GNU-stack\n\
             .section .note.GNU-stack,\"\",@progbits\n\
             .section .note.GNU-stack,\"x\",%progbits\n",
        )
        .unwrap();
        assert_eq!(
            stmts[0].stmt,
            Stmt::Directive(Directive::NoteGnuStack { executable: false })
        );
        assert_eq!(
            stmts[1].stmt,
            Stmt::Directive(Directive::NoteGnuStack { executable: false })
        );
        assert_eq!(
            stmts[2].stmt,
            Stmt::Directive(Directive::NoteGnuStack { executable: true })
        );

        for source in [
            ".section .note.GNU-stack,\"w\",@progbits\n",
            ".section .note.GNU-stack,\"x\",@nobits\n",
            ".section .note.GNU-stack,\"x\",@progbits,extra\n",
        ] {
            assert!(parse(source).is_err(), "must reject {source:?}");
        }
    }

    #[test]
    fn space_and_skip_directives_preserve_fill_byte() {
        let stmts = parse(".space 4, 0x90\n.skip 3, 0xab\n.zero 2\n.zero 5, 0xa5\n").unwrap();
        assert_eq!(
            stmts[0].stmt,
            Stmt::Directive(Directive::Space {
                size: 4,
                fill: 0x90
            })
        );
        assert_eq!(
            stmts[1].stmt,
            Stmt::Directive(Directive::Space {
                size: 3,
                fill: 0xab
            })
        );
        assert_eq!(stmts[2].stmt, Stmt::Directive(Directive::Zero(2)));
        assert_eq!(
            stmts[3].stmt,
            Stmt::Directive(Directive::Space {
                size: 5,
                fill: 0xa5
            })
        );
    }

    #[test]
    fn zero_rejects_a_third_operand() {
        for source in [".zero 4,0xa5,0x7f\n", ".zero 4,0xa5,\n"] {
            assert!(parse(source).is_err(), "must reject {source:?}");
        }
    }

    #[test]
    fn silent_assembly_is_impossible() {
        for bad in [
            "frobnicate %rax\n.data\n", // unknown mnemonic still parses (encoder rejects); directives must not
            ".unknown_directive foo\n",
            "movq $sym, %rax\n",
            "movq 8(%rax junk\n",
        ] {
            if bad.starts_with("frobnicate") {
                // Unknown MNEMONICS are the encoder's job to reject —
                // the parser records them faithfully.
                assert!(parse(bad).is_ok());
            } else {
                assert!(parse(bad).is_err(), "{:?} must not parse", bad);
            }
        }
    }
}
