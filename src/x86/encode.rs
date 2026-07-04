//! x86_64 instruction encoder (x14 deliverable 1), first slice.
//!
//! Table-driven where the ISA is regular (the arith group, setcc,
//! jcc), explicit where it is not (mov family, movabs, shifts).
//! Coverage grows family-by-family with the gas byte-differential as
//! referee; anything not in the tables errors loudly.
//!
//! Encoding walk per instruction: legacy prefixes (66 operand-size,
//! F2/F3 scalar-SSE), REX (W/R/X/B — synthesized from operand widths
//! and register numbers, honoring forced-REX byte registers and
//! rejecting REX-forbidden high-8 combinations), opcode (one-byte or
//! 0F map), ModRM, optional SIB, displacement, immediate. Branches to
//! labels are emitted as rel32 placeholders; the assembler's
//! relaxation pass (separate) may shrink intra-section jumps.

use super::reg::{Reg, RegClass, Width};
use super::{MemOperand, Operand};

/// A relocation the encoded bytes need, offset relative to the start
/// of this instruction's bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsnReloc {
    pub offset: u32,
    pub sym: String,
    pub r_type: u32,
    pub addend: i64,
}

/// A branch to a local label, patched by the assembler once layout is
/// known. `disp_offset` locates the rel32 field inside `bytes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LabelFix {
    pub label: String,
    pub disp_offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub reloc: Option<InsnReloc>,
    pub label_fix: Option<LabelFix>,
}

pub type EncodeResult = Result<Encoded, String>;

use super::super::elf::reloc::x86_64::{R_X86_64_PC32, R_X86_64_PLT32};

// -------------------------------------------------------------------
// REX / ModRM / SIB machinery
// -------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct Rex {
    w: bool,
    r: bool,
    x: bool,
    b: bool,
    /// spl/bpl/sil/dil present: emit 0x40 even with no bits set.
    forced: bool,
    /// ah/ch/dh/bh present: any REX is an error.
    forbidden: bool,
}

impl Rex {
    fn merge_reg(&mut self, r: Reg, slot: RexSlot) {
        if r.forces_rex {
            self.forced = true;
        }
        if r.rex_forbidden() {
            self.forbidden = true;
        }
        let bit = r.rex_bit();
        match slot {
            RexSlot::R => self.r |= bit,
            RexSlot::B => self.b |= bit,
            RexSlot::X => self.x |= bit,
        }
    }

    fn emit(&self, out: &mut Vec<u8>) -> Result<(), String> {
        let any = self.w || self.r || self.x || self.b || self.forced;
        if any && self.forbidden {
            return Err("high-8 register (%ah/%ch/%dh/%bh) cannot appear in a REX instruction".into());
        }
        if any {
            out.push(
                0x40 | ((self.w as u8) << 3)
                    | ((self.r as u8) << 2)
                    | ((self.x as u8) << 1)
                    | (self.b as u8),
            );
        }
        Ok(())
    }
}

enum RexSlot {
    R,
    B,
    X,
}

/// ModRM+SIB+disp for a memory operand, with `reg_field` occupying
/// the reg slot. Returns the reloc field offset within the produced
/// bytes when RIP-relative.
struct MemEnc {
    bytes: Vec<u8>,
    rip_disp_offset: Option<u32>,
}

fn encode_mem(reg_field: u8, mem: &MemOperand, rex: &mut Rex) -> Result<MemEnc, String> {
    let mut out = Vec::with_capacity(6);
    let reg3 = reg_field & 7;

    if let Some(_sym) = &mem.rip_sym {
        // RIP-relative: mod=00, rm=101, disp32 patched by reloc.
        out.push(reg3 << 3 | 0b101);
        let off = out.len() as u32;
        out.extend_from_slice(&0i32.to_le_bytes());
        return Ok(MemEnc {
            bytes: out,
            rip_disp_offset: Some(off),
        });
    }

    let base = mem.base;
    let index = mem.index;
    if let Some(idx) = index {
        if idx.width != Width::Q {
            return Err(format!("index register {} must be 64-bit", idx));
        }
        if idx.num == 4 && idx.class == RegClass::Gp && !idx.rex_bit() {
            return Err("%rsp cannot be an index register".into());
        }
        rex.merge_reg(idx, RexSlot::X);
    }
    if let Some(b) = base {
        if b.width != Width::Q {
            return Err(format!("base register {} must be 64-bit", b));
        }
        rex.merge_reg(b, RexSlot::B);
    }

    let scale_bits = match mem.scale {
        1 => 0u8,
        2 => 1,
        4 => 2,
        8 => 3,
        s => return Err(format!("invalid scale {}", s)),
    };

    let disp = mem.disp;
    let disp8 = i8::try_from(disp).is_ok();

    match (base, index) {
        (Some(b), None) => {
            let needs_sib = b.low3() == 0b100; // rsp/r12
            // rbp/r13 with no disp still need disp8=0.
            let force_disp8 = disp == 0 && b.low3() == 0b101;
            let (modbits, disp_bytes): (u8, Vec<u8>) = if disp == 0 && !force_disp8 {
                (0b00, vec![])
            } else if disp8 {
                (0b01, vec![disp as i8 as u8])
            } else {
                (0b10, (disp as i32).to_le_bytes().to_vec())
            };
            if needs_sib {
                out.push(modbits << 6 | reg3 << 3 | 0b100);
                out.push(scale_bits << 6 | 0b100 << 3 | b.low3());
            } else {
                out.push(modbits << 6 | reg3 << 3 | b.low3());
            }
            out.extend_from_slice(&disp_bytes);
        }
        (Some(b), Some(idx)) => {
            let force_disp8 = disp == 0 && b.low3() == 0b101;
            let (modbits, disp_bytes): (u8, Vec<u8>) = if disp == 0 && !force_disp8 {
                (0b00, vec![])
            } else if disp8 {
                (0b01, vec![disp as i8 as u8])
            } else {
                (0b10, (disp as i32).to_le_bytes().to_vec())
            };
            out.push(modbits << 6 | reg3 << 3 | 0b100);
            out.push(scale_bits << 6 | idx.low3() << 3 | b.low3());
            out.extend_from_slice(&disp_bytes);
        }
        (None, Some(idx)) => {
            // No base: mod=00, SIB base=101, disp32 mandatory.
            out.push(reg3 << 3 | 0b100);
            out.push(scale_bits << 6 | idx.low3() << 3 | 0b101);
            out.extend_from_slice(&(disp as i32).to_le_bytes());
        }
        (None, None) => return Err("memory operand needs base, index, or %rip".into()),
    }
    Ok(MemEnc {
        bytes: out,
        rip_disp_offset: None,
    })
}

