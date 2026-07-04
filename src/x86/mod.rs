//! x86_64 assembly support (x14): registers, operands, AT&T parsing,
//! and instruction encoding, feeding the x13 ELF writer.
//!
//! Parallel to the ARM64 modules by design — no shared encoder or
//! parser traits. The supported surface starts at exactly what the
//! armfortas x86_64 backend emits (99 mnemonics, three memory shapes
//! plus RIP-relative, no VEX) and grows via the differential fuzz
//! suites. Unsupported forms fail loudly with file/line diagnostics —
//! never assemble silently.

pub mod assemble;
pub mod encode;
pub mod parse;
pub mod reg;

pub use reg::{Reg, RegClass, Width};

/// A parsed AT&T operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand {
    Reg(Reg),
    /// `$imm` — numeric immediate. Symbolic immediates (`$sym`) are
    /// not in the backend dialect; the parser rejects them loudly
    /// until a consumer exists.
    Imm(i64),
    Mem(MemOperand),
    /// `*%rax` / `*8(%rax)` — indirect call/jmp target marker.
    IndirectReg(Reg),
    IndirectMem(MemOperand),
    /// Bare symbol in a branch position (`call foo`, `jmp .L1`).
    Sym(String),
}

/// `disp(base,index,scale)` in all degenerate forms, plus
/// `sym(%rip)` / `sym+disp(%rip)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemOperand {
    pub disp: i64,
    /// RIP-relative symbol, exclusive with base/index.
    pub rip_sym: Option<String>,
    pub base: Option<Reg>,
    pub index: Option<Reg>,
    /// 1, 2, 4, or 8. Meaningful only with an index.
    pub scale: u8,
}

impl MemOperand {
    pub fn base_only(base: Reg) -> Self {
        Self {
            disp: 0,
            rip_sym: None,
            base: Some(base),
            index: None,
            scale: 1,
        }
    }

    pub fn rip(sym: impl Into<String>, disp: i64) -> Self {
        Self {
            disp,
            rip_sym: Some(sym.into()),
            base: None,
            index: None,
            scale: 1,
        }
    }
}