// -------------------------------------------------------------------
// Instruction assembly helpers
// -------------------------------------------------------------------

fn width_of_suffix(mnemonic: &str) -> Option<(&str, Width)> {
    // Trailing b/w/l/q suffix. The caller decides whether a suffix is
    // required for the family.
    let (stem, last) = mnemonic.split_at(mnemonic.len().checked_sub(1)?);
    match last {
        "b" => Some((stem, Width::B)),
        "w" => Some((stem, Width::W)),
        "l" => Some((stem, Width::L)),
        "q" => Some((stem, Width::Q)),
        _ => None,
    }
}

fn gp_reg(op: &Operand) -> Option<Reg> {
    match op {
        Operand::Reg(r) if r.class != RegClass::Xmm => Some(*r),
        _ => None,
    }
}

fn check_width(r: Reg, w: Width, mnemonic: &str) -> Result<(), String> {
    if r.width != w {
        return Err(format!(
            "suffix of '{}' implies {:?} but register {} is {:?}",
            mnemonic, w, r, r.width
        ));
    }
    Ok(())
}

/// Emit opcode bytes for a width family: `base8` for byte forms,
/// `base8+1` otherwise, with 66 prefix for W and REX.W for Q.
fn width_setup(w: Width, rex: &mut Rex, out: &mut Vec<u8>) {
    match w {
        Width::W => out.push(0x66),
        Width::Q => rex.w = true,
        _ => {}
    }
}

struct Parts {
    prefix: Vec<u8>,
    rex: Rex,
    opcode: Vec<u8>,
    tail: Vec<u8>,
    rip_reloc: Option<(u32, String, i64)>, // (offset within tail, sym, user addend)
}

impl Parts {
    fn new() -> Self {
        Parts {
            prefix: Vec::new(),
            rex: Rex::default(),
            opcode: Vec::new(),
            tail: Vec::new(),
            rip_reloc: None,
        }
    }

    fn finish(self) -> EncodeResult {
        let mut bytes = self.prefix;
        self.rex.emit(&mut bytes)?;
        let opcode_start = bytes.len();
        bytes.extend_from_slice(&self.opcode);
        let tail_start = bytes.len();
        let _ = opcode_start;
        bytes.extend_from_slice(&self.tail);
        let mut enc = Encoded {
            bytes,
            reloc: None,
            label_fix: None,
        };
        if let Some((off_in_tail, sym, user_addend)) = self.rip_reloc {
            let field_off = tail_start as u32 + off_in_tail;
            let trailing = enc.bytes.len() as i64 - (field_off as i64 + 4);
            enc.reloc = Some(InsnReloc {
                offset: field_off,
                sym,
                r_type: R_X86_64_PC32,
                addend: user_addend - 4 - trailing,
            });
        }
        Ok(enc)
    }

    fn mem(&mut self, reg_field: u8, m: &MemOperand) -> Result<(), String> {
        let enc = encode_mem(reg_field, m, &mut self.rex)?;
        if let Some(off) = enc.rip_disp_offset {
            let sym = m.rip_sym.clone().expect("rip reloc without sym");
            self.rip_reloc = Some((self.tail.len() as u32 + off, sym, m.disp));
        }
        self.tail.extend_from_slice(&enc.bytes);
        Ok(())
    }

    fn modrm_rr(&mut self, reg: Reg, rm: Reg) {
        self.rex.merge_reg(reg, RexSlot::R);
        self.rex.merge_reg(rm, RexSlot::B);
        self.tail.push(0b11 << 6 | reg.low3() << 3 | rm.low3());
    }
}

// -------------------------------------------------------------------
// Public entry
// -------------------------------------------------------------------

/// Encode one instruction. Branch mnemonics targeting local labels
/// return a `label_fix`; calls/jumps to symbols return a PLT32 reloc
/// with gas's addend bytes already in place.
pub fn encode(mnemonic: &str, ops: &[Operand]) -> EncodeResult {
    match mnemonic {
        "ret" | "retq" => {
            return Ok(Encoded {
                bytes: vec![0xc3],
                ..Default::default()
            })
        }
        "cqto" | "cqo" => {
            return Ok(Encoded {
                bytes: vec![0x48, 0x99],
                ..Default::default()
            })
        }
        "cltd" | "cdq" => {
            return Ok(Encoded {
                bytes: vec![0x99],
                ..Default::default()
            })
        }
        "syscall" => {
            return Ok(Encoded {
                bytes: vec![0x0f, 0x05],
                ..Default::default()
            })
        }
        "call" | "callq" => return encode_call_jmp(ops, 0xe8, "call"),
        "jmp" => return encode_call_jmp(ops, 0xe9, "jmp"),
        _ => {}
    }

    if let Some(cc) = mnemonic.strip_prefix("set") {
        return encode_setcc(cc, ops);
    }
    if let Some(cc) = mnemonic.strip_prefix('j') {
        if cond_code(cc).is_some() {
            return encode_jcc(cc, ops);
        }
    }
    if mnemonic == "pushq" || mnemonic == "popq" {
        let r = gp_reg(ops.first().ok_or("push/pop needs an operand")?)
            .ok_or("push/pop supports register operands only")?;
        check_width(r, Width::Q, mnemonic)?;
        let mut p = Parts::new();
        p.rex.merge_reg(r, RexSlot::B);
        let base = if mnemonic == "pushq" { 0x50 } else { 0x58 };
        p.opcode.push(base + r.low3());
        return p.finish();
    }

    if let Some(rest) = mnemonic.strip_prefix("movabs") {
        // movabsq $imm64, %reg — the only 64-bit immediate.
        if rest != "q" {
            return Err(format!("unsupported movabs form '{}'", mnemonic));
        }
        let (imm, r) = match ops {
            [Operand::Imm(i), Operand::Reg(r)] => (*i, *r),
            _ => return Err("movabsq expects $imm64, %reg".into()),
        };
        check_width(r, Width::Q, mnemonic)?;
        let mut p = Parts::new();
        p.rex.w = true;
        p.rex.merge_reg(r, RexSlot::B);
        p.opcode.push(0xb8 + r.low3());
        p.tail.extend_from_slice(&imm.to_le_bytes());
        return p.finish();
    }

    // mov family with suffix. movq between GP and xmm belongs to the
    // SSE path (66 REX.W 0F 6E/7E), so route on operand class.
    let touches_xmm = ops
        .iter()
        .any(|o| matches!(o, Operand::Reg(r) if r.class == RegClass::Xmm));
    if let Some((stem, w)) = width_of_suffix(mnemonic) {
        match stem {
            "mov" if !touches_xmm => return encode_mov(w, ops, mnemonic),
            "lea" => return encode_lea(w, ops, mnemonic),
            "add" | "or" | "and" | "sub" | "xor" | "cmp" => {
                return encode_arith(stem, w, ops, mnemonic)
            }
            "test" => return encode_test(w, ops, mnemonic),
            "imul" => return encode_imul(w, ops, mnemonic),
            "idiv" | "div" | "neg" | "not" | "mul" => {
                return encode_group3_5(stem, w, ops, mnemonic)
            }
            "shl" | "shr" | "sar" => return encode_shift(stem, w, ops, mnemonic),
            _ => {}
        }
    }

    // Extensions: movzbl, movzbq, movzwl, movswl, movsbl, movslq ...
    if let Some(enc) = encode_extension(mnemonic, ops)? {
        return Ok(enc);
    }

    // SSE (scalar + the x10 packed baseline). Legacy encodings only —
    // the corpus emits zero VEX.
    if let Some(enc) = encode_sse(mnemonic, ops)? {
        return Ok(enc);
    }

    Err(format!(
        "unsupported mnemonic '{}' — grow the encoder with corpus evidence",
        mnemonic
    ))
}

// -------------------------------------------------------------------
// SSE
// -------------------------------------------------------------------

/// Mandatory prefix for an SSE encoding row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sse {
    None,
    P66,
    F2,
    F3,
}

impl Sse {
    fn emit(self, out: &mut Vec<u8>) {
        match self {
            Sse::None => {}
            Sse::P66 => out.push(0x66),
            Sse::F2 => out.push(0xf2),
            Sse::F3 => out.push(0xf3),
        }
    }
}

fn xmm(op: &Operand) -> Option<Reg> {
    match op {
        Operand::Reg(r) if r.class == RegClass::Xmm => Some(*r),
        _ => None,
    }
}

/// xmm/m -> xmm ops that share the load-form shape (dst xmm in the
/// reg field, src is r/m). Table: (mnemonic, prefix, opcode).
const SSE_RM: &[(&str, Sse, u8)] = &[
    // scalar arithmetic
    ("addss", Sse::F3, 0x58),
    ("addsd", Sse::F2, 0x58),
    ("subss", Sse::F3, 0x5c),
    ("subsd", Sse::F2, 0x5c),
    ("mulss", Sse::F3, 0x59),
    ("mulsd", Sse::F2, 0x59),
    ("divss", Sse::F3, 0x5e),
    ("divsd", Sse::F2, 0x5e),
    ("minss", Sse::F3, 0x5d),
    ("minsd", Sse::F2, 0x5d),
    ("maxss", Sse::F3, 0x5f),
    ("maxsd", Sse::F2, 0x5f),
    ("sqrtss", Sse::F3, 0x51),
    ("sqrtsd", Sse::F2, 0x51),
    ("ucomiss", Sse::None, 0x2e),
    ("ucomisd", Sse::P66, 0x2e),
    ("cvtss2sd", Sse::F3, 0x5a),
    ("cvtsd2ss", Sse::F2, 0x5a),
    // packed float
    ("addps", Sse::None, 0x58),
    ("addpd", Sse::P66, 0x58),
    ("subps", Sse::None, 0x5c),
    ("subpd", Sse::P66, 0x5c),
    ("mulps", Sse::None, 0x59),
    ("mulpd", Sse::P66, 0x59),
    ("minps", Sse::None, 0x5d),
    ("minpd", Sse::P66, 0x5d),
    ("maxps", Sse::None, 0x5f),
    ("maxpd", Sse::P66, 0x5f),
    ("andps", Sse::None, 0x54),
    ("andpd", Sse::P66, 0x54),
    ("orps", Sse::None, 0x56),
    ("orpd", Sse::P66, 0x56),
    ("xorps", Sse::None, 0x57),
    ("xorpd", Sse::P66, 0x57),
    // packed integer
    ("paddd", Sse::P66, 0xfe),
    ("psubd", Sse::P66, 0xfa),
    ("pand", Sse::P66, 0xdb),
    ("pandn", Sse::P66, 0xdf),
    ("por", Sse::P66, 0xeb),
    ("pxor", Sse::P66, 0xef),
    ("pcmpgtd", Sse::P66, 0x66),
    ("pmuludq", Sse::P66, 0xf4),
];

/// Moves with distinct load/store opcodes:
/// (mnemonic, prefix, load_op xmm<-r/m, store_op r/m<-xmm).
const SSE_MOV: &[(&str, Sse, u8, u8)] = &[
    ("movss", Sse::F3, 0x10, 0x11),
    ("movsd", Sse::F2, 0x10, 0x11),
    ("movups", Sse::None, 0x10, 0x11),
    ("movaps", Sse::None, 0x28, 0x29),
    ("movdqa", Sse::P66, 0x6f, 0x7f),
    ("movdqu", Sse::F3, 0x6f, 0x7f),
];

fn encode_sse(mnemonic: &str, ops: &[Operand]) -> Result<Option<Encoded>, String> {
    // Shared emitters ------------------------------------------------
    let rm_form = |prefix: Sse, opcode: &[u8], ops: &[Operand]| -> Result<Encoded, String> {
        let mut p = Parts::new();
        prefix.emit(&mut p.prefix);
        match ops {
            [Operand::Reg(src), dst_op] if src.class == RegClass::Xmm => {
                let dst = xmm(dst_op).ok_or("expected xmm destination")?;
                p.rex.merge_reg(dst, RexSlot::R);
                p.rex.merge_reg(*src, RexSlot::B);
                p.opcode.extend_from_slice(opcode);
                p.tail.push(0b11 << 6 | dst.low3() << 3 | src.low3());
            }
            [Operand::Mem(m), dst_op] => {
                let dst = xmm(dst_op).ok_or("expected xmm destination")?;
                p.rex.merge_reg(dst, RexSlot::R);
                p.opcode.extend_from_slice(opcode);
                p.mem(dst.low3(), m)?;
            }
            _ => return Err("expected xmm/mem source, xmm destination".into()),
        }
        p.finish()
    };

    if let Some((_, prefix, opcode)) = SSE_RM.iter().find(|(m, _, _)| *m == mnemonic) {
        return rm_form(*prefix, &[0x0f, *opcode], ops).map(Some);
    }

    if let Some((_, prefix, load, store)) = SSE_MOV.iter().find(|(m, _, _, _)| *m == mnemonic) {
        let mut p = Parts::new();
        prefix.emit(&mut p.prefix);
        match ops {
            // store: xmm -> mem
            [Operand::Reg(src), Operand::Mem(m)] if src.class == RegClass::Xmm => {
                p.rex.merge_reg(*src, RexSlot::R);
                p.opcode.extend_from_slice(&[0x0f, *store]);
                p.mem(src.low3(), m)?;
            }
            // load / reg-reg: gas uses the load opcode for xmm,xmm
            [src_op, Operand::Reg(dst)] if dst.class == RegClass::Xmm => {
                p.rex.merge_reg(*dst, RexSlot::R);
                p.opcode.extend_from_slice(&[0x0f, *load]);
                match src_op {
                    Operand::Reg(src) if src.class == RegClass::Xmm => {
                        p.rex.merge_reg(*src, RexSlot::B);
                        p.tail.push(0b11 << 6 | dst.low3() << 3 | src.low3());
                    }
                    Operand::Mem(m) => p.mem(dst.low3(), m)?,
                    _ => return Err(format!("unsupported {} source", mnemonic)),
                }
            }
            _ => return Err(format!("unsupported {} operands", mnemonic)),
        }
        return p.finish().map(Some);
    }

    // pshufd/shufps carry a trailing imm8: `op $imm, src, dst`.
    if mnemonic == "pshufd" || mnemonic == "shufps" {
        let (imm, src_op, dst_op) = match ops {
            [Operand::Imm(i), s, d] => (*i, s, d),
            _ => return Err(format!("{} expects $imm8, src, dst", mnemonic)),
        };
        let prefix = if mnemonic == "pshufd" { Sse::P66 } else { Sse::None };
        let opcode = if mnemonic == "pshufd" { 0x70 } else { 0xc6 };
        let mut enc = rm_form(prefix, &[0x0f, opcode], &[src_op.clone(), dst_op.clone()])?;
        enc.bytes.push(imm as u8);
        return Ok(Some(enc));
    }

    // movd / movq between GP and xmm: 66 (REX.W) 0F 6E (gp->xmm),
    // 66 (REX.W) 0F 7E (xmm->gp).
    if mnemonic == "movd" || (mnemonic == "movq" && ops.iter().any(|o| xmm(o).is_some())) {
        let wide = mnemonic == "movq";
        let mut p = Parts::new();
        p.prefix.push(0x66);
        if wide {
            p.rex.w = true;
        }
        match ops {
            [Operand::Reg(gp), dst_op] if gp.class == RegClass::Gp => {
                let dst = xmm(dst_op).ok_or("expected xmm destination")?;
                check_width(*gp, if wide { Width::Q } else { Width::L }, mnemonic)?;
                p.rex.merge_reg(dst, RexSlot::R);
                p.rex.merge_reg(*gp, RexSlot::B);
                p.opcode.extend_from_slice(&[0x0f, 0x6e]);
                p.tail.push(0b11 << 6 | dst.low3() << 3 | gp.low3());
            }
            [src_op, Operand::Reg(gp)] if gp.class == RegClass::Gp => {
                let src = xmm(src_op).ok_or("expected xmm source")?;
                check_width(*gp, if wide { Width::Q } else { Width::L }, mnemonic)?;
                p.rex.merge_reg(src, RexSlot::R);
                p.rex.merge_reg(*gp, RexSlot::B);
                p.opcode.extend_from_slice(&[0x0f, 0x7e]);
                p.tail.push(0b11 << 6 | src.low3() << 3 | gp.low3());
            }
            _ => return Err(format!("unsupported {} operands", mnemonic)),
        }
        return p.finish().map(Some);
    }

    // int -> float conversions: cvtsi2ss/sd + l/q suffix.
    for (stem, prefix) in [("cvtsi2ss", Sse::F3), ("cvtsi2sd", Sse::F2)] {
        if let Some(sfx) = mnemonic.strip_prefix(stem) {
            let wide = match sfx {
                "l" | "" => false,
                "q" => true,
                _ => continue,
            };
            let mut p = Parts::new();
            prefix.emit(&mut p.prefix);
            if wide {
                p.rex.w = true;
            }
            match ops {
                [Operand::Reg(gp), dst_op] if gp.class == RegClass::Gp => {
                    let dst = xmm(dst_op).ok_or("expected xmm destination")?;
                    check_width(*gp, if wide { Width::Q } else { Width::L }, mnemonic)?;
                    p.rex.merge_reg(dst, RexSlot::R);
                    p.rex.merge_reg(*gp, RexSlot::B);
                    p.opcode.extend_from_slice(&[0x0f, 0x2a]);
                    p.tail.push(0b11 << 6 | dst.low3() << 3 | gp.low3());
                }
                [Operand::Mem(m), dst_op] => {
                    let dst = xmm(dst_op).ok_or("expected xmm destination")?;
                    p.rex.merge_reg(dst, RexSlot::R);
                    p.opcode.extend_from_slice(&[0x0f, 0x2a]);
                    p.mem(dst.low3(), m)?;
                }
                _ => return Err(format!("unsupported {} operands", mnemonic)),
            }
            return p.finish().map(Some);
        }
    }

    // float -> int truncating conversions: cvttss2si / cvttsd2si + l/q.
    for (stem, prefix) in [("cvttss2si", Sse::F3), ("cvttsd2si", Sse::F2)] {
        if let Some(sfx) = mnemonic.strip_prefix(stem) {
            let wide = match sfx {
                "l" | "" => false,
                "q" => true,
                _ => continue,
            };
            let mut p = Parts::new();
            prefix.emit(&mut p.prefix);
            if wide {
                p.rex.w = true;
            }
            match ops {
                [src_op, Operand::Reg(gp)] if gp.class == RegClass::Gp => {
                    check_width(*gp, if wide { Width::Q } else { Width::L }, mnemonic)?;
                    p.rex.merge_reg(*gp, RexSlot::R);
                    p.opcode.extend_from_slice(&[0x0f, 0x2c]);
                    match src_op {
                        Operand::Reg(x) if x.class == RegClass::Xmm => {
                            p.rex.merge_reg(*x, RexSlot::B);
                            p.tail.push(0b11 << 6 | gp.low3() << 3 | x.low3());
                        }
                        Operand::Mem(m) => p.mem(gp.low3(), m)?,
                        _ => return Err(format!("unsupported {} source", mnemonic)),
                    }
                }
                _ => return Err(format!("unsupported {} operands", mnemonic)),
            }
            return p.finish().map(Some);
        }
    }

    Ok(None)
}

fn cond_code(cc: &str) -> Option<u8> {
    Some(match cc {
        "o" => 0x0,
        "no" => 0x1,
        "b" | "c" | "nae" => 0x2,
        "ae" | "nb" | "nc" => 0x3,
        "e" | "z" => 0x4,
        "ne" | "nz" => 0x5,
        "be" | "na" => 0x6,
        "a" | "nbe" => 0x7,
        "s" => 0x8,
        "ns" => 0x9,
        "p" | "pe" => 0xa,
        "np" | "po" => 0xb,
        "l" | "nge" => 0xc,
        "ge" | "nl" => 0xd,
        "le" | "ng" => 0xe,
        "g" | "nle" => 0xf,
        _ => return None,
    })
}

fn encode_call_jmp(ops: &[Operand], opcode: u8, what: &str) -> EncodeResult {
    match ops {
        [Operand::Sym(sym)] => {
            // gas: call/jmp to an external symbol = rel32 zero field
            // plus a PLT32 reloc with addend -4 (differentially
            // verified against gas 2.44: the field bytes are 00, the
            // addend lives only in the RELA).
            let mut bytes = vec![opcode];
            bytes.extend_from_slice(&0i32.to_le_bytes());
            Ok(Encoded {
                bytes,
                reloc: Some(InsnReloc {
                    offset: 1,
                    sym: sym.clone(),
                    r_type: R_X86_64_PLT32,
                    addend: -4,
                }),
                label_fix: None,
            })
        }
        [Operand::IndirectReg(r)] => {
            let mut p = Parts::new();
            check_width(*r, Width::Q, what)?;
            p.rex.merge_reg(*r, RexSlot::B);
            p.opcode.push(0xff);
            let ext = if opcode == 0xe8 { 2 } else { 4 };
            p.tail.push(0b11 << 6 | ext << 3 | r.low3());
            p.finish()
        }
        [Operand::IndirectMem(m)] => {
            let mut p = Parts::new();
            p.opcode.push(0xff);
            let ext = if opcode == 0xe8 { 2 } else { 4 };
            p.mem(ext, m)?;
            p.finish()
        }
        _ => Err(format!("unsupported {} operand", what)),
    }
}

fn encode_jcc(cc: &str, ops: &[Operand]) -> EncodeResult {
    let code = cond_code(cc).ok_or_else(|| format!("unknown condition '{}'", cc))?;
    match ops {
        [Operand::Sym(label)] => {
            // rel32 placeholder; assembler patches or relaxes.
            let mut bytes = vec![0x0f, 0x80 + code];
            bytes.extend_from_slice(&0i32.to_le_bytes());
            Ok(Encoded {
                bytes,
                reloc: None,
                label_fix: Some(LabelFix {
                    label: label.clone(),
                    disp_offset: 2,
                }),
            })
        }
        _ => Err("jcc expects a label".into()),
    }
}

fn encode_setcc(cc: &str, ops: &[Operand]) -> EncodeResult {
    let code = cond_code(cc).ok_or_else(|| format!("unknown condition '{}'", cc))?;
    let r = gp_reg(ops.first().ok_or("setcc needs an operand")?)
        .ok_or("setcc supports byte registers only")?;
    check_width(r, Width::B, "setcc")?;
    let mut p = Parts::new();
    p.rex.merge_reg(r, RexSlot::B);
    p.opcode.extend_from_slice(&[0x0f, 0x90 + code]);
    p.tail.push(0b11 << 6 | r.low3());
    p.finish()
}

fn encode_mov(w: Width, ops: &[Operand], mnemonic: &str) -> EncodeResult {
    let mut p = Parts::new();
    width_setup(w, &mut p.rex, &mut p.prefix);
    match ops {
        [Operand::Reg(src), Operand::Reg(dst)] => {
            check_width(*src, w, mnemonic)?;
            check_width(*dst, w, mnemonic)?;
            p.opcode.push(if w == Width::B { 0x88 } else { 0x89 });
            p.modrm_rr(*src, *dst);
        }
        [Operand::Reg(src), Operand::Mem(m)] => {
            check_width(*src, w, mnemonic)?;
            p.rex.merge_reg(*src, RexSlot::R);
            p.opcode.push(if w == Width::B { 0x88 } else { 0x89 });
            p.mem(src.low3(), m)?;
        }
        [Operand::Mem(m), Operand::Reg(dst)] => {
            check_width(*dst, w, mnemonic)?;
            p.rex.merge_reg(*dst, RexSlot::R);
            p.opcode.push(if w == Width::B { 0x8a } else { 0x8b });
            p.mem(dst.low3(), m)?;
        }
        [Operand::Imm(imm), Operand::Reg(dst)] => {
            check_width(*dst, w, mnemonic)?;
            match w {
                Width::B => {
                    p.rex.merge_reg(*dst, RexSlot::B);
                    p.opcode.push(0xb0 + dst.low3());
                    p.tail.push(*imm as u8);
                }
                Width::L => {
                    // B8+r imm32 — gas's choice for movl.
                    p.rex.merge_reg(*dst, RexSlot::B);
                    p.opcode.push(0xb8 + dst.low3());
                    p.tail.extend_from_slice(&(*imm as i32).to_le_bytes());
                }
                Width::W => {
                    p.rex.merge_reg(*dst, RexSlot::B);
                    p.opcode.push(0xb8 + dst.low3());
                    p.tail.extend_from_slice(&(*imm as i16).to_le_bytes());
                }
                Width::Q => {
                    // C7 /0 imm32 sign-extended (gas uses this when the
                    // immediate fits i32; movabsq covers the rest).
                    if i32::try_from(*imm).is_err() {
                        return Err("movq immediate does not fit i32; use movabsq".into());
                    }
                    p.rex.merge_reg(*dst, RexSlot::B);
                    p.opcode.push(0xc7);
                    p.tail.push(0b11 << 6 | dst.low3());
                    p.tail.extend_from_slice(&(*imm as i32).to_le_bytes());
                }
                Width::X => unreachable!(),
            }
        }
        [Operand::Imm(imm), Operand::Mem(m)] => {
            p.opcode.push(if w == Width::B { 0xc6 } else { 0xc7 });
            p.mem(0, m)?;
            match w {
                Width::B => p.tail.push(*imm as u8),
                Width::W => p.tail.extend_from_slice(&(*imm as i16).to_le_bytes()),
                _ => {
                    if w == Width::Q && i32::try_from(*imm).is_err() {
                        return Err("mov to memory: immediate does not fit i32".into());
                    }
                    p.tail.extend_from_slice(&(*imm as i32).to_le_bytes());
                }
            }
        }
        _ => return Err(format!("unsupported mov form for '{}'", mnemonic)),
    }
    p.finish()
}

fn encode_lea(w: Width, ops: &[Operand], mnemonic: &str) -> EncodeResult {
    let (m, dst) = match ops {
        [Operand::Mem(m), Operand::Reg(dst)] => (m, *dst),
        _ => return Err("lea expects mem, reg".into()),
    };
    check_width(dst, w, mnemonic)?;
    if w == Width::B {
        return Err("lea has no byte form".into());
    }
    let mut p = Parts::new();
    width_setup(w, &mut p.rex, &mut p.prefix);
    p.rex.merge_reg(dst, RexSlot::R);
    p.opcode.push(0x8d);
    p.mem(dst.low3(), m)?;
    p.finish()
}

/// The classic arith group: add/or/and/sub/xor/cmp share opcode
/// structure; `idx` is the /digit and the opcode row.
fn arith_idx(stem: &str) -> u8 {
    match stem {
        "add" => 0,
        "or" => 1,
        "and" => 4,
        "sub" => 5,
        "xor" => 6,
        "cmp" => 7,
        _ => unreachable!(),
    }
}

fn encode_arith(stem: &str, w: Width, ops: &[Operand], mnemonic: &str) -> EncodeResult {
    let idx = arith_idx(stem);
    let mut p = Parts::new();
    width_setup(w, &mut p.rex, &mut p.prefix);
    match ops {
        [Operand::Reg(src), Operand::Reg(dst)] => {
            check_width(*src, w, mnemonic)?;
            check_width(*dst, w, mnemonic)?;
            p.opcode
                .push(idx * 8 + if w == Width::B { 0x00 } else { 0x01 });
            p.modrm_rr(*src, *dst);
        }
        [Operand::Reg(src), Operand::Mem(m)] => {
            check_width(*src, w, mnemonic)?;
            p.rex.merge_reg(*src, RexSlot::R);
            p.opcode
                .push(idx * 8 + if w == Width::B { 0x00 } else { 0x01 });
            p.mem(src.low3(), m)?;
        }
        [Operand::Mem(m), Operand::Reg(dst)] => {
            check_width(*dst, w, mnemonic)?;
            p.rex.merge_reg(*dst, RexSlot::R);
            p.opcode
                .push(idx * 8 + if w == Width::B { 0x02 } else { 0x03 });
            p.mem(dst.low3(), m)?;
        }
        [Operand::Imm(imm), rm] => {
            let fits8 = i8::try_from(*imm).is_ok();
            // gas prefers the accumulator short forms (04/0C/.../3D)
            // when the destination is al/ax/eax/rax and the immediate
            // doesn't qualify for the sign-extended imm8 group.
            let accumulator = matches!(
                rm,
                Operand::Reg(d) if d.num == 0 && d.class == RegClass::Gp
            );
            if accumulator && (w == Width::B || !fits8) {
                if let Operand::Reg(dst) = rm {
                    check_width(*dst, w, mnemonic)?;
                }
                p.opcode
                    .push(idx * 8 + if w == Width::B { 0x04 } else { 0x05 });
                match w {
                    Width::B => p.tail.push(*imm as u8),
                    Width::W => p.tail.extend_from_slice(&(*imm as i16).to_le_bytes()),
                    _ => {
                        if w == Width::Q && i32::try_from(*imm).is_err() {
                            return Err(format!("{} immediate does not fit i32", mnemonic));
                        }
                        p.tail.extend_from_slice(&(*imm as i32).to_le_bytes());
                    }
                }
                return p.finish();
            }
            let opcode = if w == Width::B {
                0x80
            } else if fits8 {
                0x83
            } else {
                0x81
            };
            p.opcode.push(opcode);
            match rm {
                Operand::Reg(dst) => {
                    check_width(*dst, w, mnemonic)?;
                    p.rex.merge_reg(*dst, RexSlot::B);
                    p.tail.push(0b11 << 6 | idx << 3 | dst.low3());
                }
                Operand::Mem(m) => p.mem(idx, m)?,
                _ => return Err(format!("unsupported {} operands", mnemonic)),
            }
            if w == Width::B || fits8 {
                p.tail.push(*imm as u8);
            } else if w == Width::W {
                p.tail.extend_from_slice(&(*imm as i16).to_le_bytes());
            } else {
                if w == Width::Q && i32::try_from(*imm).is_err() {
                    return Err(format!("{} immediate does not fit i32", mnemonic));
                }
                p.tail.extend_from_slice(&(*imm as i32).to_le_bytes());
            }
        }
        _ => return Err(format!("unsupported {} operands", mnemonic)),
    }
    p.finish()
}

fn encode_test(w: Width, ops: &[Operand], mnemonic: &str) -> EncodeResult {
    let mut p = Parts::new();
    width_setup(w, &mut p.rex, &mut p.prefix);
    match ops {
        [Operand::Reg(a), Operand::Reg(b)] => {
            check_width(*a, w, mnemonic)?;
            check_width(*b, w, mnemonic)?;
            p.opcode.push(if w == Width::B { 0x84 } else { 0x85 });
            p.modrm_rr(*a, *b);
        }
        [Operand::Imm(imm), Operand::Reg(r)] => {
            check_width(*r, w, mnemonic)?;
            p.opcode.push(if w == Width::B { 0xf6 } else { 0xf7 });
            p.rex.merge_reg(*r, RexSlot::B);
            p.tail.push(0b11 << 6 | r.low3());
            match w {
                Width::B => p.tail.push(*imm as u8),
                Width::W => p.tail.extend_from_slice(&(*imm as i16).to_le_bytes()),
                _ => p.tail.extend_from_slice(&(*imm as i32).to_le_bytes()),
            }
        }
        _ => return Err(format!("unsupported test operands for '{}'", mnemonic)),
    }
    p.finish()
}

fn encode_imul(w: Width, ops: &[Operand], mnemonic: &str) -> EncodeResult {
    let mut p = Parts::new();
    width_setup(w, &mut p.rex, &mut p.prefix);
    match ops {
        [Operand::Reg(src), Operand::Reg(dst)] => {
            check_width(*src, w, mnemonic)?;
            check_width(*dst, w, mnemonic)?;
            p.rex.merge_reg(*dst, RexSlot::R);
            p.rex.merge_reg(*src, RexSlot::B);
            p.opcode.extend_from_slice(&[0x0f, 0xaf]);
            p.tail.push(0b11 << 6 | dst.low3() << 3 | src.low3());
        }
        [Operand::Mem(m), Operand::Reg(dst)] => {
            check_width(*dst, w, mnemonic)?;
            p.rex.merge_reg(*dst, RexSlot::R);
            p.opcode.extend_from_slice(&[0x0f, 0xaf]);
            p.mem(dst.low3(), m)?;
        }
        _ => return Err(format!("unsupported imul operands for '{}'", mnemonic)),
    }
    p.finish()
}

fn encode_group3_5(stem: &str, w: Width, ops: &[Operand], mnemonic: &str) -> EncodeResult {
    // F6/F7 group: /4 mul, /5 imul(one-op), /6 div, /7 idiv, /3 neg, /2 not.
    let ext = match stem {
        "not" => 2,
        "neg" => 3,
        "mul" => 4,
        "div" => 6,
        "idiv" => 7,
        _ => unreachable!(),
    };
    let mut p = Parts::new();
    width_setup(w, &mut p.rex, &mut p.prefix);
    p.opcode.push(if w == Width::B { 0xf6 } else { 0xf7 });
    match ops {
        [Operand::Reg(r)] => {
            check_width(*r, w, mnemonic)?;
            p.rex.merge_reg(*r, RexSlot::B);
            p.tail.push(0b11 << 6 | ext << 3 | r.low3());
        }
        [Operand::Mem(m)] => p.mem(ext, m)?,
        _ => return Err(format!("unsupported {} operands", mnemonic)),
    }
    p.finish()
}

fn encode_shift(stem: &str, w: Width, ops: &[Operand], mnemonic: &str) -> EncodeResult {
    let ext = match stem {
        "shl" => 4,
        "shr" => 5,
        "sar" => 7,
        _ => unreachable!(),
    };
    let mut p = Parts::new();
    width_setup(w, &mut p.rex, &mut p.prefix);
    match ops {
        [Operand::Imm(1), Operand::Reg(r)] => {
            check_width(*r, w, mnemonic)?;
            p.opcode.push(if w == Width::B { 0xd0 } else { 0xd1 });
            p.rex.merge_reg(*r, RexSlot::B);
            p.tail.push(0b11 << 6 | ext << 3 | r.low3());
        }
        [Operand::Imm(imm), Operand::Reg(r)] => {
            check_width(*r, w, mnemonic)?;
            p.opcode.push(if w == Width::B { 0xc0 } else { 0xc1 });
            p.rex.merge_reg(*r, RexSlot::B);
            p.tail.push(0b11 << 6 | ext << 3 | r.low3());
            p.tail.push(*imm as u8);
        }
        [Operand::Reg(cl), Operand::Reg(r)] if cl.num == 1 && cl.width == Width::B => {
            check_width(*r, w, mnemonic)?;
            p.opcode.push(if w == Width::B { 0xd2 } else { 0xd3 });
            p.rex.merge_reg(*r, RexSlot::B);
            p.tail.push(0b11 << 6 | ext << 3 | r.low3());
        }
        _ => return Err(format!("unsupported shift operands for '{}'", mnemonic)),
    }
    p.finish()
}

fn encode_extension(mnemonic: &str, ops: &[Operand]) -> Result<Option<Encoded>, String> {
    // movzbl/movzbq/movzwl/movzwq, movsbl/movsbq/movswl/movswq, movslq.
    let (src_w, dst_w, opcode): (Width, Width, Vec<u8>) = match mnemonic {
        "movzbl" => (Width::B, Width::L, vec![0x0f, 0xb6]),
        "movzbq" => (Width::B, Width::Q, vec![0x0f, 0xb6]),
        "movzwl" => (Width::W, Width::L, vec![0x0f, 0xb7]),
        "movzwq" => (Width::W, Width::Q, vec![0x0f, 0xb7]),
        "movsbl" => (Width::B, Width::L, vec![0x0f, 0xbe]),
        "movsbq" => (Width::B, Width::Q, vec![0x0f, 0xbe]),
        "movswl" => (Width::W, Width::L, vec![0x0f, 0xbf]),
        "movswq" => (Width::W, Width::Q, vec![0x0f, 0xbf]),
        "movslq" => (Width::L, Width::Q, vec![0x63]),
        _ => return Ok(None),
    };
    let mut p = Parts::new();
    if dst_w == Width::Q {
        p.rex.w = true;
    }
    match ops {
        [Operand::Reg(src), Operand::Reg(dst)] => {
            check_width(*src, src_w, mnemonic)?;
            check_width(*dst, dst_w, mnemonic)?;
            p.rex.merge_reg(*dst, RexSlot::R);
            p.rex.merge_reg(*src, RexSlot::B);
            p.opcode.extend_from_slice(&opcode);
            p.tail.push(0b11 << 6 | dst.low3() << 3 | src.low3());
        }
        [Operand::Mem(m), Operand::Reg(dst)] => {
            check_width(*dst, dst_w, mnemonic)?;
            p.rex.merge_reg(*dst, RexSlot::R);
            p.opcode.extend_from_slice(&opcode);
            p.mem(dst.low3(), m)?;
        }
        _ => return Err(format!("unsupported operands for '{}'", mnemonic)),
    }
    p.finish().map(Some)
}
