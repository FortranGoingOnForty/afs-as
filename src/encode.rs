//! ARM64 instruction encoding.
//!
//! Converts structured instruction representations into their 4-byte binary encodings.
//! Every ARM64 instruction is exactly 32 bits.

use crate::reg::{Cond, FpReg, GpReg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrExtend {
    Uxtw,
    Lsl,
    Sxtw,
    Sxtx,
}

impl AddrExtend {
    fn enc(self) -> u32 {
        match self {
            AddrExtend::Uxtw => 0b010,
            AddrExtend::Lsl => 0b011,
            AddrExtend::Sxtw => 0b110,
            AddrExtend::Sxtx => 0b111,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegShift {
    Lsl,
    Lsr,
    Asr,
}

impl RegShift {
    fn enc(self) -> u32 {
        match self {
            RegShift::Lsl => 0b00,
            RegShift::Lsr => 0b01,
            RegShift::Asr => 0b10,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegExtend {
    Uxtb,
    Uxth,
    Uxtw,
    Uxtx,
    Sxtb,
    Sxth,
    Sxtw,
    Sxtx,
}

impl RegExtend {
    fn enc(self) -> u32 {
        match self {
            RegExtend::Uxtb => 0b000,
            RegExtend::Uxth => 0b001,
            RegExtend::Uxtw => 0b010,
            RegExtend::Uxtx => 0b011,
            RegExtend::Sxtb => 0b100,
            RegExtend::Sxth => 0b101,
            RegExtend::Sxtw => 0b110,
            RegExtend::Sxtx => 0b111,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierOpt {
    Oshld,
    Oshst,
    Osh,
    Nshld,
    Nshst,
    Nsh,
    Ishld,
    Ishst,
    Ish,
    Ld,
    St,
    Sy,
}

impl BarrierOpt {
    fn enc(self) -> u32 {
        match self {
            BarrierOpt::Oshld => 0b0001,
            BarrierOpt::Oshst => 0b0010,
            BarrierOpt::Osh => 0b0011,
            BarrierOpt::Nshld => 0b0101,
            BarrierOpt::Nshst => 0b0110,
            BarrierOpt::Nsh => 0b0111,
            BarrierOpt::Ishld => 0b1001,
            BarrierOpt::Ishst => 0b1010,
            BarrierOpt::Ish => 0b1011,
            BarrierOpt::Ld => 0b1101,
            BarrierOpt::St => 0b1110,
            BarrierOpt::Sy => 0b1111,
        }
    }
}

/// An ARM64 instruction that can be encoded to its 4-byte binary form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    // ---- Data processing (register) ----
    /// ADD Xd, Xn, Xm  (sf=true for 64-bit, false for 32-bit)
    AddReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// ADD Xd, Xn, Xm, <shift> #amount
    AddShiftReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        shift: RegShift,
        amount: u8,
        sf: bool,
    },
    /// ADD Xd, Xn, Rm, <extend> {#amount}
    AddExtReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: RegExtend,
        amount: u8,
        sf: bool,
    },
    /// SUB Xd, Xn, Xm
    SubReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// SUB Xd, Xn, Xm, <shift> #amount
    SubShiftReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        shift: RegShift,
        amount: u8,
        sf: bool,
    },
    /// SUB Xd, Xn, Rm, <extend> {#amount}
    SubExtReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: RegExtend,
        amount: u8,
        sf: bool,
    },
    /// ADDS Xd, Xn, Xm  (sets flags)
    AddsReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// ADDS Xd, Xn, Xm, <shift> #amount  (sets flags)
    AddsShiftReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        shift: RegShift,
        amount: u8,
        sf: bool,
    },
    /// ADDS Xd, Xn, Rm, <extend> {#amount}  (sets flags)
    AddsExtReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: RegExtend,
        amount: u8,
        sf: bool,
    },
    /// SUBS Xd, Xn, Xm  (sets flags)
    SubsReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// SUBS Xd, Xn, Xm, <shift> #amount  (sets flags)
    SubsShiftReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        shift: RegShift,
        amount: u8,
        sf: bool,
    },
    /// SUBS Xd, Xn, Rm, <extend> {#amount}  (sets flags)
    SubsExtReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: RegExtend,
        amount: u8,
        sf: bool,
    },
    /// MUL Xd, Xn, Xm  (alias for MADD Xd, Xn, Xm, XZR)
    Mul {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// MADD Xd, Xn, Xm, Xa
    Madd {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        ra: GpReg,
        sf: bool,
    },
    /// MSUB Xd, Xn, Xm, Xa
    Msub {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        ra: GpReg,
        sf: bool,
    },
    /// UMULL Xd, Wn, Wm
    Umull { rd: GpReg, rn: GpReg, rm: GpReg },
    /// SDIV Xd, Xn, Xm
    Sdiv {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// UDIV Xd, Xn, Xm
    Udiv {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// CSEL Xd, Xn, Xm, cond
    Csel {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        cond: Cond,
        sf: bool,
    },
    /// CSINV Xd, Xn, Xm, cond
    Csinv {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        cond: Cond,
        sf: bool,
    },
    /// CSNEG Xd, Xn, Xm, cond
    Csneg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        cond: Cond,
        sf: bool,
    },
    /// CCMP Xn, #imm5, #nzcv, cond
    CcmpImm {
        rn: GpReg,
        imm5: u8,
        nzcv: u8,
        cond: Cond,
        sf: bool,
    },
    /// CCMN Xn, #imm5, #nzcv, cond
    CcmnImm {
        rn: GpReg,
        imm5: u8,
        nzcv: u8,
        cond: Cond,
        sf: bool,
    },

    // ---- Logic (register) ----
    /// AND Xd, Xn, Xm
    AndReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// ORR Xd, Xn, Xm
    OrrReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// ORN Xd, Xn, Xm
    OrnReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// EOR Xd, Xn, Xm
    EorReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// ANDS Xd, Xn, Xm  (sets flags — TST is ANDS with XZR dest)
    AndsReg {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        sf: bool,
    },
    /// AND Xd, Xn, #imm
    AndImm {
        rd: GpReg,
        rn: GpReg,
        imm: u64,
        sf: bool,
    },
    /// ORR Xd, Xn, #imm
    OrrImm {
        rd: GpReg,
        rn: GpReg,
        imm: u64,
        sf: bool,
    },
    /// EOR Xd, Xn, #imm
    EorImm {
        rd: GpReg,
        rn: GpReg,
        imm: u64,
        sf: bool,
    },
    /// ANDS Xd, Xn, #imm
    AndsImm {
        rd: GpReg,
        rn: GpReg,
        imm: u64,
        sf: bool,
    },

    // ---- Data processing (immediate) ----
    /// ADD Xd, Xn, #imm12{, LSL #12}
    AddImm {
        rd: GpReg,
        rn: GpReg,
        imm12: u16,
        shift: bool,
        sf: bool,
    },
    /// SUB Xd, Xn, #imm12{, LSL #12}
    SubImm {
        rd: GpReg,
        rn: GpReg,
        imm12: u16,
        shift: bool,
        sf: bool,
    },
    /// ADDS Xd, Xn, #imm12{, LSL #12}
    AddsImm {
        rd: GpReg,
        rn: GpReg,
        imm12: u16,
        shift: bool,
        sf: bool,
    },
    /// SUBS Xd, Xn, #imm12{, LSL #12}
    SubsImm {
        rd: GpReg,
        rn: GpReg,
        imm12: u16,
        shift: bool,
        sf: bool,
    },

    // ---- Move (wide immediate) ----
    /// MOVZ Xd, #imm16{, LSL #shift}  (shift = 0, 16, 32, 48)
    Movz {
        rd: GpReg,
        imm16: u16,
        shift: u8,
        sf: bool,
    },
    /// MOVK Xd, #imm16{, LSL #shift}  (keep other bits)
    Movk {
        rd: GpReg,
        imm16: u16,
        shift: u8,
        sf: bool,
    },
    /// MOVN Xd, #imm16{, LSL #shift}  (move NOT)
    Movn {
        rd: GpReg,
        imm16: u16,
        shift: u8,
        sf: bool,
    },

    // ---- Shifts (aliases for UBFM/SBFM/EXTR) ----
    /// LSL Xd, Xn, #amount  (alias for UBFM)
    LslImm {
        rd: GpReg,
        rn: GpReg,
        amount: u8,
        sf: bool,
    },
    /// LSR Xd, Xn, #amount  (alias for UBFM)
    LsrImm {
        rd: GpReg,
        rn: GpReg,
        amount: u8,
        sf: bool,
    },
    /// ASR Xd, Xn, #amount  (alias for SBFM)
    AsrImm {
        rd: GpReg,
        rn: GpReg,
        amount: u8,
        sf: bool,
    },
    /// UBFIZ Xd, Xn, #lsb, #width  (alias for UBFM)
    Ubfiz {
        rd: GpReg,
        rn: GpReg,
        lsb: u8,
        width: u8,
        sf: bool,
    },
    /// BFI Xd, Xn, #lsb, #width  (alias for BFM)
    Bfi {
        rd: GpReg,
        rn: GpReg,
        lsb: u8,
        width: u8,
        sf: bool,
    },
    /// BFXIL Xd, Xn, #lsb, #width  (alias for BFM)
    Bfxil {
        rd: GpReg,
        rn: GpReg,
        lsb: u8,
        width: u8,
        sf: bool,
    },

    // ---- Branches ----
    /// B #offset  (unconditional, PC-relative, offset in bytes, must be aligned)
    B { offset: i32 },
    /// BL #offset  (branch and link / call)
    Bl { offset: i32 },
    /// B.cond #offset  (conditional branch)
    BCond { cond: Cond, offset: i32 },
    /// CBZ Xt, #offset
    Cbz { rt: GpReg, offset: i32, sf: bool },
    /// CBNZ Xt, #offset
    Cbnz { rt: GpReg, offset: i32, sf: bool },
    /// TBZ Xt, #bit, #offset
    Tbz {
        rt: GpReg,
        bit: u8,
        offset: i32,
        sf: bool,
    },
    /// TBNZ Xt, #bit, #offset
    Tbnz {
        rt: GpReg,
        bit: u8,
        offset: i32,
        sf: bool,
    },
    /// RET {Xn}
    Ret { rn: GpReg },
    /// BR Xn  (indirect branch)
    Br { rn: GpReg },
    /// BLR Xn  (indirect call)
    Blr { rn: GpReg },
    /// CSINC Xd, Xn, Xm, cond
    Csinc {
        rd: GpReg,
        rn: GpReg,
        rm: GpReg,
        cond: Cond,
        sf: bool,
    },

    // ---- Address generation ----
    /// ADR Xd, #imm  (PC-relative, ±1MB range)
    Adr { rd: GpReg, imm: i32 },
    /// ADRP Xd, #imm  (page-relative, 4KB pages, ±4GB range)
    Adrp { rd: GpReg, imm: i32 },

    // ---- Load/Store (unsigned offset) ----
    /// LDR Xt, [Xn, #offset]  (64-bit, offset is byte offset, must be 8-byte aligned)
    LdrImm64 { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDR Wt, [Xn, #offset]  (32-bit, 4-byte aligned)
    LdrImm32 { rt: GpReg, rn: GpReg, offset: u16 },
    /// STR Xt, [Xn, #offset]  (64-bit)
    StrImm64 { rt: GpReg, rn: GpReg, offset: u16 },
    /// STR Wt, [Xn, #offset]  (32-bit)
    StrImm32 { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDUR Xt, [Xn, #offset]  (64-bit signed unscaled offset)
    Ldur64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDUR Wt, [Xn, #offset]  (32-bit signed unscaled offset)
    Ldur32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STUR Xt, [Xn, #offset]  (64-bit signed unscaled offset)
    Stur64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STUR Wt, [Xn, #offset]  (32-bit signed unscaled offset)
    Stur32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDR Xt, [Xn, Rm{, extend}]
    LdrReg64 {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDR Wt, [Xn, Rm{, extend}]
    LdrReg32 {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// STR Xt, [Xn, Rm{, extend}]
    StrReg64 {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// STR Wt, [Xn, Rm{, extend}]
    StrReg32 {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDRB Wt, [Xn, #offset]  (byte load, zero-extend)
    Ldrb { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRSB Wt, [Xn, #offset]  (byte load, sign-extend to 32)
    Ldrsb32 { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRSB Xt, [Xn, #offset]  (byte load, sign-extend to 64)
    Ldrsb64 { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRH Wt, [Xn, #offset]  (halfword load, zero-extend)
    Ldrh { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRSH Wt, [Xn, #offset]  (halfword load, sign-extend to 32)
    Ldrsh32 { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRSH Xt, [Xn, #offset]  (halfword load, sign-extend to 64)
    Ldrsh64 { rt: GpReg, rn: GpReg, offset: u16 },
    /// STRB Wt, [Xn, #offset]
    Strb { rt: GpReg, rn: GpReg, offset: u16 },
    /// STRH Wt, [Xn, #offset]
    Strh { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRSW Xt, [Xn, #offset]  (32-bit load, sign-extend to 64)
    Ldrsw { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRB Wt, [Xn, Rm{, extend}]
    LdrbReg {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDRSB Wt, [Xn, Rm{, extend}]
    LdrsbReg32 {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDRSB Xt, [Xn, Rm{, extend}]
    LdrsbReg64 {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDRH Wt, [Xn, Rm{, extend}]
    LdrhReg {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDRSH Wt, [Xn, Rm{, extend}]
    LdrshReg32 {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDRSH Xt, [Xn, Rm{, extend}]
    LdrshReg64 {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// STRB Wt, [Xn, Rm{, extend}]
    StrbReg {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// STRH Wt, [Xn, Rm{, extend}]
    StrhReg {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDRSW Xt, [Xn, Rm{, extend}]
    LdrswReg {
        rt: GpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },

    /// LDR Dt, [Xn, #offset]
    LdrFpImm64 { rt: FpReg, rn: GpReg, offset: u16 },
    /// LDR St, [Xn, #offset]
    LdrFpImm32 { rt: FpReg, rn: GpReg, offset: u16 },
    /// LDR Ht, [Xn, #offset]
    LdrFpImm16 { rt: FpReg, rn: GpReg, offset: u16 },
    /// LDR Bt, [Xn, #offset]
    LdrFpImm8 { rt: FpReg, rn: GpReg, offset: u16 },
    /// LDR Qt, [Xn, #offset]
    LdrFpImm128 { rt: FpReg, rn: GpReg, offset: u16 },
    /// STR Dt, [Xn, #offset]
    StrFpImm64 { rt: FpReg, rn: GpReg, offset: u16 },
    /// STR St, [Xn, #offset]
    StrFpImm32 { rt: FpReg, rn: GpReg, offset: u16 },
    /// STR Ht, [Xn, #offset]
    StrFpImm16 { rt: FpReg, rn: GpReg, offset: u16 },
    /// STR Bt, [Xn, #offset]
    StrFpImm8 { rt: FpReg, rn: GpReg, offset: u16 },
    /// STR Qt, [Xn, #offset]
    StrFpImm128 { rt: FpReg, rn: GpReg, offset: u16 },
    /// LDR Dt, [Xn, Rm{, extend}]
    LdrFpReg64 {
        rt: FpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDR St, [Xn, Rm{, extend}]
    LdrFpReg32 {
        rt: FpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// LDR Qt, [Xn, Rm{, extend}]
    LdrFpReg128 {
        rt: FpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// STR Dt, [Xn, Rm{, extend}]
    StrFpReg64 {
        rt: FpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// STR St, [Xn, Rm{, extend}]
    StrFpReg32 {
        rt: FpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },
    /// STR Qt, [Xn, Rm{, extend}]
    StrFpReg128 {
        rt: FpReg,
        rn: GpReg,
        rm: GpReg,
        extend: AddrExtend,
        shift: bool,
    },

    /// LDR Xt, label
    LdrLit64 { rt: GpReg, offset: i32 },
    /// LDR Wt, label
    LdrLit32 { rt: GpReg, offset: i32 },
    /// LDRSW Xt, label
    LdrswLit { rt: GpReg, offset: i32 },
    /// LDR Dt, label
    LdrFpLit64 { rt: FpReg, offset: i32 },
    /// LDR St, label
    LdrFpLit32 { rt: FpReg, offset: i32 },
    /// LDR Qt, label
    LdrFpLit128 { rt: FpReg, offset: i32 },

    // ---- Load/Store (pre-index) ----
    /// LDR Wt, [Xn, #offset]!  (pre-index, 32-bit)
    LdrPre32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STR Wt, [Xn, #offset]!  (pre-index, 32-bit)
    StrPre32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRB Wt, [Xn, #offset]!  (pre-index, byte)
    LdrbPre { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRSB Wt, [Xn, #offset]!  (pre-index, byte)
    LdrsbPre32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRSB Xt, [Xn, #offset]!  (pre-index, byte)
    LdrsbPre64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STRB Wt, [Xn, #offset]!  (pre-index, byte)
    StrbPre { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRH Wt, [Xn, #offset]!  (pre-index, halfword)
    LdrhPre { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRSH Wt, [Xn, #offset]!  (pre-index, halfword)
    LdrshPre32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRSH Xt, [Xn, #offset]!  (pre-index, halfword)
    LdrshPre64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STRH Wt, [Xn, #offset]!  (pre-index, halfword)
    StrhPre { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDR Xt, [Xn, #offset]!  (pre-index, 64-bit)
    LdrPre64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STR Xt, [Xn, #offset]!  (pre-index, 64-bit)
    StrPre64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDR Dt, [Xn, #offset]!  (pre-index, double)
    LdrFpPre64 { rt: FpReg, rn: GpReg, offset: i16 },
    /// STR Dt, [Xn, #offset]!  (pre-index, double)
    StrFpPre64 { rt: FpReg, rn: GpReg, offset: i16 },
    /// LDR St, [Xn, #offset]!  (pre-index, single)
    LdrFpPre32 { rt: FpReg, rn: GpReg, offset: i16 },
    /// STR St, [Xn, #offset]!  (pre-index, single)
    StrFpPre32 { rt: FpReg, rn: GpReg, offset: i16 },
    /// LDR Qt, [Xn, #offset]!  (pre-index, vector)
    LdrFpPre128 { rt: FpReg, rn: GpReg, offset: i16 },
    /// STR Qt, [Xn, #offset]!  (pre-index, vector)
    StrFpPre128 { rt: FpReg, rn: GpReg, offset: i16 },

    // ---- Load/Store (post-index) ----
    /// LDR Wt, [Xn], #offset  (post-index, 32-bit)
    LdrPost32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STR Wt, [Xn], #offset  (post-index, 32-bit)
    StrPost32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRB Wt, [Xn], #offset  (post-index, byte)
    LdrbPost { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRSB Wt, [Xn], #offset  (post-index, byte)
    LdrsbPost32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRSB Xt, [Xn], #offset  (post-index, byte)
    LdrsbPost64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STRB Wt, [Xn], #offset  (post-index, byte)
    StrbPost { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRH Wt, [Xn], #offset  (post-index, halfword)
    LdrhPost { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRSH Wt, [Xn], #offset  (post-index, halfword)
    LdrshPost32 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDRSH Xt, [Xn], #offset  (post-index, halfword)
    LdrshPost64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STRH Wt, [Xn], #offset  (post-index, halfword)
    StrhPost { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDR Xt, [Xn], #offset  (post-index, 64-bit)
    LdrPost64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STR Xt, [Xn], #offset  (post-index, 64-bit)
    StrPost64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// LDR Dt, [Xn], #offset  (post-index, double)
    LdrFpPost64 { rt: FpReg, rn: GpReg, offset: i16 },
    /// STR Dt, [Xn], #offset  (post-index, double)
    StrFpPost64 { rt: FpReg, rn: GpReg, offset: i16 },
    /// LDR St, [Xn], #offset  (post-index, single)
    LdrFpPost32 { rt: FpReg, rn: GpReg, offset: i16 },
    /// STR St, [Xn], #offset  (post-index, single)
    StrFpPost32 { rt: FpReg, rn: GpReg, offset: i16 },
    /// LDR Qt, [Xn], #offset  (post-index, vector)
    LdrFpPost128 { rt: FpReg, rn: GpReg, offset: i16 },
    /// STR Qt, [Xn], #offset  (post-index, vector)
    StrFpPost128 { rt: FpReg, rn: GpReg, offset: i16 },

    // ---- Atomic memory operations ----
    /// LDAPRB Wt, [Xn]
    Ldaprb { rt: GpReg, rn: GpReg },
    /// LDAPRH Wt, [Xn]
    Ldaprh { rt: GpReg, rn: GpReg },
    /// LDAPR Wt, [Xn]
    Ldapr32 { rt: GpReg, rn: GpReg },
    /// LDAPR Xt, [Xn]
    Ldapr64 { rt: GpReg, rn: GpReg },
    /// STLRB Wt, [Xn]
    Stlrb { rt: GpReg, rn: GpReg },
    /// STLRH Wt, [Xn]
    Stlrh { rt: GpReg, rn: GpReg },
    /// STLR Wt, [Xn]
    Stlr32 { rt: GpReg, rn: GpReg },
    /// STLR Xt, [Xn]
    Stlr64 { rt: GpReg, rn: GpReg },
    /// LDADDALB Ws, Wt, [Xn]
    Ldaddalb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDADDALH Ws, Wt, [Xn]
    Ldaddalh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDADDAL Ws, Wt, [Xn]
    Ldaddal32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDADDAL Xs, Xt, [Xn]
    Ldaddal64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDUMAXALB Ws, Wt, [Xn]
    Ldumaxalb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDUMAXALH Ws, Wt, [Xn]
    Ldumaxalh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDUMAXAL Ws, Wt, [Xn]
    Ldumaxal32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDUMAXAL Xs, Xt, [Xn]
    Ldumaxal64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSMAXALB Ws, Wt, [Xn]
    Ldsmaxalb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSMAXALH Ws, Wt, [Xn]
    Ldsmaxalh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSMAXAL Ws, Wt, [Xn]
    Ldsmaxal32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSMAXAL Xs, Xt, [Xn]
    Ldsmaxal64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDUMINALB Ws, Wt, [Xn]
    Lduminalb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDUMINALH Ws, Wt, [Xn]
    Lduminalh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDUMINAL Ws, Wt, [Xn]
    Lduminal32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDUMINAL Xs, Xt, [Xn]
    Lduminal64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSMINALB Ws, Wt, [Xn]
    Ldsminalb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSMINALH Ws, Wt, [Xn]
    Ldsminalh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSMINAL Ws, Wt, [Xn]
    Ldsminal32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSMINAL Xs, Xt, [Xn]
    Ldsminal64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDCLRALB Ws, Wt, [Xn]
    Ldclralb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDCLRALH Ws, Wt, [Xn]
    Ldclralh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDCLRAL Ws, Wt, [Xn]
    Ldclral32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDCLRAL Xs, Xt, [Xn]
    Ldclral64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDEORALB Ws, Wt, [Xn]
    Ldeoralb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDEORALH Ws, Wt, [Xn]
    Ldeoralh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDEORAL Ws, Wt, [Xn]
    Ldeoral32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDEORAL Xs, Xt, [Xn]
    Ldeoral64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSETALB Ws, Wt, [Xn]
    Ldsetalb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSETALH Ws, Wt, [Xn]
    Ldsetalh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSETAL Ws, Wt, [Xn]
    Ldsetal32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// LDSETAL Xs, Xt, [Xn]
    Ldsetal64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// SWPAL Ws, Wt, [Xn]
    Swpal32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// SWPAL Xs, Xt, [Xn]
    Swpal64 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// SWPALB Ws, Wt, [Xn]
    Swpalb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// SWPALH Ws, Wt, [Xn]
    Swpalh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// CASALB Ws, Wt, [Xn]
    Casalb { rs: GpReg, rt: GpReg, rn: GpReg },
    /// CASALH Ws, Wt, [Xn]
    Casalh { rs: GpReg, rt: GpReg, rn: GpReg },
    /// CASAL Ws, Wt, [Xn]
    Casal32 { rs: GpReg, rt: GpReg, rn: GpReg },
    /// CASAL Xs, Xt, [Xn]
    Casal64 { rs: GpReg, rt: GpReg, rn: GpReg },

    // ---- Load/Store pair ----
    /// STP Wt1, Wt2, [Xn, #offset]  (signed offset, 32-bit)
    StpOff32 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Xt1, Xt2, [Xn, #offset]  (signed offset, 64-bit)
    StpOff64 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Wt1, Wt2, [Xn, #offset]  (signed offset, 32-bit)
    LdpOff32 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Xt1, Xt2, [Xn, #offset]  (signed offset, 64-bit)
    LdpOff64 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Wt1, Wt2, [Xn, #offset]!  (pre-index, 32-bit)
    StpPre32 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Xt1, Xt2, [Xn, #offset]!  (pre-index, 64-bit)
    StpPre64 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Wt1, Wt2, [Xn], #offset  (post-index, 32-bit)
    StpPost32 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Xt1, Xt2, [Xn], #offset  (post-index, 64-bit)
    StpPost64 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Wt1, Wt2, [Xn, #offset]!  (pre-index, 32-bit)
    LdpPre32 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Xt1, Xt2, [Xn, #offset]!  (pre-index, 64-bit)
    LdpPre64 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Wt1, Wt2, [Xn], #offset  (post-index, 32-bit)
    LdpPost32 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Xt1, Xt2, [Xn], #offset  (post-index, 64-bit)
    LdpPost64 {
        rt1: GpReg,
        rt2: GpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Dt1, Dt2, [Xn, #offset]  (signed offset, double)
    StpFpOff64 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Dt1, Dt2, [Xn, #offset]  (signed offset, double)
    LdpFpOff64 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Dt1, Dt2, [Xn, #offset]!  (pre-index, double)
    StpFpPre64 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Dt1, Dt2, [Xn], #offset  (post-index, double)
    StpFpPost64 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Dt1, Dt2, [Xn, #offset]!  (pre-index, double)
    LdpFpPre64 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Dt1, Dt2, [Xn], #offset  (post-index, double)
    LdpFpPost64 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP St1, St2, [Xn, #offset]  (signed offset, single)
    StpFpOff32 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP St1, St2, [Xn, #offset]  (signed offset, single)
    LdpFpOff32 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP St1, St2, [Xn, #offset]!  (pre-index, single)
    StpFpPre32 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP St1, St2, [Xn], #offset  (post-index, single)
    StpFpPost32 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP St1, St2, [Xn, #offset]!  (pre-index, single)
    LdpFpPre32 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP St1, St2, [Xn], #offset  (post-index, single)
    LdpFpPost32 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Qt1, Qt2, [Xn, #offset]  (signed offset, 128-bit)
    StpFpOff128 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Qt1, Qt2, [Xn, #offset]  (signed offset, 128-bit)
    LdpFpOff128 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Qt1, Qt2, [Xn, #offset]!  (pre-index, 128-bit)
    StpFpPre128 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// STP Qt1, Qt2, [Xn], #offset  (post-index, 128-bit)
    StpFpPost128 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Qt1, Qt2, [Xn, #offset]!  (pre-index, 128-bit)
    LdpFpPre128 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },
    /// LDP Qt1, Qt2, [Xn], #offset  (post-index, 128-bit)
    LdpFpPost128 {
        rt1: FpReg,
        rt2: FpReg,
        rn: GpReg,
        offset: i16,
    },

    // ---- Floating point arithmetic ----
    /// FADD Dd, Dn, Dm  (double)
    FaddD { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FSUB Dd, Dn, Dm  (double)
    FsubD { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMUL Dd, Dn, Dm  (double)
    FmulD { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FDIV Dd, Dn, Dm  (double)
    FdivD { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FADD Sd, Sn, Sm  (single)
    FaddS { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FADD.4S Vd, Vn, Vm
    FaddV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FADDP.2D Vd, Vn, Vm
    FaddpV2D { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FADDP.4S Vd, Vn, Vm
    FaddpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMAXP.2D Vd, Vn, Vm
    FmaxpV2D { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMAXP.4S Vd, Vn, Vm
    FmaxpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMINP.2D Vd, Vn, Vm
    FminpV2D { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMINP.4S Vd, Vn, Vm
    FminpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMAXNMP.4S Vd, Vn, Vm
    FmaxnmpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMINNMP.4S Vd, Vn, Vm
    FminnmpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FADDP.2S Sd, Vn
    FaddpV2S { rd: FpReg, rn: FpReg },
    /// FADDP.2D Dd, Vn
    FaddpV2DScalar { rd: FpReg, rn: FpReg },
    /// FMAXP.2D Dd, Vn
    FmaxpV2DScalar { rd: FpReg, rn: FpReg },
    /// FMINP.2D Dd, Vn
    FminpV2DScalar { rd: FpReg, rn: FpReg },
    /// FMAXNMP.2D Dd, Vn
    FmaxnmpV2DScalar { rd: FpReg, rn: FpReg },
    /// FMINNMP.2D Dd, Vn
    FminnmpV2DScalar { rd: FpReg, rn: FpReg },
    /// FMLA.4S Vd, Vn, Vm
    FmlaV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMLS.4S Vd, Vn, Vm
    FmlsV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// ADD.4S Vd, Vn, Vm
    AddV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// ADDP.2D Vd, Vn, Vm
    AddpV2D { rd: FpReg, rn: FpReg, rm: FpReg },
    /// ADDP.16B Vd, Vn, Vm
    AddpV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// ADDP.8H Vd, Vn, Vm
    AddpV8H { rd: FpReg, rn: FpReg, rm: FpReg },
    /// ADDP.4S Vd, Vn, Vm
    AddpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SMAXP.16B Vd, Vn, Vm
    SmaxpV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SMAXP.8H Vd, Vn, Vm
    SmaxpV8H { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SMAXP.4S Vd, Vn, Vm
    SmaxpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SMINP.16B Vd, Vn, Vm
    SminpV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SMINP.8H Vd, Vn, Vm
    SminpV8H { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SMINP.4S Vd, Vn, Vm
    SminpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMAX.4S Vd, Vn, Vm
    FmaxV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMIN.4S Vd, Vn, Vm
    FminV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMAXNM.4S Vd, Vn, Vm
    FmaxnmV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMAXNMP.2D Vd, Vn, Vm
    FmaxnmpV2D { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMINNM.4S Vd, Vn, Vm
    FminnmV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMINNMP.2D Vd, Vn, Vm
    FminnmpV2D { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SMAX.4S Vd, Vn, Vm
    SmaxV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SMIN.4S Vd, Vn, Vm
    SminV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UMAX.4S Vd, Vn, Vm
    UmaxV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UMAXP.16B Vd, Vn, Vm
    UmaxpV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UMAXP.8H Vd, Vn, Vm
    UmaxpV8H { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UMIN.4S Vd, Vn, Vm
    UminV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UMINP.16B Vd, Vn, Vm
    UminpV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UMINP.8H Vd, Vn, Vm
    UminpV8H { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UMAXP.4S Vd, Vn, Vm
    UmaxpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UMINP.4S Vd, Vn, Vm
    UminpV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// ADDV.16B Bd, Vn
    AddvV16B { rd: FpReg, rn: FpReg },
    /// ADDV.8H Hd, Vn
    AddvV8H { rd: FpReg, rn: FpReg },
    /// ADDV.4S Sd, Vn
    AddvV4S { rd: FpReg, rn: FpReg },
    /// UMAXV.16B Bd, Vn
    UmaxvV16B { rd: FpReg, rn: FpReg },
    /// UMAXV.8H Hd, Vn
    UmaxvV8H { rd: FpReg, rn: FpReg },
    /// UMAXV.4S Sd, Vn
    UmaxvV4S { rd: FpReg, rn: FpReg },
    /// SMAXV.16B Bd, Vn
    SmaxvV16B { rd: FpReg, rn: FpReg },
    /// SMAXV.8H Hd, Vn
    SmaxvV8H { rd: FpReg, rn: FpReg },
    /// SMAXV.4S Sd, Vn
    SmaxvV4S { rd: FpReg, rn: FpReg },
    /// UMINV.16B Bd, Vn
    UminvV16B { rd: FpReg, rn: FpReg },
    /// UMINV.8H Hd, Vn
    UminvV8H { rd: FpReg, rn: FpReg },
    /// UMINV.4S Sd, Vn
    UminvV4S { rd: FpReg, rn: FpReg },
    /// SMINV.16B Bd, Vn
    SminvV16B { rd: FpReg, rn: FpReg },
    /// SMINV.8H Hd, Vn
    SminvV8H { rd: FpReg, rn: FpReg },
    /// SMINV.4S Sd, Vn
    SminvV4S { rd: FpReg, rn: FpReg },
    /// FMAXV.4S Sd, Vn
    FmaxvV4S { rd: FpReg, rn: FpReg },
    /// FMINV.4S Sd, Vn
    FminvV4S { rd: FpReg, rn: FpReg },
    /// FMAXNMV.4S Sd, Vn
    FmaxnmvV4S { rd: FpReg, rn: FpReg },
    /// FMINNMV.4S Sd, Vn
    FminnmvV4S { rd: FpReg, rn: FpReg },
    /// FSUB.4S Vd, Vn, Vm
    FsubV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// SUB.4S Vd, Vn, Vm
    SubV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FSUB Sd, Sn, Sm  (single)
    FsubS { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMUL.4S Vd, Vn, Vm
    FmulV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMUL Sd, Sn, Sm  (single)
    FmulS { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FDIV.4S Vd, Vn, Vm
    FdivV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FABS.4S Vd, Vn
    FabsV4S { rd: FpReg, rn: FpReg },
    /// FABS.2D Vd, Vn
    FabsV2D { rd: FpReg, rn: FpReg },
    /// FNEG.4S Vd, Vn
    FnegV4S { rd: FpReg, rn: FpReg },
    /// FNEG.2D Vd, Vn
    FnegV2D { rd: FpReg, rn: FpReg },
    /// FSQRT.4S Vd, Vn
    FsqrtV4S { rd: FpReg, rn: FpReg },
    /// FSQRT.2D Vd, Vn
    FsqrtV2D { rd: FpReg, rn: FpReg },
    /// SCVTF.4S Vd, Vn
    ScvtfV4S { rd: FpReg, rn: FpReg },
    /// UCVTF.4S Vd, Vn
    UcvtfV4S { rd: FpReg, rn: FpReg },
    /// FCVTZS.4S Vd, Vn
    FcvtzsV4S { rd: FpReg, rn: FpReg },
    /// FCVTZU.4S Vd, Vn
    FcvtzuV4S { rd: FpReg, rn: FpReg },
    /// FRECPE.4S Vd, Vn
    FrecpeV4S { rd: FpReg, rn: FpReg },
    /// FRECPS.4S Vd, Vn, Vm
    FrecpsV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FRSQRTE.4S Vd, Vn
    FrsqrteV4S { rd: FpReg, rn: FpReg },
    /// FRSQRTS.4S Vd, Vn, Vm
    FrsqrtsV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FRINTN.4S Vd, Vn
    FrintnV4S { rd: FpReg, rn: FpReg },
    /// FRINTM.4S Vd, Vn
    FrintmV4S { rd: FpReg, rn: FpReg },
    /// FRINTP.4S Vd, Vn
    FrintpV4S { rd: FpReg, rn: FpReg },
    /// FRINTZ.4S Vd, Vn
    FrintzV4S { rd: FpReg, rn: FpReg },
    /// FRINTA.4S Vd, Vn
    FrintaV4S { rd: FpReg, rn: FpReg },
    /// FRINTI.4S Vd, Vn
    FrintiV4S { rd: FpReg, rn: FpReg },
    /// FDIV Sd, Sn, Sm  (single)
    FdivS { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMOV Dd, Dn
    FmovRegD { rd: FpReg, rn: FpReg },
    /// FMOV Sd, Sn
    FmovRegS { rd: FpReg, rn: FpReg },
    /// MOV.8B Vd, Vn
    MovV8B { rd: FpReg, rn: FpReg },
    /// MOV.16B Vd, Vn
    MovV16B { rd: FpReg, rn: FpReg },
    /// MOV.4S Vd, Vn
    MovV4S { rd: FpReg, rn: FpReg },
    /// MOV.2D Vd, Vn
    MovV2D { rd: FpReg, rn: FpReg },
    /// DUP.16B Vd, Vn[index]
    DupV16B { rd: FpReg, rn: FpReg, index: u8 },
    /// DUP.8H Vd, Vn[index]
    DupV8H { rd: FpReg, rn: FpReg, index: u8 },
    /// DUP.4S Vd, Vn[index]
    DupV4S { rd: FpReg, rn: FpReg, index: u8 },
    /// DUP.2D Vd, Vn[index]
    DupV2D { rd: FpReg, rn: FpReg, index: u8 },
    /// AND.16B Vd, Vn, Vm
    AndV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// BIC.16B Vd, Vn, Vm
    BicV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// ORR.16B Vd, Vn, Vm
    OrrV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// EOR.16B Vd, Vn, Vm
    EorV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// BIF.16B Vd, Vn, Vm
    BifV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// BIT.16B Vd, Vn, Vm
    BitV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// BSL.16B Vd, Vn, Vm
    BslV16B { rd: FpReg, rn: FpReg, rm: FpReg },
    /// CMEQ.4S Vd, Vn, Vm
    CmeqV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FCMEQ.4S Vd, Vn, Vm
    FcmeqV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// CMHS.4S Vd, Vn, Vm
    CmhsV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// CMHI.4S Vd, Vn, Vm
    CmhiV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// CMGE.4S Vd, Vn, Vm
    CmgeV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FCMGE.4S Vd, Vn, Vm
    FcmgeV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// CMGT.4S Vd, Vn, Vm
    CmgtV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FCMGT.4S Vd, Vn, Vm
    FcmgtV4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// EXT.16B Vd, Vn, Vm, #index
    ExtV16B {
        rd: FpReg,
        rn: FpReg,
        rm: FpReg,
        index: u8,
    },
    /// REV64.4S Vd, Vn
    Rev64V4S { rd: FpReg, rn: FpReg },
    /// ZIP1.4S Vd, Vn, Vm
    Zip1V4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// ZIP2.4S Vd, Vn, Vm
    Zip2V4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UZP1.4S Vd, Vn, Vm
    Uzp1V4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// UZP2.4S Vd, Vn, Vm
    Uzp2V4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// TRN1.4S Vd, Vn, Vm
    Trn1V4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// TRN2.4S Vd, Vn, Vm
    Trn2V4S { rd: FpReg, rn: FpReg, rm: FpReg },
    /// TBL.16B Vd, { Vn... }, Vm
    TblV16B {
        rd: FpReg,
        table: FpReg,
        table_len: u8,
        index: FpReg,
    },
    /// TBX.16B Vd, { Vn... }, Vm
    TbxV16B {
        rd: FpReg,
        table: FpReg,
        table_len: u8,
        index: FpReg,
    },
    /// MOV Sd, Vn[index]
    MovFromLaneS { rd: FpReg, rn: FpReg, index: u8 },
    /// MOV Dd, Vn[index]
    MovFromLaneD { rd: FpReg, rn: FpReg, index: u8 },
    /// MOV.S Wd, Vn[index]
    MovFromLaneGpS { rd: GpReg, rn: FpReg, index: u8 },
    /// MOV.D Xd, Vn[index]
    MovFromLaneGpD { rd: GpReg, rn: FpReg, index: u8 },
    /// UMOV.H Wd, Vn[index]
    UmovFromLaneH { rd: GpReg, rn: FpReg, index: u8 },
    /// UMOV.B Wd, Vn[index]
    UmovFromLaneB { rd: GpReg, rn: FpReg, index: u8 },
    /// SMOV.H Wd, Vn[index]
    SmovFromLaneH { rd: GpReg, rn: FpReg, index: u8 },
    /// SMOV.B Wd, Vn[index]
    SmovFromLaneB { rd: GpReg, rn: FpReg, index: u8 },
    /// MOV.S Vd[index], Vn[index]
    MovLaneS {
        rd: FpReg,
        rd_index: u8,
        rn: FpReg,
        rn_index: u8,
    },
    /// MOV.D Vd[index], Vn[index]
    MovLaneD {
        rd: FpReg,
        rd_index: u8,
        rn: FpReg,
        rn_index: u8,
    },
    /// MOV.H Vd[index], Vn[index]
    MovLaneH {
        rd: FpReg,
        rd_index: u8,
        rn: FpReg,
        rn_index: u8,
    },
    /// MOV.B Vd[index], Vn[index]
    MovLaneB {
        rd: FpReg,
        rd_index: u8,
        rn: FpReg,
        rn_index: u8,
    },
    /// MOV.S Vd[index], Wn
    MovLaneFromGpS { rd: FpReg, rd_index: u8, rn: GpReg },
    /// MOV.D Vd[index], Xn
    MovLaneFromGpD { rd: FpReg, rd_index: u8, rn: GpReg },
    /// MOV.H Vd[index], Wn
    MovLaneFromGpH { rd: FpReg, rd_index: u8, rn: GpReg },
    /// MOV.B Vd[index], Wn
    MovLaneFromGpB { rd: FpReg, rd_index: u8, rn: GpReg },
    /// FNEG Dd, Dn
    FnegD { rd: FpReg, rn: FpReg },
    /// FNEG Sd, Sn
    FnegS { rd: FpReg, rn: FpReg },
    /// FABS Dd, Dn
    FabsD { rd: FpReg, rn: FpReg },
    /// FABS Sd, Sn
    FabsS { rd: FpReg, rn: FpReg },
    /// FSQRT Dd, Dn
    FsqrtD { rd: FpReg, rn: FpReg },
    /// FSQRT Sd, Sn
    FsqrtS { rd: FpReg, rn: FpReg },
    /// FCMP Dn, Dm
    FcmpD { rn: FpReg, rm: FpReg },
    /// FCMP Sn, Sm
    FcmpS { rn: FpReg, rm: FpReg },
    /// FMOV Dd, #imm
    FmovImmD { rd: FpReg, imm8: u8 },
    /// FMOV Sd, #imm
    FmovImmS { rd: FpReg, imm8: u8 },
    /// FCSEL Dd, Dn, Dm, cond
    FcselD {
        rd: FpReg,
        rn: FpReg,
        rm: FpReg,
        cond: Cond,
    },
    /// FCSEL Sd, Sn, Sm, cond
    FcselS {
        rd: FpReg,
        rn: FpReg,
        rm: FpReg,
        cond: Cond,
    },
    /// FMADD Dd, Dn, Dm, Da  (Dd = Da + Dn*Dm)
    FmaddD {
        rd: FpReg,
        rn: FpReg,
        rm: FpReg,
        ra: FpReg,
    },
    /// FMADD Sd, Sn, Sm, Sa
    FmaddS {
        rd: FpReg,
        rn: FpReg,
        rm: FpReg,
        ra: FpReg,
    },

    // ---- FP / integer conversion ----
    /// FCVTZS Xd, Dn  (double -> signed 64-bit int, truncate toward zero)
    FcvtzsD { rd: GpReg, rn: FpReg },
    /// SCVTF Dd, Xn  (signed 64-bit int -> double)
    ScvtfD { rd: FpReg, rn: GpReg },
    /// FMOV Dd, Xn  (move bits GP -> FP, no conversion)
    FmovToD { rd: FpReg, rn: GpReg },
    /// FMOV Sd, Wn  (move bits GP -> FP, no conversion)
    FmovToS { rd: FpReg, rn: GpReg },
    /// FMOV Xd, Dn  (move bits FP -> GP, no conversion)
    FmovFromD { rd: GpReg, rn: FpReg },
    /// FMOV Wd, Sn  (move bits FP -> GP, no conversion)
    FmovFromS { rd: GpReg, rn: FpReg },

    // ---- System ----
    /// SVC #imm16
    Svc { imm16: u16 },
    /// NOP
    Nop,
    /// YIELD
    Yield,
    /// WFE
    Wfe,
    /// WFI
    Wfi,
    /// SEV
    Sev,
    /// SEVL
    Sevl,
    /// DMB <option>
    Dmb { option: BarrierOpt },
    /// DSB <option>
    Dsb { option: BarrierOpt },
    /// ISB {<option>}
    Isb { option: BarrierOpt },
    /// BRK #imm16
    Brk { imm16: u16 },
}

impl Inst {
    /// Encode this instruction into its 32-bit binary representation.
    pub fn encode(&self) -> u32 {
        match self {
            // ---- Data processing (register) ----
            Inst::AddReg { rd, rn, rm, sf } => dp_reg(*sf, 0b00, 0b01011, 0b00, *rm, 0, *rn, *rd),
            Inst::AddShiftReg {
                rd,
                rn,
                rm,
                shift,
                amount,
                sf,
            } => dp_reg(
                *sf,
                0b00,
                0b01011,
                shift.enc(),
                *rm,
                *amount as u32,
                *rn,
                *rd,
            ),
            Inst::AddExtReg {
                rd,
                rn,
                rm,
                extend,
                amount,
                sf,
            } => dp_ext(*sf, 0b00, *rm, *extend, *amount, *rn, *rd),
            Inst::SubReg { rd, rn, rm, sf } => dp_reg(*sf, 0b10, 0b01011, 0b00, *rm, 0, *rn, *rd),
            Inst::SubShiftReg {
                rd,
                rn,
                rm,
                shift,
                amount,
                sf,
            } => dp_reg(
                *sf,
                0b10,
                0b01011,
                shift.enc(),
                *rm,
                *amount as u32,
                *rn,
                *rd,
            ),
            Inst::SubExtReg {
                rd,
                rn,
                rm,
                extend,
                amount,
                sf,
            } => dp_ext(*sf, 0b10, *rm, *extend, *amount, *rn, *rd),
            Inst::AddsReg { rd, rn, rm, sf } => dp_reg(*sf, 0b01, 0b01011, 0b00, *rm, 0, *rn, *rd),
            Inst::AddsShiftReg {
                rd,
                rn,
                rm,
                shift,
                amount,
                sf,
            } => dp_reg(
                *sf,
                0b01,
                0b01011,
                shift.enc(),
                *rm,
                *amount as u32,
                *rn,
                *rd,
            ),
            Inst::AddsExtReg {
                rd,
                rn,
                rm,
                extend,
                amount,
                sf,
            } => dp_ext(*sf, 0b01, *rm, *extend, *amount, *rn, *rd),
            Inst::SubsReg { rd, rn, rm, sf } => dp_reg(*sf, 0b11, 0b01011, 0b00, *rm, 0, *rn, *rd),
            Inst::SubsShiftReg {
                rd,
                rn,
                rm,
                shift,
                amount,
                sf,
            } => dp_reg(
                *sf,
                0b11,
                0b01011,
                shift.enc(),
                *rm,
                *amount as u32,
                *rn,
                *rd,
            ),
            Inst::SubsExtReg {
                rd,
                rn,
                rm,
                extend,
                amount,
                sf,
            } => dp_ext(*sf, 0b11, *rm, *extend, *amount, *rn, *rd),

            // MUL: alias for MADD Xd, Xn, Xm, XZR
            // sf|00|11011|000|Rm|0|Ra(11111)|Rn|Rd
            Inst::Mul { rd, rn, rm, sf } => {
                let s = (*sf as u32) << 31;
                s | (0b00_11011_000 << 21)
                    | (rm.enc() << 16)
                    | (0b0_11111 << 10)
                    | (rn.enc() << 5)
                    | rd.enc()
            }
            // MADD: sf|00|11011|000|Rm|0|Ra|Rn|Rd
            Inst::Madd { rd, rn, rm, ra, sf } => {
                let s = (*sf as u32) << 31;
                s | (0b00_11011_000 << 21)
                    | (rm.enc() << 16)
                    | (ra.enc() << 10)
                    | (rn.enc() << 5)
                    | rd.enc()
            }
            Inst::Msub { rd, rn, rm, ra, sf } => {
                let s = (*sf as u32) << 31;
                s | (0b00_11011_000 << 21)
                    | (rm.enc() << 16)
                    | (1 << 15)
                    | (ra.enc() << 10)
                    | (rn.enc() << 5)
                    | rd.enc()
            }
            Inst::Umull { rd, rn, rm } => {
                0x9BA0_0000 | (rm.enc() << 16) | (0b1_1111 << 10) | (rn.enc() << 5) | rd.enc()
            }
            // SDIV: sf|0|0|11010110|Rm|00001|1|Rn|Rd
            Inst::Sdiv { rd, rn, rm, sf } => {
                let s = (*sf as u32) << 31;
                s | (0b0_0_11010110 << 21)
                    | (rm.enc() << 16)
                    | (0b000011 << 10)
                    | (rn.enc() << 5)
                    | rd.enc()
            }
            // UDIV: sf|0|0|11010110|Rm|00001|0|Rn|Rd
            Inst::Udiv { rd, rn, rm, sf } => {
                let s = (*sf as u32) << 31;
                s | (0b0_0_11010110 << 21)
                    | (rm.enc() << 16)
                    | (0b000010 << 10)
                    | (rn.enc() << 5)
                    | rd.enc()
            }

            // ---- Logic (register) ----
            Inst::AndReg { rd, rn, rm, sf } => logic_reg(*sf, 0b00, false, *rm, *rn, *rd),
            Inst::OrrReg { rd, rn, rm, sf } => logic_reg(*sf, 0b01, false, *rm, *rn, *rd),
            Inst::OrnReg { rd, rn, rm, sf } => logic_reg(*sf, 0b01, true, *rm, *rn, *rd),
            Inst::EorReg { rd, rn, rm, sf } => logic_reg(*sf, 0b10, false, *rm, *rn, *rd),
            Inst::AndsReg { rd, rn, rm, sf } => logic_reg(*sf, 0b11, false, *rm, *rn, *rd),
            Inst::AndImm { rd, rn, imm, sf } => logical_imm(*sf, 0b00, *imm, *rn, *rd),
            Inst::OrrImm { rd, rn, imm, sf } => logical_imm(*sf, 0b01, *imm, *rn, *rd),
            Inst::EorImm { rd, rn, imm, sf } => logical_imm(*sf, 0b10, *imm, *rn, *rd),
            Inst::AndsImm { rd, rn, imm, sf } => logical_imm(*sf, 0b11, *imm, *rn, *rd),

            // ---- Data processing (immediate) ----
            Inst::AddImm {
                rd,
                rn,
                imm12,
                shift,
                sf,
            } => dp_imm(*sf, 0b00, *imm12, *shift, *rn, *rd),
            Inst::SubImm {
                rd,
                rn,
                imm12,
                shift,
                sf,
            } => dp_imm(*sf, 0b10, *imm12, *shift, *rn, *rd),
            Inst::AddsImm {
                rd,
                rn,
                imm12,
                shift,
                sf,
            } => dp_imm(*sf, 0b01, *imm12, *shift, *rn, *rd),
            Inst::SubsImm {
                rd,
                rn,
                imm12,
                shift,
                sf,
            } => dp_imm(*sf, 0b11, *imm12, *shift, *rn, *rd),

            // ---- Move (wide immediate) ----
            Inst::Movz {
                rd,
                imm16,
                shift,
                sf,
            } => mov_wide(*sf, 0b10, *imm16, *shift, *rd),
            Inst::Movk {
                rd,
                imm16,
                shift,
                sf,
            } => mov_wide(*sf, 0b11, *imm16, *shift, *rd),
            Inst::Movn {
                rd,
                imm16,
                shift,
                sf,
            } => mov_wide(*sf, 0b00, *imm16, *shift, *rd),

            // ---- Shifts (bitfield aliases) ----
            Inst::LslImm { rd, rn, amount, sf } => {
                let bits = if *sf { 64u8 } else { 32u8 };
                let immr = bits.wrapping_sub(*amount) & (bits - 1);
                let imms = bits - 1 - *amount;
                bitfield(*sf, 0b10, immr, imms, *rn, *rd)
            }
            Inst::LsrImm { rd, rn, amount, sf } => {
                let imms = if *sf { 63u8 } else { 31u8 };
                bitfield(*sf, 0b10, *amount, imms, *rn, *rd)
            }
            Inst::AsrImm { rd, rn, amount, sf } => {
                let imms = if *sf { 63u8 } else { 31u8 };
                bitfield(*sf, 0b00, *amount, imms, *rn, *rd)
            }
            Inst::Ubfiz {
                rd,
                rn,
                lsb,
                width,
                sf,
            } => {
                let bits = if *sf { 64u8 } else { 32u8 };
                let immr = bits.wrapping_sub(*lsb) & (bits - 1);
                bitfield(*sf, 0b10, immr, width.wrapping_sub(1), *rn, *rd)
            }
            Inst::Bfi {
                rd,
                rn,
                lsb,
                width,
                sf,
            } => {
                let bits = if *sf { 64u8 } else { 32u8 };
                let immr = bits.wrapping_sub(*lsb) & (bits - 1);
                bitfield(*sf, 0b01, immr, width.wrapping_sub(1), *rn, *rd)
            }
            Inst::Bfxil {
                rd,
                rn,
                lsb,
                width,
                sf,
            } => bitfield(
                *sf,
                0b01,
                *lsb,
                lsb.wrapping_add(*width).wrapping_sub(1),
                *rn,
                *rd,
            ),

            // ---- Branches ----
            Inst::B { offset } => {
                let imm26 = ((*offset >> 2) as u32) & 0x03FF_FFFF;
                (0b000101 << 26) | imm26
            }
            Inst::Bl { offset } => {
                let imm26 = ((*offset >> 2) as u32) & 0x03FF_FFFF;
                (0b100101 << 26) | imm26
            }
            Inst::BCond { cond, offset } => {
                let imm19 = ((*offset >> 2) as u32) & 0x7FFFF;
                (0b01010100 << 24) | (imm19 << 5) | cond.enc()
            }
            Inst::Cbz { rt, offset, sf } => {
                let s = (*sf as u32) << 31;
                let imm19 = ((*offset >> 2) as u32) & 0x7FFFF;
                s | (0b011010_0 << 24) | (imm19 << 5) | rt.enc()
            }
            Inst::Cbnz { rt, offset, sf } => {
                let s = (*sf as u32) << 31;
                let imm19 = ((*offset >> 2) as u32) & 0x7FFFF;
                s | (0b011010_1 << 24) | (imm19 << 5) | rt.enc()
            }
            Inst::Tbz {
                rt,
                bit,
                offset,
                sf: _,
            } => {
                let b5 = ((*bit >> 5) as u32) & 0x1;
                let b40 = (*bit as u32) & 0x1F;
                let imm14 = ((*offset >> 2) as u32) & 0x3FFF;
                (b5 << 31) | (0b011011 << 25) | (b40 << 19) | (imm14 << 5) | rt.enc()
            }
            Inst::Tbnz {
                rt,
                bit,
                offset,
                sf: _,
            } => {
                let b5 = ((*bit >> 5) as u32) & 0x1;
                let b40 = (*bit as u32) & 0x1F;
                let imm14 = ((*offset >> 2) as u32) & 0x3FFF;
                (b5 << 31) | (0b011011 << 25) | (1 << 24) | (b40 << 19) | (imm14 << 5) | rt.enc()
            }
            Inst::Ret { rn } => 0xD65F0000 | (rn.enc() << 5),
            Inst::Br { rn } => 0xD61F0000 | (rn.enc() << 5),
            Inst::Blr { rn } => 0xD63F0000 | (rn.enc() << 5),
            Inst::Csel {
                rd,
                rn,
                rm,
                cond,
                sf,
            } => csel(*sf, 0b00, *rm, *cond, *rn, *rd),
            Inst::Csinc {
                rd,
                rn,
                rm,
                cond,
                sf,
            } => csel(*sf, 0b01, *rm, *cond, *rn, *rd),
            Inst::Csinv {
                rd,
                rn,
                rm,
                cond,
                sf,
            } => csel(*sf, 0b10, *rm, *cond, *rn, *rd),
            Inst::Csneg {
                rd,
                rn,
                rm,
                cond,
                sf,
            } => csel(*sf, 0b11, *rm, *cond, *rn, *rd),
            Inst::CcmpImm {
                rn,
                imm5,
                nzcv,
                cond,
                sf,
            } => ccmp_imm(*sf, true, *rn, *imm5, *nzcv, *cond),
            Inst::CcmnImm {
                rn,
                imm5,
                nzcv,
                cond,
                sf,
            } => ccmp_imm(*sf, false, *rn, *imm5, *nzcv, *cond),

            // ---- Address generation ----
            Inst::Adr { rd, imm } => {
                let immlo = (*imm as u32) & 0x3;
                let immhi = ((*imm as u32) >> 2) & 0x7FFFF;
                (immlo << 29) | (0b10000 << 24) | (immhi << 5) | rd.enc()
            }
            Inst::Adrp { rd, imm } => {
                let page = (*imm >> 12) as u32;
                let immlo = page & 0x3;
                let immhi = (page >> 2) & 0x7FFFF;
                (1 << 31) | (immlo << 29) | (0b10000 << 24) | (immhi << 5) | rd.enc()
            }

            // ---- Load/Store (unsigned offset) ----
            Inst::LdrImm64 { rt, rn, offset } => {
                let uoff = (*offset / 8) as u32;
                (0b11_111_0_01_01 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::LdrImm32 { rt, rn, offset } => {
                let uoff = (*offset / 4) as u32;
                (0b10_111_0_01_01 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::StrImm64 { rt, rn, offset } => {
                let uoff = (*offset / 8) as u32;
                (0b11_111_0_01_00 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::StrImm32 { rt, rn, offset } => {
                let uoff = (*offset / 4) as u32;
                (0b10_111_0_01_00 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldur64 { rt, rn, offset } => ldst_idx(0b11, 0b01, *offset, 0b00, *rn, *rt),
            Inst::Ldur32 { rt, rn, offset } => ldst_idx(0b10, 0b01, *offset, 0b00, *rn, *rt),
            Inst::Stur64 { rt, rn, offset } => ldst_idx(0b11, 0b00, *offset, 0b00, *rn, *rt),
            Inst::Stur32 { rt, rn, offset } => ldst_idx(0b10, 0b00, *offset, 0b00, *rn, *rt),
            Inst::LdrReg64 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b11, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrReg32 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b10, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::StrReg64 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b11, 0b00, *rm, *extend, *shift, *rn, *rt),
            Inst::StrReg32 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b10, 0b00, *rm, *extend, *shift, *rn, *rt),
            Inst::Ldrb { rt, rn, offset } => {
                let uoff = *offset as u32;
                (0b00_111_0_01_01 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldrsb32 { rt, rn, offset } => {
                let uoff = *offset as u32;
                (0b111_001 << 24) | (0b11 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldrsb64 { rt, rn, offset } => {
                let uoff = *offset as u32;
                (0b111_001 << 24) | (0b10 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldrh { rt, rn, offset } => {
                let uoff = (*offset / 2) as u32;
                (0b01_111_0_01_01 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldrsh32 { rt, rn, offset } => {
                let uoff = (*offset / 2) as u32;
                (0b01 << 30)
                    | (0b111_001 << 24)
                    | (0b11 << 22)
                    | (uoff << 10)
                    | (rn.enc() << 5)
                    | rt.enc()
            }
            Inst::Ldrsh64 { rt, rn, offset } => {
                let uoff = (*offset / 2) as u32;
                (0b01 << 30)
                    | (0b111_001 << 24)
                    | (0b10 << 22)
                    | (uoff << 10)
                    | (rn.enc() << 5)
                    | rt.enc()
            }
            Inst::Strb { rt, rn, offset } => {
                let uoff = *offset as u32;
                (0b00_111_0_01_00 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Strh { rt, rn, offset } => {
                let uoff = (*offset / 2) as u32;
                (0b01_111_0_01_00 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldrsw { rt, rn, offset } => {
                let uoff = (*offset / 4) as u32;
                (0b10_111_0_01_10 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::LdrbReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b00, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrsbReg32 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b00, 0b11, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrsbReg64 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b00, 0b10, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrhReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b01, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrshReg32 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b01, 0b11, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrshReg64 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b01, 0b10, *rm, *extend, *shift, *rn, *rt),
            Inst::StrbReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b00, 0b00, *rm, *extend, *shift, *rn, *rt),
            Inst::StrhReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b01, 0b00, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrswReg {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg(0b10, 0b10, *rm, *extend, *shift, *rn, *rt),

            Inst::LdrFpImm64 { rt, rn, offset } => ldst_uimm_fp(0b11, 0b01, 3, *offset, *rn, *rt),
            Inst::LdrFpImm32 { rt, rn, offset } => ldst_uimm_fp(0b10, 0b01, 2, *offset, *rn, *rt),
            Inst::LdrFpImm16 { rt, rn, offset } => ldst_uimm_fp(0b01, 0b01, 1, *offset, *rn, *rt),
            Inst::LdrFpImm8 { rt, rn, offset } => ldst_uimm_fp(0b00, 0b01, 0, *offset, *rn, *rt),
            Inst::LdrFpImm128 { rt, rn, offset } => ldst_uimm_fp(0b00, 0b11, 4, *offset, *rn, *rt),
            Inst::StrFpImm64 { rt, rn, offset } => ldst_uimm_fp(0b11, 0b00, 3, *offset, *rn, *rt),
            Inst::StrFpImm32 { rt, rn, offset } => ldst_uimm_fp(0b10, 0b00, 2, *offset, *rn, *rt),
            Inst::StrFpImm16 { rt, rn, offset } => ldst_uimm_fp(0b01, 0b00, 1, *offset, *rn, *rt),
            Inst::StrFpImm8 { rt, rn, offset } => ldst_uimm_fp(0b00, 0b00, 0, *offset, *rn, *rt),
            Inst::StrFpImm128 { rt, rn, offset } => ldst_uimm_fp(0b00, 0b10, 4, *offset, *rn, *rt),
            Inst::LdrFpReg64 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg_fp(0b11, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrFpReg32 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg_fp(0b10, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrFpReg128 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg_fp(0b00, 0b11, *rm, *extend, *shift, *rn, *rt),
            Inst::StrFpReg64 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg_fp(0b11, 0b00, *rm, *extend, *shift, *rn, *rt),
            Inst::StrFpReg32 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg_fp(0b10, 0b00, *rm, *extend, *shift, *rn, *rt),
            Inst::StrFpReg128 {
                rt,
                rn,
                rm,
                extend,
                shift,
            } => ldst_reg_fp(0b00, 0b10, *rm, *extend, *shift, *rn, *rt),

            Inst::LdrLit64 { rt, offset } => ldr_lit(0b01, *offset, *rt),
            Inst::LdrLit32 { rt, offset } => ldr_lit(0b00, *offset, *rt),
            Inst::LdrswLit { rt, offset } => ldr_lit(0b10, *offset, *rt),
            Inst::LdrFpLit64 { rt, offset } => ldr_lit_fp(0b01, *offset, *rt),
            Inst::LdrFpLit32 { rt, offset } => ldr_lit_fp(0b00, *offset, *rt),
            Inst::LdrFpLit128 { rt, offset } => ldr_lit_fp(0b10, *offset, *rt),

            // ---- Load/Store (pre/post-index) ----
            Inst::LdrPre32 { rt, rn, offset } => ldst_idx(0b10, 0b01, *offset, 0b11, *rn, *rt),
            Inst::StrPre32 { rt, rn, offset } => ldst_idx(0b10, 0b00, *offset, 0b11, *rn, *rt),
            Inst::LdrbPre { rt, rn, offset } => ldst_idx(0b00, 0b01, *offset, 0b11, *rn, *rt),
            Inst::LdrsbPre32 { rt, rn, offset } => ldst_idx(0b00, 0b11, *offset, 0b11, *rn, *rt),
            Inst::LdrsbPre64 { rt, rn, offset } => ldst_idx(0b00, 0b10, *offset, 0b11, *rn, *rt),
            Inst::StrbPre { rt, rn, offset } => ldst_idx(0b00, 0b00, *offset, 0b11, *rn, *rt),
            Inst::LdrhPre { rt, rn, offset } => ldst_idx(0b01, 0b01, *offset, 0b11, *rn, *rt),
            Inst::LdrshPre32 { rt, rn, offset } => ldst_idx(0b01, 0b11, *offset, 0b11, *rn, *rt),
            Inst::LdrshPre64 { rt, rn, offset } => ldst_idx(0b01, 0b10, *offset, 0b11, *rn, *rt),
            Inst::StrhPre { rt, rn, offset } => ldst_idx(0b01, 0b00, *offset, 0b11, *rn, *rt),
            Inst::LdrPre64 { rt, rn, offset } => ldst_idx(0b11, 0b01, *offset, 0b11, *rn, *rt),
            Inst::StrPre64 { rt, rn, offset } => ldst_idx(0b11, 0b00, *offset, 0b11, *rn, *rt),
            Inst::LdrPost32 { rt, rn, offset } => ldst_idx(0b10, 0b01, *offset, 0b01, *rn, *rt),
            Inst::StrPost32 { rt, rn, offset } => ldst_idx(0b10, 0b00, *offset, 0b01, *rn, *rt),
            Inst::LdrbPost { rt, rn, offset } => ldst_idx(0b00, 0b01, *offset, 0b01, *rn, *rt),
            Inst::LdrsbPost32 { rt, rn, offset } => ldst_idx(0b00, 0b11, *offset, 0b01, *rn, *rt),
            Inst::LdrsbPost64 { rt, rn, offset } => ldst_idx(0b00, 0b10, *offset, 0b01, *rn, *rt),
            Inst::StrbPost { rt, rn, offset } => ldst_idx(0b00, 0b00, *offset, 0b01, *rn, *rt),
            Inst::LdrhPost { rt, rn, offset } => ldst_idx(0b01, 0b01, *offset, 0b01, *rn, *rt),
            Inst::LdrshPost32 { rt, rn, offset } => ldst_idx(0b01, 0b11, *offset, 0b01, *rn, *rt),
            Inst::LdrshPost64 { rt, rn, offset } => ldst_idx(0b01, 0b10, *offset, 0b01, *rn, *rt),
            Inst::StrhPost { rt, rn, offset } => ldst_idx(0b01, 0b00, *offset, 0b01, *rn, *rt),
            Inst::LdrPost64 { rt, rn, offset } => ldst_idx(0b11, 0b01, *offset, 0b01, *rn, *rt),
            Inst::StrPost64 { rt, rn, offset } => ldst_idx(0b11, 0b00, *offset, 0b01, *rn, *rt),
            Inst::LdrFpPre64 { rt, rn, offset } => ldst_idx_fp(0b11, 0b01, *offset, 0b11, *rn, *rt),
            Inst::StrFpPre64 { rt, rn, offset } => ldst_idx_fp(0b11, 0b00, *offset, 0b11, *rn, *rt),
            Inst::LdrFpPre32 { rt, rn, offset } => ldst_idx_fp(0b10, 0b01, *offset, 0b11, *rn, *rt),
            Inst::StrFpPre32 { rt, rn, offset } => ldst_idx_fp(0b10, 0b00, *offset, 0b11, *rn, *rt),
            Inst::LdrFpPre128 { rt, rn, offset } => {
                ldst_idx_fp(0b00, 0b11, *offset, 0b11, *rn, *rt)
            }
            Inst::StrFpPre128 { rt, rn, offset } => {
                ldst_idx_fp(0b00, 0b10, *offset, 0b11, *rn, *rt)
            }
            Inst::LdrFpPost64 { rt, rn, offset } => {
                ldst_idx_fp(0b11, 0b01, *offset, 0b01, *rn, *rt)
            }
            Inst::StrFpPost64 { rt, rn, offset } => {
                ldst_idx_fp(0b11, 0b00, *offset, 0b01, *rn, *rt)
            }
            Inst::LdrFpPost32 { rt, rn, offset } => {
                ldst_idx_fp(0b10, 0b01, *offset, 0b01, *rn, *rt)
            }
            Inst::StrFpPost32 { rt, rn, offset } => {
                ldst_idx_fp(0b10, 0b00, *offset, 0b01, *rn, *rt)
            }
            Inst::LdrFpPost128 { rt, rn, offset } => {
                ldst_idx_fp(0b00, 0b11, *offset, 0b01, *rn, *rt)
            }
            Inst::StrFpPost128 { rt, rn, offset } => {
                ldst_idx_fp(0b00, 0b10, *offset, 0b01, *rn, *rt)
            }

            // ---- Atomic memory operations ----
            Inst::Ldaprb { rt, rn } => 0x38BFC000 | (rn.enc() << 5) | rt.enc(),
            Inst::Ldaprh { rt, rn } => 0x78BFC000 | (rn.enc() << 5) | rt.enc(),
            Inst::Ldapr32 { rt, rn } => 0xB8BFC000 | (rn.enc() << 5) | rt.enc(),
            Inst::Ldapr64 { rt, rn } => 0xF8BFC000 | (rn.enc() << 5) | rt.enc(),
            Inst::Stlrb { rt, rn } => 0x089FFC00 | (rn.enc() << 5) | rt.enc(),
            Inst::Stlrh { rt, rn } => 0x489FFC00 | (rn.enc() << 5) | rt.enc(),
            Inst::Stlr32 { rt, rn } => 0x889FFC00 | (rn.enc() << 5) | rt.enc(),
            Inst::Stlr64 { rt, rn } => 0xC89FFC00 | (rn.enc() << 5) | rt.enc(),
            Inst::Ldaddalb { rs, rt, rn } => {
                0x38E00000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldaddalh { rs, rt, rn } => {
                0x78E00000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldaddal32 { rs, rt, rn } => {
                0xB8E00000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldaddal64 { rs, rt, rn } => {
                0xF8E00000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldumaxalb { rs, rt, rn } => {
                0x38E06000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldumaxalh { rs, rt, rn } => {
                0x78E06000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldumaxal32 { rs, rt, rn } => {
                0xB8E06000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldumaxal64 { rs, rt, rn } => {
                0xF8E06000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsmaxalb { rs, rt, rn } => {
                0x38E04000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsmaxalh { rs, rt, rn } => {
                0x78E04000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsmaxal32 { rs, rt, rn } => {
                0xB8E04000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsmaxal64 { rs, rt, rn } => {
                0xF8E04000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Lduminalb { rs, rt, rn } => {
                0x38E07000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Lduminalh { rs, rt, rn } => {
                0x78E07000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Lduminal32 { rs, rt, rn } => {
                0xB8E07000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Lduminal64 { rs, rt, rn } => {
                0xF8E07000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsminalb { rs, rt, rn } => {
                0x38E05000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsminalh { rs, rt, rn } => {
                0x78E05000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsminal32 { rs, rt, rn } => {
                0xB8E05000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsminal64 { rs, rt, rn } => {
                0xF8E05000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldclralb { rs, rt, rn } => {
                0x38E01000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldclralh { rs, rt, rn } => {
                0x78E01000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldclral32 { rs, rt, rn } => {
                0xB8E01000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldclral64 { rs, rt, rn } => {
                0xF8E01000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldeoralb { rs, rt, rn } => {
                0x38E02000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldeoralh { rs, rt, rn } => {
                0x78E02000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldeoral32 { rs, rt, rn } => {
                0xB8E02000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldeoral64 { rs, rt, rn } => {
                0xF8E02000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsetalb { rs, rt, rn } => {
                0x38E03000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsetalh { rs, rt, rn } => {
                0x78E03000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsetal32 { rs, rt, rn } => {
                0xB8E03000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldsetal64 { rs, rt, rn } => {
                0xF8E03000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Swpal32 { rs, rt, rn } => {
                0xB8E08000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Swpal64 { rs, rt, rn } => {
                0xF8E08000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Swpalb { rs, rt, rn } => {
                0x38E08000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Swpalh { rs, rt, rn } => {
                0x78E08000 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Casalb { rs, rt, rn } => {
                0x08E0FC00 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Casalh { rs, rt, rn } => {
                0x48E0FC00 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Casal32 { rs, rt, rn } => {
                0x88E0FC00 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Casal64 { rs, rt, rn } => {
                0xC8E0FC00 | (rs.enc() << 16) | (rn.enc() << 5) | rt.enc()
            }

            // ---- Load/Store pair ----
            Inst::StpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b00, 0b010, 0, *offset, *rt2, *rn, *rt1),
            Inst::StpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b10, 0b010, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b00, 0b010, 1, *offset, *rt2, *rn, *rt1),
            Inst::LdpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b10, 0b010, 1, *offset, *rt2, *rn, *rt1),
            Inst::StpPre32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b00, 0b011, 0, *offset, *rt2, *rn, *rt1),
            Inst::StpPre64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b10, 0b011, 0, *offset, *rt2, *rn, *rt1),
            Inst::StpPost32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b00, 0b001, 0, *offset, *rt2, *rn, *rt1),
            Inst::StpPost64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b10, 0b001, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpPre32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b00, 0b011, 1, *offset, *rt2, *rn, *rt1),
            Inst::LdpPre64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b10, 0b011, 1, *offset, *rt2, *rn, *rt1),
            Inst::LdpPost32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b00, 0b001, 1, *offset, *rt2, *rn, *rt1),
            Inst::LdpPost64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp(0b10, 0b001, 1, *offset, *rt2, *rn, *rt1),
            Inst::StpFpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b01, 0b010, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpOff64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b01, 0b010, 1, *offset, *rt2, *rn, *rt1),
            Inst::StpFpPre64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b01, 0b011, 0, *offset, *rt2, *rn, *rt1),
            Inst::StpFpPost64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b01, 0b001, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpPre64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b01, 0b011, 1, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpPost64 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b01, 0b001, 1, *offset, *rt2, *rn, *rt1),
            Inst::StpFpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b00, 0b010, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpOff32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b00, 0b010, 1, *offset, *rt2, *rn, *rt1),
            Inst::StpFpPre32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b00, 0b011, 0, *offset, *rt2, *rn, *rt1),
            Inst::StpFpPost32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b00, 0b001, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpPre32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b00, 0b011, 1, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpPost32 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b00, 0b001, 1, *offset, *rt2, *rn, *rt1),
            Inst::StpFpOff128 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b10, 0b010, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpOff128 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b10, 0b010, 1, *offset, *rt2, *rn, *rt1),
            Inst::StpFpPre128 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b10, 0b011, 0, *offset, *rt2, *rn, *rt1),
            Inst::StpFpPost128 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b10, 0b001, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpPre128 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b10, 0b011, 1, *offset, *rt2, *rn, *rt1),
            Inst::LdpFpPost128 {
                rt1,
                rt2,
                rn,
                offset,
            } => ldp_stp_fp(0b10, 0b001, 1, *offset, *rt2, *rn, *rt1),

            // ---- FP arithmetic ----
            Inst::FaddD { rd, rn, rm } => fp_arith(0b01, 0b0010, *rm, *rn, *rd),
            Inst::FsubD { rd, rn, rm } => fp_arith(0b01, 0b0011, *rm, *rn, *rd),
            Inst::FmulD { rd, rn, rm } => fp_arith(0b01, 0b0000, *rm, *rn, *rd),
            Inst::FdivD { rd, rn, rm } => fp_arith(0b01, 0b0001, *rm, *rn, *rd),
            Inst::FaddS { rd, rn, rm } => fp_arith(0b00, 0b0010, *rm, *rn, *rd),
            Inst::FaddV4S { rd, rn, rm } => simd_fp_arith_4s(0x4E20D400, *rm, *rn, *rd),
            Inst::FaddpV2D { rd, rn, rm } => simd_fp_arith_2d(0x6E60D400, *rm, *rn, *rd),
            Inst::FaddpV4S { rd, rn, rm } => simd_fp_arith_4s(0x6E20D400, *rm, *rn, *rd),
            Inst::FmaxpV2D { rd, rn, rm } => simd_fp_arith_2d(0x6E60F400, *rm, *rn, *rd),
            Inst::FmaxpV4S { rd, rn, rm } => simd_fp_arith_4s(0x6E20F400, *rm, *rn, *rd),
            Inst::FminpV2D { rd, rn, rm } => simd_fp_arith_2d(0x6EE0F400, *rm, *rn, *rd),
            Inst::FminpV4S { rd, rn, rm } => simd_fp_arith_4s(0x6EA0F400, *rm, *rn, *rd),
            Inst::FmaxnmpV2D { rd, rn, rm } => simd_fp_arith_2d(0x6E60C400, *rm, *rn, *rd),
            Inst::FmaxnmpV4S { rd, rn, rm } => simd_fp_arith_4s(0x6E20C400, *rm, *rn, *rd),
            Inst::FminnmpV2D { rd, rn, rm } => simd_fp_arith_2d(0x6EE0C400, *rm, *rn, *rd),
            Inst::FminnmpV4S { rd, rn, rm } => simd_fp_arith_4s(0x6EA0C400, *rm, *rn, *rd),
            Inst::FaddpV2S { rd, rn } => simd_reduce_4s(0x7E30D800, *rn, *rd),
            Inst::FaddpV2DScalar { rd, rn } => simd_reduce(0x7E70D800, *rn, *rd),
            Inst::FmaxpV2DScalar { rd, rn } => simd_reduce(0x7E70F800, *rn, *rd),
            Inst::FminpV2DScalar { rd, rn } => simd_reduce(0x7EF0F800, *rn, *rd),
            Inst::FmaxnmpV2DScalar { rd, rn } => simd_reduce(0x7E70C800, *rn, *rd),
            Inst::FminnmpV2DScalar { rd, rn } => simd_reduce(0x7EF0C800, *rn, *rd),
            Inst::FmlaV4S { rd, rn, rm } => simd_fp_arith_4s(0x4E20CC00, *rm, *rn, *rd),
            Inst::FmlsV4S { rd, rn, rm } => simd_fp_arith_4s(0x4EA0CC00, *rm, *rn, *rd),
            Inst::AddV4S { rd, rn, rm } => simd_binary(0x4EA08400, *rm, *rn, *rd),
            Inst::AddpV2D { rd, rn, rm } => simd_binary(0x4EE0BC00, *rm, *rn, *rd),
            Inst::AddpV16B { rd, rn, rm } => simd_binary(0x4E20BC00, *rm, *rn, *rd),
            Inst::AddpV8H { rd, rn, rm } => simd_binary(0x4E60BC00, *rm, *rn, *rd),
            Inst::AddpV4S { rd, rn, rm } => simd_binary(0x4EA0BC00, *rm, *rn, *rd),
            Inst::SmaxpV16B { rd, rn, rm } => simd_binary(0x4E20A400, *rm, *rn, *rd),
            Inst::SmaxpV8H { rd, rn, rm } => simd_binary(0x4E60A400, *rm, *rn, *rd),
            Inst::SmaxpV4S { rd, rn, rm } => simd_binary(0x4EA0A400, *rm, *rn, *rd),
            Inst::SminpV16B { rd, rn, rm } => simd_binary(0x4E20AC00, *rm, *rn, *rd),
            Inst::SminpV8H { rd, rn, rm } => simd_binary(0x4E60AC00, *rm, *rn, *rd),
            Inst::SminpV4S { rd, rn, rm } => simd_binary(0x4EA0AC00, *rm, *rn, *rd),
            Inst::FmaxV4S { rd, rn, rm } => simd_fp_arith_4s(0x4E20F400, *rm, *rn, *rd),
            Inst::FminV4S { rd, rn, rm } => simd_fp_arith_4s(0x4EA0F400, *rm, *rn, *rd),
            Inst::FmaxnmV4S { rd, rn, rm } => simd_fp_arith_4s(0x4E20C400, *rm, *rn, *rd),
            Inst::FminnmV4S { rd, rn, rm } => simd_fp_arith_4s(0x4EA0C400, *rm, *rn, *rd),
            Inst::SmaxV4S { rd, rn, rm } => simd_binary(0x4EA06400, *rm, *rn, *rd),
            Inst::SminV4S { rd, rn, rm } => simd_binary(0x4EA06C00, *rm, *rn, *rd),
            Inst::UmaxV4S { rd, rn, rm } => simd_binary(0x6EA06400, *rm, *rn, *rd),
            Inst::UmaxpV16B { rd, rn, rm } => simd_binary(0x6E20A400, *rm, *rn, *rd),
            Inst::UmaxpV8H { rd, rn, rm } => simd_binary(0x6E60A400, *rm, *rn, *rd),
            Inst::UminV4S { rd, rn, rm } => simd_binary(0x6EA06C00, *rm, *rn, *rd),
            Inst::UminpV16B { rd, rn, rm } => simd_binary(0x6E20AC00, *rm, *rn, *rd),
            Inst::UminpV8H { rd, rn, rm } => simd_binary(0x6E60AC00, *rm, *rn, *rd),
            Inst::UmaxpV4S { rd, rn, rm } => simd_binary(0x6EA0A400, *rm, *rn, *rd),
            Inst::UminpV4S { rd, rn, rm } => simd_binary(0x6EA0AC00, *rm, *rn, *rd),
            Inst::AddvV16B { rd, rn } => simd_reduce(0x4E31B800, *rn, *rd),
            Inst::AddvV8H { rd, rn } => simd_reduce(0x4E71B800, *rn, *rd),
            Inst::AddvV4S { rd, rn } => simd_reduce_4s(0x4EB1B800, *rn, *rd),
            Inst::UmaxvV16B { rd, rn } => simd_reduce(0x6E30A800, *rn, *rd),
            Inst::UmaxvV8H { rd, rn } => simd_reduce(0x6E70A800, *rn, *rd),
            Inst::UmaxvV4S { rd, rn } => simd_reduce_4s(0x6EB0A800, *rn, *rd),
            Inst::SmaxvV16B { rd, rn } => simd_reduce(0x4E30A800, *rn, *rd),
            Inst::SmaxvV8H { rd, rn } => simd_reduce(0x4E70A800, *rn, *rd),
            Inst::SmaxvV4S { rd, rn } => simd_reduce_4s(0x4EB0A800, *rn, *rd),
            Inst::UminvV16B { rd, rn } => simd_reduce(0x6E31A800, *rn, *rd),
            Inst::UminvV8H { rd, rn } => simd_reduce(0x6E71A800, *rn, *rd),
            Inst::UminvV4S { rd, rn } => simd_reduce_4s(0x6EB1A800, *rn, *rd),
            Inst::SminvV16B { rd, rn } => simd_reduce(0x4E31A800, *rn, *rd),
            Inst::SminvV8H { rd, rn } => simd_reduce(0x4E71A800, *rn, *rd),
            Inst::SminvV4S { rd, rn } => simd_reduce_4s(0x4EB1A800, *rn, *rd),
            Inst::FmaxvV4S { rd, rn } => simd_reduce_4s(0x6E30F800, *rn, *rd),
            Inst::FminvV4S { rd, rn } => simd_reduce_4s(0x6EB0F800, *rn, *rd),
            Inst::FmaxnmvV4S { rd, rn } => simd_reduce_4s(0x6E30C800, *rn, *rd),
            Inst::FminnmvV4S { rd, rn } => simd_reduce_4s(0x6EB0C800, *rn, *rd),
            Inst::FsubV4S { rd, rn, rm } => simd_fp_arith_4s(0x4EA0D400, *rm, *rn, *rd),
            Inst::SubV4S { rd, rn, rm } => simd_binary(0x6EA08400, *rm, *rn, *rd),
            Inst::FsubS { rd, rn, rm } => fp_arith(0b00, 0b0011, *rm, *rn, *rd),
            Inst::FmulV4S { rd, rn, rm } => simd_fp_arith_4s(0x6E20DC00, *rm, *rn, *rd),
            Inst::FmulS { rd, rn, rm } => fp_arith(0b00, 0b0000, *rm, *rn, *rd),
            Inst::FdivV4S { rd, rn, rm } => simd_fp_arith_4s(0x6E20FC00, *rm, *rn, *rd),
            Inst::FabsV4S { rd, rn } => simd_unary(0x4EA0F800, *rn, *rd),
            Inst::FabsV2D { rd, rn } => simd_unary(0x4EE0F800, *rn, *rd),
            Inst::FnegV4S { rd, rn } => simd_unary(0x6EA0F800, *rn, *rd),
            Inst::FnegV2D { rd, rn } => simd_unary(0x6EE0F800, *rn, *rd),
            Inst::FsqrtV4S { rd, rn } => simd_unary(0x6EA1F800, *rn, *rd),
            Inst::FsqrtV2D { rd, rn } => simd_unary(0x6EE1F800, *rn, *rd),
            Inst::ScvtfV4S { rd, rn } => simd_unary(0x4E21D800, *rn, *rd),
            Inst::UcvtfV4S { rd, rn } => simd_unary(0x6E21D800, *rn, *rd),
            Inst::FcvtzsV4S { rd, rn } => simd_unary(0x4EA1B800, *rn, *rd),
            Inst::FcvtzuV4S { rd, rn } => simd_unary(0x6EA1B800, *rn, *rd),
            Inst::FrecpeV4S { rd, rn } => simd_unary(0x4EA1D800, *rn, *rd),
            Inst::FrecpsV4S { rd, rn, rm } => simd_fp_arith_4s(0x4E20FC00, *rm, *rn, *rd),
            Inst::FrsqrteV4S { rd, rn } => simd_unary(0x6EA1D800, *rn, *rd),
            Inst::FrsqrtsV4S { rd, rn, rm } => simd_fp_arith_4s(0x4EA0FC00, *rm, *rn, *rd),
            Inst::FrintnV4S { rd, rn } => simd_unary(0x4E218800, *rn, *rd),
            Inst::FrintmV4S { rd, rn } => simd_unary(0x4E219800, *rn, *rd),
            Inst::FrintpV4S { rd, rn } => simd_unary(0x4EA18800, *rn, *rd),
            Inst::FrintzV4S { rd, rn } => simd_unary(0x4EA19800, *rn, *rd),
            Inst::FrintaV4S { rd, rn } => simd_unary(0x6E218800, *rn, *rd),
            Inst::FrintiV4S { rd, rn } => simd_unary(0x6EA19800, *rn, *rd),
            Inst::FdivS { rd, rn, rm } => fp_arith(0b00, 0b0001, *rm, *rn, *rd),
            Inst::FmovRegD { rd, rn } => fp_mov_reg(0x1E604000, *rn, *rd),
            Inst::FmovRegS { rd, rn } => fp_mov_reg(0x1E204000, *rn, *rd),
            Inst::MovV8B { rd, rn } => simd_mov_reg(0x0EA01C00, *rn, *rd),
            Inst::MovV16B { rd, rn } => simd_mov_reg(0x4EA01C00, *rn, *rd),
            Inst::MovV4S { rd, rn } => simd_mov_reg(0x4EA01C00, *rn, *rd),
            Inst::MovV2D { rd, rn } => simd_mov_reg(0x4EA01C00, *rn, *rd),
            Inst::DupV16B { rd, rn, index } => simd_dup_lane(0, *rn, *index, *rd),
            Inst::DupV8H { rd, rn, index } => simd_dup_lane(1, *rn, *index, *rd),
            Inst::DupV4S { rd, rn, index } => simd_dup_lane(2, *rn, *index, *rd),
            Inst::DupV2D { rd, rn, index } => simd_dup_lane(3, *rn, *index, *rd),
            Inst::AndV16B { rd, rn, rm } => simd_binary(0x4E201C00, *rm, *rn, *rd),
            Inst::BicV16B { rd, rn, rm } => simd_binary(0x4E601C00, *rm, *rn, *rd),
            Inst::OrrV16B { rd, rn, rm } => simd_binary(0x4EA01C00, *rm, *rn, *rd),
            Inst::EorV16B { rd, rn, rm } => simd_binary(0x6E201C00, *rm, *rn, *rd),
            Inst::BifV16B { rd, rn, rm } => simd_binary(0x6EE01C00, *rm, *rn, *rd),
            Inst::BitV16B { rd, rn, rm } => simd_binary(0x6EA01C00, *rm, *rn, *rd),
            Inst::BslV16B { rd, rn, rm } => simd_binary(0x6E601C00, *rm, *rn, *rd),
            Inst::CmeqV4S { rd, rn, rm } => simd_binary(0x6EA08C00, *rm, *rn, *rd),
            Inst::FcmeqV4S { rd, rn, rm } => simd_binary(0x4E20E400, *rm, *rn, *rd),
            Inst::CmhsV4S { rd, rn, rm } => simd_binary(0x6EA03C00, *rm, *rn, *rd),
            Inst::CmhiV4S { rd, rn, rm } => simd_binary(0x6EA03400, *rm, *rn, *rd),
            Inst::CmgeV4S { rd, rn, rm } => simd_binary(0x4EA03C00, *rm, *rn, *rd),
            Inst::FcmgeV4S { rd, rn, rm } => simd_binary(0x6E20E400, *rm, *rn, *rd),
            Inst::CmgtV4S { rd, rn, rm } => simd_binary(0x4EA03400, *rm, *rn, *rd),
            Inst::FcmgtV4S { rd, rn, rm } => simd_binary(0x6EA0E400, *rm, *rn, *rd),
            Inst::ExtV16B { rd, rn, rm, index } => simd_ext_16b(*rm, *rn, *rd, *index),
            Inst::Rev64V4S { rd, rn } => simd_unary(0x4EA00800, *rn, *rd),
            Inst::Zip1V4S { rd, rn, rm } => simd_binary(0x4E803800, *rm, *rn, *rd),
            Inst::Zip2V4S { rd, rn, rm } => simd_binary(0x4E807800, *rm, *rn, *rd),
            Inst::Uzp1V4S { rd, rn, rm } => simd_binary(0x4E801800, *rm, *rn, *rd),
            Inst::Uzp2V4S { rd, rn, rm } => simd_binary(0x4E805800, *rm, *rn, *rd),
            Inst::Trn1V4S { rd, rn, rm } => simd_binary(0x4E802800, *rm, *rn, *rd),
            Inst::Trn2V4S { rd, rn, rm } => simd_binary(0x4E806800, *rm, *rn, *rd),
            Inst::TblV16B {
                rd,
                table,
                table_len,
                index,
            } => simd_table_lookup(0x4E000000, *index, *table, *table_len, *rd),
            Inst::TbxV16B {
                rd,
                table,
                table_len,
                index,
            } => simd_table_lookup(0x4E001000, *index, *table, *table_len, *rd),
            Inst::MovFromLaneS { rd, rn, index } => simd_extract_lane_s(*rn, *index, *rd),
            Inst::MovFromLaneD { rd, rn, index } => simd_extract_lane_d(*rn, *index, *rd),
            Inst::MovFromLaneGpS { rd, rn, index } => simd_extract_lane_gp(2, *rn, *index, *rd),
            Inst::MovFromLaneGpD { rd, rn, index } => simd_extract_lane_gp(3, *rn, *index, *rd),
            Inst::UmovFromLaneH { rd, rn, index } => simd_extract_lane_gp(1, *rn, *index, *rd),
            Inst::UmovFromLaneB { rd, rn, index } => simd_extract_lane_gp(0, *rn, *index, *rd),
            Inst::SmovFromLaneH { rd, rn, index } => simd_extract_lane_gp_signed(1, *rn, *index, *rd),
            Inst::SmovFromLaneB { rd, rn, index } => simd_extract_lane_gp_signed(0, *rn, *index, *rd),
            Inst::MovLaneS {
                rd,
                rd_index,
                rn,
                rn_index,
            } => simd_insert_lane_s(*rn, *rn_index, *rd, *rd_index),
            Inst::MovLaneD {
                rd,
                rd_index,
                rn,
                rn_index,
            } => simd_insert_lane_d(*rn, *rn_index, *rd, *rd_index),
            Inst::MovLaneH {
                rd,
                rd_index,
                rn,
                rn_index,
            } => simd_insert_lane_h(*rn, *rn_index, *rd, *rd_index),
            Inst::MovLaneB {
                rd,
                rd_index,
                rn,
                rn_index,
            } => simd_insert_lane_b(*rn, *rn_index, *rd, *rd_index),
            Inst::MovLaneFromGpS { rd, rd_index, rn } => {
                simd_insert_lane_gp(2, *rn, *rd_index, *rd)
            }
            Inst::MovLaneFromGpD { rd, rd_index, rn } => {
                simd_insert_lane_gp(3, *rn, *rd_index, *rd)
            }
            Inst::MovLaneFromGpH { rd, rd_index, rn } => {
                simd_insert_lane_gp(1, *rn, *rd_index, *rd)
            }
            Inst::MovLaneFromGpB { rd, rd_index, rn } => {
                simd_insert_lane_gp(0, *rn, *rd_index, *rd)
            }

            Inst::FnegD { rd, rn } => fp_unary(0b01, 0b0000_10, *rn, *rd),
            Inst::FnegS { rd, rn } => fp_unary(0b00, 0b0000_10, *rn, *rd),
            Inst::FabsD { rd, rn } => fp_unary(0b01, 0b0000_01, *rn, *rd),
            Inst::FabsS { rd, rn } => fp_unary(0b00, 0b0000_01, *rn, *rd),
            Inst::FsqrtD { rd, rn } => fp_unary(0b01, 0b0000_11, *rn, *rd),
            Inst::FsqrtS { rd, rn } => fp_unary(0b00, 0b0000_11, *rn, *rd),
            Inst::FcmpD { rn, rm } => fp_cmp(0b01, *rn, *rm),
            Inst::FcmpS { rn, rm } => fp_cmp(0b00, *rn, *rm),
            Inst::FmovImmD { rd, imm8 } => fp_imm(0b01, *imm8, *rd),
            Inst::FmovImmS { rd, imm8 } => fp_imm(0b00, *imm8, *rd),
            Inst::FcselD { rd, rn, rm, cond } => fp_csel(0b01, *rm, *cond, *rn, *rd),
            Inst::FcselS { rd, rn, rm, cond } => fp_csel(0b00, *rm, *cond, *rn, *rd),
            Inst::FmaddD { rd, rn, rm, ra } => fp_madd(0b01, *rd, *rn, *rm, *ra),
            Inst::FmaddS { rd, rn, rm, ra } => fp_madd(0b00, *rd, *rn, *rm, *ra),

            // ---- FP / integer conversion ----
            Inst::FcvtzsD { rd, rn } => {
                (0b1_00_11110_01_1 << 21) | (0b11_000 << 16) | (rn.enc() << 5) | rd.enc()
            }
            Inst::ScvtfD { rd, rn } => {
                (0b1_00_11110_01_1 << 21) | (0b00_010 << 16) | (rn.enc() << 5) | rd.enc()
            }
            Inst::FmovToD { rd, rn } => {
                (0b1_00_11110_01_1 << 21) | (0b00_111 << 16) | (rn.enc() << 5) | rd.enc()
            }
            Inst::FmovToS { rd, rn } => 0x1E270000 | (rn.enc() << 5) | rd.enc(),
            Inst::FmovFromD { rd, rn } => {
                (0b1_00_11110_01_1 << 21) | (0b00_110 << 16) | (rn.enc() << 5) | rd.enc()
            }
            Inst::FmovFromS { rd, rn } => 0x1E260000 | (rn.enc() << 5) | rd.enc(),

            // ---- System ----
            Inst::Svc { imm16 } => (0b11010100_000 << 21) | ((*imm16 as u32) << 5) | 0b000_01,
            Inst::Nop => hint(0),
            Inst::Yield => hint(1),
            Inst::Wfe => hint(2),
            Inst::Wfi => hint(3),
            Inst::Sev => hint(4),
            Inst::Sevl => hint(5),
            Inst::Dmb { option } => barrier(0xD50330BF, *option),
            Inst::Dsb { option } => barrier(0xD503309F, *option),
            Inst::Isb { option } => barrier(0xD50330DF, *option),
            Inst::Brk { imm16 } => (0b11010100_001 << 21) | ((*imm16 as u32) << 5),
        }
    }
}

// ---- Encoding helpers ----

#[allow(clippy::too_many_arguments)]
fn dp_reg(
    sf: bool,
    opc: u32,
    fixed: u32,
    shift: u32,
    rm: GpReg,
    imm6: u32,
    rn: GpReg,
    rd: GpReg,
) -> u32 {
    ((sf as u32) << 31)
        | (opc << 29)
        | (fixed << 24)
        | (shift << 22)
        | (rm.enc() << 16)
        | (imm6 << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

fn logic_reg(sf: bool, opc: u32, n: bool, rm: GpReg, rn: GpReg, rd: GpReg) -> u32 {
    ((sf as u32) << 31)
        | (opc << 29)
        | (0b01010 << 24)
        | ((n as u32) << 21)
        | (rm.enc() << 16)
        | (rn.enc() << 5)
        | rd.enc()
}

fn logical_imm(sf: bool, opc: u32, imm: u64, rn: GpReg, rd: GpReg) -> u32 {
    let width = if sf { 64 } else { 32 };
    let imm = if sf { imm } else { imm & 0xFFFF_FFFF };
    let (n, immr, imms) =
        encode_logical_immediate(imm, width).expect("logical immediate validated by parser");
    ((sf as u32) << 31)
        | (opc << 29)
        | (0b100100 << 23)
        | ((n as u32) << 22)
        | ((immr as u32) << 16)
        | ((imms as u32) << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

fn dp_imm(sf: bool, op: u32, imm12: u16, shift: bool, rn: GpReg, rd: GpReg) -> u32 {
    ((sf as u32) << 31)
        | (op << 29)
        | (0b100010 << 23)
        | ((shift as u32) << 22)
        | ((imm12 as u32) << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

fn dp_ext(
    sf: bool,
    op: u32,
    rm: GpReg,
    extend: RegExtend,
    amount: u8,
    rn: GpReg,
    rd: GpReg,
) -> u32 {
    ((sf as u32) << 31)
        | (op << 29)
        | (0b01011 << 24)
        | (1 << 21)
        | (rm.enc() << 16)
        | (extend.enc() << 13)
        | ((amount as u32) << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

fn mov_wide(sf: bool, opc: u32, imm16: u16, shift: u8, rd: GpReg) -> u32 {
    let hw = (shift / 16) as u32;
    ((sf as u32) << 31)
        | (opc << 29)
        | (0b100101 << 23)
        | (hw << 21)
        | ((imm16 as u32) << 5)
        | rd.enc()
}

fn bitfield(sf: bool, opc: u32, immr: u8, imms: u8, rn: GpReg, rd: GpReg) -> u32 {
    let n = sf as u32;
    ((sf as u32) << 31)
        | (opc << 29)
        | (0b100110 << 23)
        | (n << 22)
        | ((immr as u32) << 16)
        | ((imms as u32) << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

fn encode_logical_immediate(imm: u64, width: u8) -> Option<(bool, u8, u8)> {
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let imm = imm & mask;
    if imm == 0 || imm == mask {
        return None;
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
                let pattern = rotate_right_with_width(base, rot, esize);
                let candidate = replicate_pattern(pattern, esize, width);
                if candidate == imm {
                    let n = esize == 64;
                    let imms =
                        (((!(u64::from(esize) - 1)) << 1) | u64::from(ones - 1)) as u8 & 0x3F;
                    return Some((n, rot, imms));
                }
            }
        }
    }
    None
}

fn rotate_right_with_width(value: u64, rot: u8, width: u8) -> u64 {
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

fn replicate_pattern(pattern: u64, esize: u8, width: u8) -> u64 {
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

fn csel(sf: bool, variant: u32, rm: GpReg, cond: Cond, rn: GpReg, rd: GpReg) -> u32 {
    ((sf as u32) << 31)
        | (((variant >> 1) & 0x1) << 30)
        | (0b0011010100 << 21)
        | (rm.enc() << 16)
        | (cond.enc() << 12)
        | ((variant & 0x1) << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

fn ccmp_imm(sf: bool, is_cmp: bool, rn: GpReg, imm5: u8, nzcv: u8, cond: Cond) -> u32 {
    let base = match (sf, is_cmp) {
        (false, true) => 0x7A400800,
        (false, false) => 0x3A400800,
        (true, true) => 0xFA400800,
        (true, false) => 0xBA400800,
    };
    base | (((imm5 as u32) & 0x1F) << 16)
        | ((cond.enc() & 0xF) << 12)
        | ((rn.enc() & 0x1F) << 5)
        | ((nzcv as u32) & 0xF)
}

fn ldst_idx(size: u32, opc: u32, offset: i16, idx: u32, rn: GpReg, rt: GpReg) -> u32 {
    let imm9 = (offset as u32) & 0x1FF;
    (size << 30)
        | (0b111_0_00 << 24)
        | (opc << 22)
        | (imm9 << 12)
        | (idx << 10)
        | (rn.enc() << 5)
        | rt.enc()
}

fn ldst_uimm_fp(size: u32, opc: u32, scale: u8, offset: u16, rn: GpReg, rt: FpReg) -> u32 {
    let uoff = ((offset as u32) >> scale) & 0xFFF;
    (size << 30) | (0b111_1_01 << 24) | (opc << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
}

fn ldst_reg(
    size: u32,
    opc: u32,
    rm: GpReg,
    extend: AddrExtend,
    shift: bool,
    rn: GpReg,
    rt: GpReg,
) -> u32 {
    (size << 30)
        | (0b111 << 27)
        | (opc << 22)
        | (1 << 21)
        | (rm.enc() << 16)
        | (extend.enc() << 13)
        | ((shift as u32) << 12)
        | (0b10 << 10)
        | (rn.enc() << 5)
        | rt.enc()
}

fn ldst_reg_fp(
    size: u32,
    opc: u32,
    rm: GpReg,
    extend: AddrExtend,
    shift: bool,
    rn: GpReg,
    rt: FpReg,
) -> u32 {
    ldst_reg(size, opc, rm, extend, shift, rn, GpReg::new(rt.num())) | (1 << 26)
}

fn ldr_lit(opc: u32, offset: i32, rt: GpReg) -> u32 {
    let imm19 = ((offset >> 2) as u32) & 0x7FFFF;
    (opc << 30) | (0b011 << 27) | (imm19 << 5) | rt.enc()
}

fn ldr_lit_fp(opc: u32, offset: i32, rt: FpReg) -> u32 {
    ldr_lit(opc, offset, GpReg::new(rt.num())) | (1 << 26)
}

fn ldst_idx_fp(size: u32, opc: u32, offset: i16, idx: u32, rn: GpReg, rt: FpReg) -> u32 {
    ldst_idx(size, opc, offset, idx, rn, GpReg::new(rt.num())) | (1 << 26)
}

/// Load/store pair.
/// Format: opc(2)|101|mode(3)|L(1)|imm7(7)|Rt2(5)|Rn(5)|Rt1(5)
/// mode: 001=post-index, 010=signed-offset, 011=pre-index
fn ldp_stp(opc: u32, mode: u32, l: u32, offset: i16, rt2: GpReg, rn: GpReg, rt1: GpReg) -> u32 {
    let scale = if opc == 0b00 { 2 } else { 3 };
    let imm7 = ((offset >> scale) as u32) & 0x7F;
    (opc << 30)
        | (0b101 << 27)
        | (mode << 23)
        | (l << 22)
        | (imm7 << 15)
        | (rt2.enc() << 10)
        | (rn.enc() << 5)
        | rt1.enc()
}

fn ldp_stp_fp(opc: u32, mode: u32, l: u32, offset: i16, rt2: FpReg, rn: GpReg, rt1: FpReg) -> u32 {
    let scale = match opc {
        0b00 => 2,
        0b01 => 3,
        0b10 => 4,
        _ => unreachable!("invalid FP pair opc"),
    };
    let imm7 = ((offset >> scale) as u32) & 0x7F;
    (opc << 30)
        | (0b101 << 27)
        | (1 << 26)
        | (mode << 23)
        | (l << 22)
        | (imm7 << 15)
        | (rt2.enc() << 10)
        | (rn.enc() << 5)
        | rt1.enc()
}

fn hint(imm: u32) -> u32 {
    0xD503201F | (imm << 5)
}

fn barrier(base: u32, option: BarrierOpt) -> u32 {
    base | (option.enc() << 8)
}

/// FP one-source (FNEG, FABS, FSQRT).
/// Format: 0|00|11110|ftype(2)|1|opcode(6)|10000|Rn(5)|Rd(5)
fn fp_unary(ftype: u32, opcode: u32, rn: FpReg, rd: FpReg) -> u32 {
    (0b000_11110 << 24)
        | (ftype << 22)
        | (1 << 21)
        | (opcode << 15)
        | (0b10000 << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

/// FCMP Rn, Rm.
/// Format: 0|00|11110|ftype(2)|1|Rm(5)|00|1000|Rn(5)|00000
fn fp_cmp(ftype: u32, rn: FpReg, rm: FpReg) -> u32 {
    (0b000_11110 << 24)
        | (ftype << 22)
        | (1 << 21)
        | (rm.enc() << 16)
        | (0b00_1000 << 10)
        | (rn.enc() << 5)
}

fn fp_imm(ftype: u32, imm8: u8, rd: FpReg) -> u32 {
    (0b000_11110 << 24) | (ftype << 22) | (1 << 21) | ((imm8 as u32) << 13) | (1 << 12) | rd.enc()
}

fn fp_csel(ftype: u32, rm: FpReg, cond: Cond, rn: FpReg, rd: FpReg) -> u32 {
    (0b000_11110 << 24)
        | (ftype << 22)
        | (1 << 21)
        | (rm.enc() << 16)
        | (cond.enc() << 12)
        | (0b11 << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

/// FMADD Rd, Rn, Rm, Ra.
/// Format: 0|00|11111|ftype(2)|0|Rm(5)|0|Ra(5)|Rn(5)|Rd(5)
fn fp_madd(ftype: u32, rd: FpReg, rn: FpReg, rm: FpReg, ra: FpReg) -> u32 {
    (0b000_11111 << 24)
        | (ftype << 22)
        | (rm.enc() << 16)
        | (ra.enc() << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

/// FP two-source arithmetic (FADD, FSUB, FMUL, FDIV).
fn fp_arith(ftype: u32, opcode: u32, rm: FpReg, rn: FpReg, rd: FpReg) -> u32 {
    (0b000_11110 << 24)
        | (ftype << 22)
        | (1 << 21)
        | (rm.enc() << 16)
        | (opcode << 12)
        | (0b10 << 10)
        | (rn.enc() << 5)
        | rd.enc()
}

fn simd_fp_arith_4s(base: u32, rm: FpReg, rn: FpReg, rd: FpReg) -> u32 {
    base | (rm.enc() << 16) | (rn.enc() << 5) | rd.enc()
}

fn simd_fp_arith_2d(base: u32, rm: FpReg, rn: FpReg, rd: FpReg) -> u32 {
    base | (rm.enc() << 16) | (rn.enc() << 5) | rd.enc()
}

fn simd_binary(base: u32, rm: FpReg, rn: FpReg, rd: FpReg) -> u32 {
    base | (rm.enc() << 16) | (rn.enc() << 5) | rd.enc()
}

fn simd_unary(base: u32, rn: FpReg, rd: FpReg) -> u32 {
    base | (rn.enc() << 5) | rd.enc()
}

fn simd_mov_reg(base: u32, rn: FpReg, rd: FpReg) -> u32 {
    base | (rn.enc() << 16) | (rn.enc() << 5) | rd.enc()
}

fn simd_reduce(base: u32, rn: FpReg, rd: FpReg) -> u32 {
    base | (rn.enc() << 5) | rd.enc()
}

fn simd_reduce_4s(base: u32, rn: FpReg, rd: FpReg) -> u32 {
    simd_reduce(base, rn, rd)
}

fn simd_lane_imm5(size_log2: u8, index: u8) -> u32 {
    ((1u32) << size_log2) | ((index as u32) << (size_log2 + 1))
}

fn simd_dup_lane(size_log2: u8, rn: FpReg, index: u8, rd: FpReg) -> u32 {
    0x4E000400 | (simd_lane_imm5(size_log2, index) << 16) | (rn.enc() << 5) | rd.enc()
}

fn simd_ext_16b(rm: FpReg, rn: FpReg, rd: FpReg, index: u8) -> u32 {
    0x6E000000 | (rm.enc() << 16) | ((index as u32) << 11) | (rn.enc() << 5) | rd.enc()
}

fn simd_table_lookup(base: u32, index: FpReg, table: FpReg, table_len: u8, rd: FpReg) -> u32 {
    assert!((1..=4).contains(&table_len), "table length must be 1..=4");
    base | (index.enc() << 16) | (((table_len as u32) - 1) << 13) | (table.enc() << 5) | rd.enc()
}

fn fp_mov_reg(base: u32, rn: FpReg, rd: FpReg) -> u32 {
    base | (rn.enc() << 5) | rd.enc()
}

fn simd_extract_lane_s(rn: FpReg, index: u8, rd: FpReg) -> u32 {
    0x5E040400 | ((index as u32) << 19) | (rn.enc() << 5) | rd.enc()
}

fn simd_extract_lane_d(rn: FpReg, index: u8, rd: FpReg) -> u32 {
    0x5E080400 | ((index as u32) << 20) | (rn.enc() << 5) | rd.enc()
}

fn simd_extract_lane_gp(size_log2: u8, rn: FpReg, index: u8, rd: GpReg) -> u32 {
    let base = if size_log2 == 3 {
        0x4E003C00
    } else {
        0x0E003C00
    };
    base | (simd_lane_imm5(size_log2, index) << 16) | (rn.enc() << 5) | rd.enc()
}

fn simd_extract_lane_gp_signed(size_log2: u8, rn: FpReg, index: u8, rd: GpReg) -> u32 {
    0x0E002C00 | (simd_lane_imm5(size_log2, index) << 16) | (rn.enc() << 5) | rd.enc()
}

fn simd_insert_lane_s(rn: FpReg, rn_index: u8, rd: FpReg, rd_index: u8) -> u32 {
    0x6E040400 | ((rd_index as u32) << 19) | ((rn_index as u32) << 13) | (rn.enc() << 5) | rd.enc()
}

fn simd_insert_lane_d(rn: FpReg, rn_index: u8, rd: FpReg, rd_index: u8) -> u32 {
    0x6E080400 | ((rd_index as u32) << 20) | ((rn_index as u32) << 14) | (rn.enc() << 5) | rd.enc()
}

fn simd_insert_lane_h(rn: FpReg, rn_index: u8, rd: FpReg, rd_index: u8) -> u32 {
    0x6E020400 | ((rd_index as u32) << 18) | ((rn_index as u32) << 12) | (rn.enc() << 5) | rd.enc()
}

fn simd_insert_lane_b(rn: FpReg, rn_index: u8, rd: FpReg, rd_index: u8) -> u32 {
    0x6E010400 | ((rd_index as u32) << 17) | ((rn_index as u32) << 11) | (rn.enc() << 5) | rd.enc()
}

fn simd_insert_lane_gp(size_log2: u8, rn: GpReg, index: u8, rd: FpReg) -> u32 {
    0x4E001C00 | (simd_lane_imm5(size_log2, index) << 16) | (rn.enc() << 5) | rd.enc()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reg::*;

    // ================================================================
    // Ground truth: every expected value was extracted from Apple `as`
    // by assembling the instruction and reading the 4-byte encoding
    // with `otool -t`.
    // ================================================================

    // ---- Data processing (register) ----

    #[test]
    fn add_x0_x1_x2() {
        assert_eq!(
            Inst::AddReg {
                rd: X0,
                rn: X1,
                rm: X2,
                sf: true
            }
            .encode(),
            0x8B020020
        );
    }
    #[test]
    fn sub_x3_x4_x5() {
        assert_eq!(
            Inst::SubReg {
                rd: X3,
                rn: X4,
                rm: X5,
                sf: true
            }
            .encode(),
            0xCB050083
        );
    }
    #[test]
    fn add_w0_w1_w2() {
        assert_eq!(
            Inst::AddReg {
                rd: W0,
                rn: W1,
                rm: W2,
                sf: false
            }
            .encode(),
            0x0B020020
        );
    }
    #[test]
    fn sub_w3_w4_w5() {
        assert_eq!(
            Inst::SubReg {
                rd: W3,
                rn: W4,
                rm: W5,
                sf: false
            }
            .encode(),
            0x4B050083
        );
    }
    #[test]
    fn add_x0_x1_x2_lsl3() {
        assert_eq!(
            Inst::AddShiftReg {
                rd: X0,
                rn: X1,
                rm: X2,
                shift: RegShift::Lsl,
                amount: 3,
                sf: true
            }
            .encode(),
            0x8B020C20
        );
    }
    #[test]
    fn add_x0_x0_w1_sxtw() {
        assert_eq!(
            Inst::AddExtReg {
                rd: X0,
                rn: X0,
                rm: W1,
                extend: RegExtend::Sxtw,
                amount: 0,
                sf: true
            }
            .encode(),
            0x8B21C000
        );
    }
    #[test]
    fn add_x0_x0_w1_sxtw3() {
        assert_eq!(
            Inst::AddExtReg {
                rd: X0,
                rn: X0,
                rm: W1,
                extend: RegExtend::Sxtw,
                amount: 3,
                sf: true
            }
            .encode(),
            0x8B21CC00
        );
    }
    #[test]
    fn sub_w3_w4_w5_asr7() {
        assert_eq!(
            Inst::SubShiftReg {
                rd: W3,
                rn: W4,
                rm: W5,
                shift: RegShift::Asr,
                amount: 7,
                sf: false
            }
            .encode(),
            0x4B851C83
        );
    }
    #[test]
    fn sub_x2_x3_w4_uxtw2() {
        assert_eq!(
            Inst::SubExtReg {
                rd: X2,
                rn: X3,
                rm: W4,
                extend: RegExtend::Uxtw,
                amount: 2,
                sf: true
            }
            .encode(),
            0xCB244862
        );
    }
    #[test]
    fn subs_w10_w8_w9_uxtb() {
        assert_eq!(
            Inst::SubsExtReg {
                rd: W10,
                rn: W8,
                rm: W9,
                extend: RegExtend::Uxtb,
                amount: 0,
                sf: false
            }
            .encode(),
            0x6B29010A
        );
    }
    #[test]
    fn subs_w11_w12_w13_uxth() {
        assert_eq!(
            Inst::SubsExtReg {
                rd: W11,
                rn: W12,
                rm: W13,
                extend: RegExtend::Uxth,
                amount: 0,
                sf: false
            }
            .encode(),
            0x6B2D218B
        );
    }
    #[test]
    fn subs_x14_x15_w16_sxtb() {
        assert_eq!(
            Inst::SubsExtReg {
                rd: X14,
                rn: X15,
                rm: W16,
                extend: RegExtend::Sxtb,
                amount: 0,
                sf: true
            }
            .encode(),
            0xEB3081EE
        );
    }
    #[test]
    fn subs_x17_x18_w19_sxth1() {
        assert_eq!(
            Inst::SubsExtReg {
                rd: X17,
                rn: X18,
                rm: W19,
                extend: RegExtend::Sxth,
                amount: 1,
                sf: true
            }
            .encode(),
            0xEB33A651
        );
    }
    #[test]
    fn cmp_x6_x7_lsr4() {
        assert_eq!(
            Inst::SubsShiftReg {
                rd: XZR,
                rn: X6,
                rm: X7,
                shift: RegShift::Lsr,
                amount: 4,
                sf: true
            }
            .encode(),
            0xEB4710DF
        );
    }
    #[test]
    fn mul_x6_x7_x8() {
        assert_eq!(
            Inst::Mul {
                rd: X6,
                rn: X7,
                rm: X8,
                sf: true
            }
            .encode(),
            0x9B087CE6
        );
    }
    #[test]
    fn madd_w0_w0_w0_w8() {
        assert_eq!(
            Inst::Madd {
                rd: W0,
                rn: W0,
                rm: W0,
                ra: W8,
                sf: false
            }
            .encode(),
            0x1B002000
        );
    }
    #[test]
    fn umull_x9_w8_w9() {
        assert_eq!(
            Inst::Umull {
                rd: X9,
                rn: W8,
                rm: W9
            }
            .encode(),
            0x9BA97D09
        );
    }
    #[test]
    fn msub_w9_w8_w1_w0() {
        assert_eq!(
            Inst::Msub {
                rd: W9,
                rn: W8,
                rm: W1,
                ra: W0,
                sf: false
            }
            .encode(),
            0x1B018109
        );
    }
    #[test]
    fn sdiv_x9_x10_x11() {
        assert_eq!(
            Inst::Sdiv {
                rd: X9,
                rn: X10,
                rm: X11,
                sf: true
            }
            .encode(),
            0x9ACB0D49
        );
    }
    #[test]
    fn udiv_x12_x13_x14() {
        assert_eq!(
            Inst::Udiv {
                rd: X12,
                rn: X13,
                rm: X14,
                sf: true
            }
            .encode(),
            0x9ACE09AC
        );
    }

    // ---- Logic (register) ----

    #[test]
    fn and_x0_x1_x2() {
        assert_eq!(
            Inst::AndReg {
                rd: X0,
                rn: X1,
                rm: X2,
                sf: true
            }
            .encode(),
            0x8A020020
        );
    }
    #[test]
    fn orr_x0_x1_x2() {
        assert_eq!(
            Inst::OrrReg {
                rd: X0,
                rn: X1,
                rm: X2,
                sf: true
            }
            .encode(),
            0xAA020020
        );
    }
    #[test]
    fn orn_x0_xzr_x1() {
        assert_eq!(
            Inst::OrnReg {
                rd: X0,
                rn: XZR,
                rm: X1,
                sf: true
            }
            .encode(),
            0xAA2103E0
        );
    }
    #[test]
    fn eor_x0_x1_x2() {
        assert_eq!(
            Inst::EorReg {
                rd: X0,
                rn: X1,
                rm: X2,
                sf: true
            }
            .encode(),
            0xCA020020
        );
    }
    #[test]
    fn and_w8_w8_0x7() {
        assert_eq!(
            Inst::AndImm {
                rd: W8,
                rn: W8,
                imm: 0x7,
                sf: false
            }
            .encode(),
            0x12000908
        );
    }
    #[test]
    fn and_x9_x9_0xff() {
        assert_eq!(
            Inst::AndImm {
                rd: X9,
                rn: X9,
                imm: 0xFF,
                sf: true
            }
            .encode(),
            0x92401D29
        );
    }

    // ---- Data processing (immediate) ----

    #[test]
    fn add_x0_x1_42() {
        assert_eq!(
            Inst::AddImm {
                rd: X0,
                rn: X1,
                imm12: 42,
                shift: false,
                sf: true
            }
            .encode(),
            0x9100A820
        );
    }
    #[test]
    fn sub_x0_x1_42() {
        assert_eq!(
            Inst::SubImm {
                rd: X0,
                rn: X1,
                imm12: 42,
                shift: false,
                sf: true
            }
            .encode(),
            0xD100A820
        );
    }
    #[test]
    fn add_x0_x1_42_lsl12() {
        assert_eq!(
            Inst::AddImm {
                rd: X0,
                rn: X1,
                imm12: 42,
                shift: true,
                sf: true
            }
            .encode(),
            0x9140A820
        );
    }

    // ---- Move (wide immediate) ----

    #[test]
    fn movz_x0_0x1234() {
        assert_eq!(
            Inst::Movz {
                rd: X0,
                imm16: 0x1234,
                shift: 0,
                sf: true
            }
            .encode(),
            0xD2824680
        );
    }
    #[test]
    fn movz_x0_0x5678_lsl16() {
        assert_eq!(
            Inst::Movz {
                rd: X0,
                imm16: 0x5678,
                shift: 16,
                sf: true
            }
            .encode(),
            0xD2AACF00
        );
    }
    #[test]
    fn movk_x0_0xabcd_lsl32() {
        assert_eq!(
            Inst::Movk {
                rd: X0,
                imm16: 0xABCD,
                shift: 32,
                sf: true
            }
            .encode(),
            0xF2D579A0
        );
    }
    #[test]
    fn movn_x0_0() {
        assert_eq!(
            Inst::Movn {
                rd: X0,
                imm16: 0,
                shift: 0,
                sf: true
            }
            .encode(),
            0x92800000
        );
    }

    // ---- Compare (SUBS/ADDS/ANDS to XZR) ----

    #[test]
    fn cmp_x0_x1() {
        assert_eq!(
            Inst::SubsReg {
                rd: XZR,
                rn: X0,
                rm: X1,
                sf: true
            }
            .encode(),
            0xEB01001F
        );
    }
    #[test]
    fn cmn_x0_x1() {
        assert_eq!(
            Inst::AddsReg {
                rd: XZR,
                rn: X0,
                rm: X1,
                sf: true
            }
            .encode(),
            0xAB01001F
        );
    }
    #[test]
    fn tst_x0_x1() {
        assert_eq!(
            Inst::AndsReg {
                rd: XZR,
                rn: X0,
                rm: X1,
                sf: true
            }
            .encode(),
            0xEA01001F
        );
    }

    // ---- Shifts ----

    #[test]
    fn lsl_x0_x1_3() {
        assert_eq!(
            Inst::LslImm {
                rd: X0,
                rn: X1,
                amount: 3,
                sf: true
            }
            .encode(),
            0xD37DF020
        );
    }
    #[test]
    fn lsr_x0_x1_3() {
        assert_eq!(
            Inst::LsrImm {
                rd: X0,
                rn: X1,
                amount: 3,
                sf: true
            }
            .encode(),
            0xD343FC20
        );
    }
    #[test]
    fn asr_x0_x1_3() {
        assert_eq!(
            Inst::AsrImm {
                rd: X0,
                rn: X1,
                amount: 3,
                sf: true
            }
            .encode(),
            0x9343FC20
        );
    }

    // W-register shifts (32-bit, different immr/imms field widths)
    #[test]
    fn lsl_w0_w1_3() {
        assert_eq!(
            Inst::LslImm {
                rd: W0,
                rn: W1,
                amount: 3,
                sf: false
            }
            .encode(),
            0x531D7020
        );
    }
    #[test]
    fn lsr_w5_w6_8() {
        assert_eq!(
            Inst::LsrImm {
                rd: W5,
                rn: W6,
                amount: 8,
                sf: false
            }
            .encode(),
            0x53087CC5
        );
    }
    #[test]
    fn asr_w5_w6_15() {
        assert_eq!(
            Inst::AsrImm {
                rd: W5,
                rn: W6,
                amount: 15,
                sf: false
            }
            .encode(),
            0x130F7CC5
        );
    }
    #[test]
    fn ubfiz_w8_w0_5_3() {
        assert_eq!(
            Inst::Ubfiz {
                rd: W8,
                rn: W0,
                lsb: 5,
                width: 3,
                sf: false
            }
            .encode(),
            0x531B0808
        );
    }
    #[test]
    fn bfi_w0_w8_5_27() {
        assert_eq!(
            Inst::Bfi {
                rd: W0,
                rn: W8,
                lsb: 5,
                width: 27,
                sf: false
            }
            .encode(),
            0x331B6900
        );
    }
    #[test]
    fn bfxil_w8_w0_3_5() {
        assert_eq!(
            Inst::Bfxil {
                rd: W8,
                rn: W0,
                lsb: 3,
                width: 5,
                sf: false
            }
            .encode(),
            0x33031C08
        );
    }

    // ---- Branches ----

    #[test]
    fn b_plus4() {
        assert_eq!(Inst::B { offset: 4 }.encode(), 0x14000001);
    }
    #[test]
    fn b_minus8() {
        assert_eq!(Inst::B { offset: -8 }.encode(), 0x17FFFFFE);
    }
    #[test]
    fn bl_plus16() {
        assert_eq!(Inst::Bl { offset: 16 }.encode(), 0x94000004);
    }
    #[test]
    fn b_eq_plus8() {
        assert_eq!(
            Inst::BCond {
                cond: Cond::EQ,
                offset: 8
            }
            .encode(),
            0x54000040
        );
    }
    #[test]
    fn b_ne_plus12() {
        assert_eq!(
            Inst::BCond {
                cond: Cond::NE,
                offset: 12
            }
            .encode(),
            0x54000061
        );
    }
    #[test]
    fn b_ge_plus16() {
        assert_eq!(
            Inst::BCond {
                cond: Cond::GE,
                offset: 16
            }
            .encode(),
            0x5400008A
        );
    }
    #[test]
    fn cbz_x0_plus8() {
        assert_eq!(
            Inst::Cbz {
                rt: X0,
                offset: 8,
                sf: true
            }
            .encode(),
            0xB4000040
        );
    }
    #[test]
    fn cbnz_x1_plus12() {
        assert_eq!(
            Inst::Cbnz {
                rt: X1,
                offset: 12,
                sf: true
            }
            .encode(),
            0xB5000061
        );
    }
    #[test]
    fn tbz_x0_bit5_plus8() {
        assert_eq!(
            Inst::Tbz {
                rt: X0,
                bit: 5,
                offset: 8,
                sf: true
            }
            .encode(),
            0x36280040
        );
    }
    #[test]
    fn tbnz_x1_bit33_plus12() {
        assert_eq!(
            Inst::Tbnz {
                rt: X1,
                bit: 33,
                offset: 12,
                sf: true
            }
            .encode(),
            0xB7080061
        );
    }
    #[test]
    fn ret_x30() {
        assert_eq!(Inst::Ret { rn: X30 }.encode(), 0xD65F03C0);
    }
    #[test]
    fn br_x16() {
        assert_eq!(Inst::Br { rn: X16 }.encode(), 0xD61F0200);
    }
    #[test]
    fn blr_x17() {
        assert_eq!(Inst::Blr { rn: X17 }.encode(), 0xD63F0220);
    }
    #[test]
    fn csel_w0_w0_w1_gt() {
        assert_eq!(
            Inst::Csel {
                rd: W0,
                rn: W0,
                rm: W1,
                cond: Cond::GT,
                sf: false
            }
            .encode(),
            0x1A81C000
        );
    }
    #[test]
    fn ccmp_w0_3_4_ne() {
        assert_eq!(
            Inst::CcmpImm {
                rn: W0,
                imm5: 3,
                nzcv: 4,
                cond: Cond::NE,
                sf: false
            }
            .encode(),
            0x7A431804
        );
    }
    #[test]
    fn ccmp_w1_10_0_lt() {
        assert_eq!(
            Inst::CcmpImm {
                rn: W1,
                imm5: 10,
                nzcv: 0,
                cond: Cond::LT,
                sf: false
            }
            .encode(),
            0x7A4AB820
        );
    }
    #[test]
    fn ccmp_x2_5_7_eq() {
        assert_eq!(
            Inst::CcmpImm {
                rn: X2,
                imm5: 5,
                nzcv: 7,
                cond: Cond::EQ,
                sf: true
            }
            .encode(),
            0xFA450847
        );
    }
    #[test]
    fn ccmn_w3_9_1_ge() {
        assert_eq!(
            Inst::CcmnImm {
                rn: W3,
                imm5: 9,
                nzcv: 1,
                cond: Cond::GE,
                sf: false
            }
            .encode(),
            0x3A49A861
        );
    }
    #[test]
    fn csinc_x2_x3_x3_ne() {
        assert_eq!(
            Inst::Csinc {
                rd: X2,
                rn: X3,
                rm: X3,
                cond: Cond::NE,
                sf: true
            }
            .encode(),
            0x9A831462
        );
    }
    #[test]
    fn csinv_x2_x3_x4_ne() {
        assert_eq!(
            Inst::Csinv {
                rd: X2,
                rn: X3,
                rm: X4,
                cond: Cond::NE,
                sf: true
            }
            .encode(),
            0xDA841062
        );
    }
    #[test]
    fn csneg_x5_x6_x7_gt() {
        assert_eq!(
            Inst::Csneg {
                rd: X5,
                rn: X6,
                rm: X7,
                cond: Cond::GT,
                sf: true
            }
            .encode(),
            0xDA87C4C5
        );
    }

    // ---- Address generation ----

    #[test]
    fn adr_x0_8() {
        assert_eq!(Inst::Adr { rd: X0, imm: 8 }.encode(), 0x10000040);
    }
    #[test]
    fn adrp_x0_0() {
        assert_eq!(Inst::Adrp { rd: X0, imm: 0 }.encode(), 0x90000000);
    }

    // ---- Load/Store (unsigned offset) ----

    #[test]
    fn ldr_x0_x1() {
        assert_eq!(
            Inst::LdrImm64 {
                rt: X0,
                rn: X1,
                offset: 0
            }
            .encode(),
            0xF9400020
        );
    }
    #[test]
    fn ldr_x0_x1_8() {
        assert_eq!(
            Inst::LdrImm64 {
                rt: X0,
                rn: X1,
                offset: 8
            }
            .encode(),
            0xF9400420
        );
    }
    #[test]
    fn str_x2_x3_16() {
        assert_eq!(
            Inst::StrImm64 {
                rt: X2,
                rn: X3,
                offset: 16
            }
            .encode(),
            0xF9000862
        );
    }
    #[test]
    fn ldr_w4_x5_4() {
        assert_eq!(
            Inst::LdrImm32 {
                rt: W4,
                rn: X5,
                offset: 4
            }
            .encode(),
            0xB94004A4
        );
    }
    #[test]
    fn str_w6_x7_8() {
        assert_eq!(
            Inst::StrImm32 {
                rt: W6,
                rn: X7,
                offset: 8
            }
            .encode(),
            0xB90008E6
        );
    }
    #[test]
    fn ldur_x9_x29_m8() {
        assert_eq!(
            Inst::Ldur64 {
                rt: X9,
                rn: X29,
                offset: -8
            }
            .encode(),
            0xF85F83A9
        );
    }
    #[test]
    fn stur_w6_x7_m4() {
        assert_eq!(
            Inst::Stur32 {
                rt: W6,
                rn: X7,
                offset: -4
            }
            .encode(),
            0xB81FC0E6
        );
    }
    #[test]
    fn ldr_x0_x1_x2() {
        assert_eq!(
            Inst::LdrReg64 {
                rt: X0,
                rn: X1,
                rm: X2,
                extend: AddrExtend::Lsl,
                shift: false
            }
            .encode(),
            0xF8626820
        );
    }
    #[test]
    fn ldr_x3_x4_x5_lsl3() {
        assert_eq!(
            Inst::LdrReg64 {
                rt: X3,
                rn: X4,
                rm: X5,
                extend: AddrExtend::Lsl,
                shift: true
            }
            .encode(),
            0xF8657883
        );
    }
    #[test]
    fn ldr_x6_x7_w8_uxtw3() {
        assert_eq!(
            Inst::LdrReg64 {
                rt: X6,
                rn: X7,
                rm: W8,
                extend: AddrExtend::Uxtw,
                shift: true
            }
            .encode(),
            0xF86858E6
        );
    }
    #[test]
    fn ldr_x9_x10_x11_sxtx3() {
        assert_eq!(
            Inst::LdrReg64 {
                rt: X9,
                rn: X10,
                rm: X11,
                extend: AddrExtend::Sxtx,
                shift: true
            }
            .encode(),
            0xF86BF949
        );
    }
    #[test]
    fn str_x12_x13_x14() {
        assert_eq!(
            Inst::StrReg64 {
                rt: X12,
                rn: X13,
                rm: X14,
                extend: AddrExtend::Lsl,
                shift: false
            }
            .encode(),
            0xF82E69AC
        );
    }
    #[test]
    fn ldrb_w8_x9_1() {
        assert_eq!(
            Inst::Ldrb {
                rt: W8,
                rn: X9,
                offset: 1
            }
            .encode(),
            0x39400528
        );
    }
    #[test]
    fn ldrsb_w0_x1_3() {
        assert_eq!(
            Inst::Ldrsb32 {
                rt: W0,
                rn: X1,
                offset: 3
            }
            .encode(),
            0x39C00C20
        );
    }
    #[test]
    fn ldrsb_x0_x1_3() {
        assert_eq!(
            Inst::Ldrsb64 {
                rt: X0,
                rn: X1,
                offset: 3
            }
            .encode(),
            0x39800C20
        );
    }
    #[test]
    fn ldrb_w9_x1_post_1() {
        assert_eq!(
            Inst::LdrbPost {
                rt: W9,
                rn: X1,
                offset: 1
            }
            .encode(),
            0x38401429
        );
    }
    #[test]
    fn ldrsb_w0_x1_post_1() {
        assert_eq!(
            Inst::LdrsbPost32 {
                rt: W0,
                rn: X1,
                offset: 1
            }
            .encode(),
            0x38C01420
        );
    }
    #[test]
    fn ldrh_w10_x11_2() {
        assert_eq!(
            Inst::Ldrh {
                rt: W10,
                rn: X11,
                offset: 2
            }
            .encode(),
            0x7940056A
        );
    }
    #[test]
    fn ldrsh_w0_x1_4() {
        assert_eq!(
            Inst::Ldrsh32 {
                rt: W0,
                rn: X1,
                offset: 4
            }
            .encode(),
            0x79C00820
        );
    }
    #[test]
    fn ldrsh_x0_x1_4() {
        assert_eq!(
            Inst::Ldrsh64 {
                rt: X0,
                rn: X1,
                offset: 4
            }
            .encode(),
            0x79800820
        );
    }
    #[test]
    fn strb_w9_x8_post_1() {
        assert_eq!(
            Inst::StrbPost {
                rt: W9,
                rn: X8,
                offset: 1
            }
            .encode(),
            0x38001509
        );
    }
    #[test]
    fn strb_w8_x9() {
        assert_eq!(
            Inst::Strb {
                rt: W8,
                rn: X9,
                offset: 0
            }
            .encode(),
            0x39000128
        );
    }
    #[test]
    fn strh_w10_x11_6() {
        assert_eq!(
            Inst::Strh {
                rt: W10,
                rn: X11,
                offset: 6
            }
            .encode(),
            0x79000D6A
        );
    }
    #[test]
    fn ldrh_w3_x4_pre_2() {
        assert_eq!(
            Inst::LdrhPre {
                rt: W3,
                rn: X4,
                offset: 2
            }
            .encode(),
            0x78402C83
        );
    }
    #[test]
    fn ldrsh_w0_x1_pre_2() {
        assert_eq!(
            Inst::LdrshPre32 {
                rt: W0,
                rn: X1,
                offset: 2
            }
            .encode(),
            0x78C02C20
        );
    }
    #[test]
    fn ldrsh_x0_x1_pre_2() {
        assert_eq!(
            Inst::LdrshPre64 {
                rt: X0,
                rn: X1,
                offset: 2
            }
            .encode(),
            0x78802C20
        );
    }
    #[test]
    fn strh_w5_x6_post_2() {
        assert_eq!(
            Inst::StrhPost {
                rt: W5,
                rn: X6,
                offset: 2
            }
            .encode(),
            0x780024C5
        );
    }
    #[test]
    fn ldrsw_x0_x1_4() {
        assert_eq!(
            Inst::Ldrsw {
                rt: X0,
                rn: X1,
                offset: 4
            }
            .encode(),
            0xB9800420
        );
    }
    #[test]
    fn ldrb_w0_x1_x2() {
        assert_eq!(
            Inst::LdrbReg {
                rt: W0,
                rn: X1,
                rm: X2,
                extend: AddrExtend::Lsl,
                shift: false
            }
            .encode(),
            0x38626820
        );
    }
    #[test]
    fn ldrsb_w0_x1_x2() {
        assert_eq!(
            Inst::LdrsbReg32 {
                rt: W0,
                rn: X1,
                rm: X2,
                extend: AddrExtend::Lsl,
                shift: false
            }
            .encode(),
            0x38E26820
        );
    }
    #[test]
    fn ldrsb_x0_x1_x2() {
        assert_eq!(
            Inst::LdrsbReg64 {
                rt: X0,
                rn: X1,
                rm: X2,
                extend: AddrExtend::Lsl,
                shift: false
            }
            .encode(),
            0x38A26820
        );
    }
    #[test]
    fn ldrh_w3_x4_w5_uxtw1() {
        assert_eq!(
            Inst::LdrhReg {
                rt: W3,
                rn: X4,
                rm: W5,
                extend: AddrExtend::Uxtw,
                shift: true
            }
            .encode(),
            0x78655883
        );
    }
    #[test]
    fn ldrsh_w0_x1_w2_uxtw1() {
        assert_eq!(
            Inst::LdrshReg32 {
                rt: W0,
                rn: X1,
                rm: W2,
                extend: AddrExtend::Uxtw,
                shift: true
            }
            .encode(),
            0x78E25820
        );
    }
    #[test]
    fn ldrsh_x0_x1_w2_sxtw1() {
        assert_eq!(
            Inst::LdrshReg64 {
                rt: X0,
                rn: X1,
                rm: W2,
                extend: AddrExtend::Sxtw,
                shift: true
            }
            .encode(),
            0x78A2D820
        );
    }
    #[test]
    fn ldrsw_x6_x7_w8_sxtw2() {
        assert_eq!(
            Inst::LdrswReg {
                rt: X6,
                rn: X7,
                rm: W8,
                extend: AddrExtend::Sxtw,
                shift: true
            }
            .encode(),
            0xB8A8D8E6
        );
    }
    #[test]
    fn ldaprb_w0_x1() {
        assert_eq!(Inst::Ldaprb { rt: W0, rn: X1 }.encode(), 0x38BFC020);
    }
    #[test]
    fn ldaprh_w2_x3() {
        assert_eq!(Inst::Ldaprh { rt: W2, rn: X3 }.encode(), 0x78BFC062);
    }
    #[test]
    fn ldapr_w8_x9() {
        assert_eq!(Inst::Ldapr32 { rt: W8, rn: X9 }.encode(), 0xB8BFC128);
    }
    #[test]
    fn ldapr_x8_x9() {
        assert_eq!(Inst::Ldapr64 { rt: X8, rn: X9 }.encode(), 0xF8BFC128);
    }
    #[test]
    fn stlrb_w4_x5() {
        assert_eq!(Inst::Stlrb { rt: W4, rn: X5 }.encode(), 0x089FFCA4);
    }
    #[test]
    fn stlrh_w6_x7() {
        assert_eq!(Inst::Stlrh { rt: W6, rn: X7 }.encode(), 0x489FFCE6);
    }
    #[test]
    fn ldaddalb_w0_w1_x2() {
        assert_eq!(
            Inst::Ldaddalb {
                rs: W0,
                rt: W1,
                rn: X2
            }
            .encode(),
            0x38E00041
        );
    }
    #[test]
    fn ldaddalh_w3_w4_x5() {
        assert_eq!(
            Inst::Ldaddalh {
                rs: W3,
                rt: W4,
                rn: X5
            }
            .encode(),
            0x78E300A4
        );
    }
    #[test]
    fn ldumaxalb_w0_w1_x2() {
        assert_eq!(
            Inst::Ldumaxalb {
                rs: W0,
                rt: W1,
                rn: X2
            }
            .encode(),
            0x38E06041
        );
    }
    #[test]
    fn ldumaxalh_w3_w4_x5() {
        assert_eq!(
            Inst::Ldumaxalh {
                rs: W3,
                rt: W4,
                rn: X5
            }
            .encode(),
            0x78E360A4
        );
    }
    #[test]
    fn ldsmaxalb_w18_w19_x20() {
        assert_eq!(
            Inst::Ldsmaxalb {
                rs: W18,
                rt: W19,
                rn: X20
            }
            .encode(),
            0x38F24293
        );
    }
    #[test]
    fn ldsmaxalh_w24_w25_x26() {
        assert_eq!(
            Inst::Ldsmaxalh {
                rs: W24,
                rt: W25,
                rn: X26
            }
            .encode(),
            0x78F84359
        );
    }
    #[test]
    fn lduminalb_w12_w13_x14() {
        assert_eq!(
            Inst::Lduminalb {
                rs: W12,
                rt: W13,
                rn: X14
            }
            .encode(),
            0x38EC71CD
        );
    }
    #[test]
    fn lduminalh_w15_w16_x17() {
        assert_eq!(
            Inst::Lduminalh {
                rs: W15,
                rt: W16,
                rn: X17
            }
            .encode(),
            0x78EF7230
        );
    }
    #[test]
    fn ldsminalb_w21_w22_x23() {
        assert_eq!(
            Inst::Ldsminalb {
                rs: W21,
                rt: W22,
                rn: X23
            }
            .encode(),
            0x38F552F6
        );
    }
    #[test]
    fn ldsminalh_w27_w28_x29() {
        assert_eq!(
            Inst::Ldsminalh {
                rs: W27,
                rt: W28,
                rn: X29
            }
            .encode(),
            0x78FB53BC
        );
    }
    #[test]
    fn ldclralb_w6_w7_x8() {
        assert_eq!(
            Inst::Ldclralb {
                rs: W6,
                rt: W7,
                rn: X8
            }
            .encode(),
            0x38E61107
        );
    }
    #[test]
    fn ldclralh_w15_w16_x17() {
        assert_eq!(
            Inst::Ldclralh {
                rs: W15,
                rt: W16,
                rn: X17
            }
            .encode(),
            0x78EF1230
        );
    }
    #[test]
    fn ldeoralb_w3_w4_x5() {
        assert_eq!(
            Inst::Ldeoralb {
                rs: W3,
                rt: W4,
                rn: X5
            }
            .encode(),
            0x38E320A4
        );
    }
    #[test]
    fn ldeoralh_w12_w13_x14() {
        assert_eq!(
            Inst::Ldeoralh {
                rs: W12,
                rt: W13,
                rn: X14
            }
            .encode(),
            0x78EC21CD
        );
    }
    #[test]
    fn ldsetalb_w0_w1_x2() {
        assert_eq!(
            Inst::Ldsetalb {
                rs: W0,
                rt: W1,
                rn: X2
            }
            .encode(),
            0x38E03041
        );
    }
    #[test]
    fn ldsetalh_w9_w10_x11() {
        assert_eq!(
            Inst::Ldsetalh {
                rs: W9,
                rt: W10,
                rn: X11
            }
            .encode(),
            0x78E9316A
        );
    }
    #[test]
    fn ldumaxal_w6_w7_x8() {
        assert_eq!(
            Inst::Ldumaxal32 {
                rs: W6,
                rt: W7,
                rn: X8
            }
            .encode(),
            0xB8E66107
        );
    }
    #[test]
    fn ldumaxal_x9_x10_x11() {
        assert_eq!(
            Inst::Ldumaxal64 {
                rs: X9,
                rt: X10,
                rn: X11
            }
            .encode(),
            0xF8E9616A
        );
    }
    #[test]
    fn stlr_w10_x11() {
        assert_eq!(Inst::Stlr32 { rt: W10, rn: X11 }.encode(), 0x889FFD6A);
    }
    #[test]
    fn stlr_x10_x11() {
        assert_eq!(Inst::Stlr64 { rt: X10, rn: X11 }.encode(), 0xC89FFD6A);
    }
    #[test]
    fn ldaddal_w0_w8_x8() {
        assert_eq!(
            Inst::Ldaddal32 {
                rs: W0,
                rt: W8,
                rn: X8
            }
            .encode(),
            0xB8E00108
        );
    }
    #[test]
    fn ldaddal_x0_x8_x8() {
        assert_eq!(
            Inst::Ldaddal64 {
                rs: X0,
                rt: X8,
                rn: X8
            }
            .encode(),
            0xF8E00108
        );
    }
    #[test]
    fn ldsmaxal_w0_w1_x2() {
        assert_eq!(
            Inst::Ldsmaxal32 {
                rs: W0,
                rt: W1,
                rn: X2
            }
            .encode(),
            0xB8E04041
        );
    }
    #[test]
    fn ldsminal_x9_x10_x11() {
        assert_eq!(
            Inst::Ldsminal64 {
                rs: X9,
                rt: X10,
                rn: X11
            }
            .encode(),
            0xF8E9516A
        );
    }
    #[test]
    fn ldclral_w12_w13_x14() {
        assert_eq!(
            Inst::Ldclral32 {
                rs: W12,
                rt: W13,
                rn: X14
            }
            .encode(),
            0xB8EC11CD
        );
    }
    #[test]
    fn ldeoral_x9_x10_x11() {
        assert_eq!(
            Inst::Ldeoral64 {
                rs: X9,
                rt: X10,
                rn: X11
            }
            .encode(),
            0xF8E9216A
        );
    }
    #[test]
    fn ldsetal_w0_w1_x2() {
        assert_eq!(
            Inst::Ldsetal32 {
                rs: W0,
                rt: W1,
                rn: X2
            }
            .encode(),
            0xB8E03041
        );
    }
    #[test]
    fn lduminal_w18_w19_x20() {
        assert_eq!(
            Inst::Lduminal32 {
                rs: W18,
                rt: W19,
                rn: X20
            }
            .encode(),
            0xB8F27293
        );
    }
    #[test]
    fn lduminal_x21_x22_x23() {
        assert_eq!(
            Inst::Lduminal64 {
                rs: X21,
                rt: X22,
                rn: X23
            }
            .encode(),
            0xF8F572F6
        );
    }
    #[test]
    fn swpal_w0_w0_x8() {
        assert_eq!(
            Inst::Swpal32 {
                rs: W0,
                rt: W0,
                rn: X8
            }
            .encode(),
            0xB8E08100
        );
    }
    #[test]
    fn swpal_x1_x2_x3() {
        assert_eq!(
            Inst::Swpal64 {
                rs: X1,
                rt: X2,
                rn: X3
            }
            .encode(),
            0xF8E18062
        );
    }
    #[test]
    fn swpalb_w8_w9_x10() {
        assert_eq!(
            Inst::Swpalb {
                rs: W8,
                rt: W9,
                rn: X10
            }
            .encode(),
            0x38E88149
        );
    }
    #[test]
    fn swpalh_w11_w12_x13() {
        assert_eq!(
            Inst::Swpalh {
                rs: W11,
                rt: W12,
                rn: X13
            }
            .encode(),
            0x78EB81AC
        );
    }
    #[test]
    fn casalb_w6_w7_x8() {
        assert_eq!(
            Inst::Casalb {
                rs: W6,
                rt: W7,
                rn: X8
            }
            .encode(),
            0x08E6FD07
        );
    }
    #[test]
    fn casalh_w9_w10_x11() {
        assert_eq!(
            Inst::Casalh {
                rs: W9,
                rt: W10,
                rn: X11
            }
            .encode(),
            0x48E9FD6A
        );
    }
    #[test]
    fn casal_w4_w5_x6() {
        assert_eq!(
            Inst::Casal32 {
                rs: W4,
                rt: W5,
                rn: X6
            }
            .encode(),
            0x88E4FCC5
        );
    }
    #[test]
    fn casal_x7_x8_x9() {
        assert_eq!(
            Inst::Casal64 {
                rs: X7,
                rt: X8,
                rn: X9
            }
            .encode(),
            0xC8E7FD28
        );
    }
    #[test]
    fn ldr_d0_x1() {
        assert_eq!(
            Inst::LdrFpImm64 {
                rt: D0,
                rn: X1,
                offset: 0
            }
            .encode(),
            0xFD400020
        );
    }
    #[test]
    fn ldr_q0_sp_16() {
        assert_eq!(
            Inst::LdrFpImm128 {
                rt: FpReg::new(0),
                rn: SP,
                offset: 16
            }
            .encode(),
            0x3DC007E0
        );
    }
    #[test]
    fn ldr_h2_sp_14() {
        assert_eq!(
            Inst::LdrFpImm16 {
                rt: FpReg::new(2),
                rn: SP,
                offset: 14
            }
            .encode(),
            0x7D401FE2
        );
    }
    #[test]
    fn ldr_b2_sp_15() {
        assert_eq!(
            Inst::LdrFpImm8 {
                rt: FpReg::new(2),
                rn: SP,
                offset: 15
            }
            .encode(),
            0x3D403FE2
        );
    }
    #[test]
    fn str_q1_sp() {
        assert_eq!(
            Inst::StrFpImm128 {
                rt: FpReg::new(1),
                rn: SP,
                offset: 0
            }
            .encode(),
            0x3D8003E1
        );
    }
    #[test]
    fn str_h2_sp_14() {
        assert_eq!(
            Inst::StrFpImm16 {
                rt: FpReg::new(2),
                rn: SP,
                offset: 14
            }
            .encode(),
            0x7D001FE2
        );
    }
    #[test]
    fn str_b2_sp_15() {
        assert_eq!(
            Inst::StrFpImm8 {
                rt: FpReg::new(2),
                rn: SP,
                offset: 15
            }
            .encode(),
            0x3D003FE2
        );
    }
    #[test]
    fn ldr_q0_lit_16() {
        assert_eq!(
            Inst::LdrFpLit128 {
                rt: FpReg::new(0),
                offset: 16
            }
            .encode(),
            0x9C000080
        );
    }
    #[test]
    fn ldr_q0_x1_x2() {
        assert_eq!(
            Inst::LdrFpReg128 {
                rt: FpReg::new(0),
                rn: X1,
                rm: X2,
                extend: AddrExtend::Lsl,
                shift: false
            }
            .encode(),
            0x3CE26820
        );
    }
    #[test]
    fn str_q1_x3_w4_uxtw4() {
        assert_eq!(
            Inst::StrFpReg128 {
                rt: FpReg::new(1),
                rn: X3,
                rm: W4,
                extend: AddrExtend::Uxtw,
                shift: true
            }
            .encode(),
            0x3CA45861
        );
    }
    #[test]
    fn ldr_q0_sp_post_16() {
        assert_eq!(
            Inst::LdrFpPost128 {
                rt: FpReg::new(0),
                rn: SP,
                offset: 16
            }
            .encode(),
            0x3CC107E0
        );
    }
    #[test]
    fn str_q1_sp_pre_m16() {
        assert_eq!(
            Inst::StrFpPre128 {
                rt: FpReg::new(1),
                rn: SP,
                offset: -16
            }
            .encode(),
            0x3C9F0FE1
        );
    }
    #[test]
    fn str_d2_x3_16() {
        assert_eq!(
            Inst::StrFpImm64 {
                rt: D2,
                rn: X3,
                offset: 16
            }
            .encode(),
            0xFD000862
        );
    }
    #[test]
    fn ldr_s4_x5_x6() {
        assert_eq!(
            Inst::LdrFpReg32 {
                rt: S4,
                rn: X5,
                rm: X6,
                extend: AddrExtend::Lsl,
                shift: false
            }
            .encode(),
            0xBC6668A4
        );
    }
    #[test]
    fn str_s7_x8_w9_uxtw2() {
        assert_eq!(
            Inst::StrFpReg32 {
                rt: S7,
                rn: X8,
                rm: W9,
                extend: AddrExtend::Uxtw,
                shift: true
            }
            .encode(),
            0xBC295907
        );
    }
    #[test]
    fn ldr_d10_lit_8() {
        assert_eq!(Inst::LdrFpLit64 { rt: D10, offset: 8 }.encode(), 0x5C00004A);
    }
    #[test]
    fn ldr_d0_sp_post_8() {
        assert_eq!(
            Inst::LdrFpPost64 {
                rt: D0,
                rn: SP,
                offset: 8
            }
            .encode(),
            0xFC4087E0
        );
    }
    #[test]
    fn str_d1_sp_pre_m16() {
        assert_eq!(
            Inst::StrFpPre64 {
                rt: D1,
                rn: SP,
                offset: -16
            }
            .encode(),
            0xFC1F0FE1
        );
    }
    #[test]
    fn ldr_s2_sp_post_4() {
        assert_eq!(
            Inst::LdrFpPost32 {
                rt: S2,
                rn: SP,
                offset: 4
            }
            .encode(),
            0xBC4047E2
        );
    }
    #[test]
    fn str_s3_sp_pre_m8() {
        assert_eq!(
            Inst::StrFpPre32 {
                rt: S3,
                rn: SP,
                offset: -8
            }
            .encode(),
            0xBC1F8FE3
        );
    }
    #[test]
    fn ldr_lit64_x0_plus8() {
        assert_eq!(Inst::LdrLit64 { rt: X0, offset: 8 }.encode(), 0x58000040);
    }
    #[test]
    fn ldr_lit32_w0_plus12() {
        assert_eq!(Inst::LdrLit32 { rt: W0, offset: 12 }.encode(), 0x18000060);
    }
    #[test]
    fn ldrsw_lit_x1_plus8() {
        assert_eq!(Inst::LdrswLit { rt: X1, offset: 8 }.encode(), 0x98000041);
    }

    // ---- Load/Store pair ----

    #[test]
    fn stp_x29_x30_sp_pre_m16() {
        assert_eq!(
            Inst::StpPre64 {
                rt1: X29,
                rt2: X30,
                rn: SP,
                offset: -16
            }
            .encode(),
            0xA9BF7BFD
        );
    }
    #[test]
    fn stp_x29_x30_sp_post_16() {
        assert_eq!(
            Inst::StpPost64 {
                rt1: X29,
                rt2: X30,
                rn: SP,
                offset: 16
            }
            .encode(),
            0xA8817BFD
        );
    }
    #[test]
    fn ldp_x29_x30_sp_pre_m16() {
        assert_eq!(
            Inst::LdpPre64 {
                rt1: X29,
                rt2: X30,
                rn: SP,
                offset: -16
            }
            .encode(),
            0xA9FF7BFD
        );
    }
    #[test]
    fn ldp_x29_x30_sp_post_16() {
        assert_eq!(
            Inst::LdpPost64 {
                rt1: X29,
                rt2: X30,
                rn: SP,
                offset: 16
            }
            .encode(),
            0xA8C17BFD
        );
    }
    #[test]
    fn stp_x19_x20_sp_post_32() {
        assert_eq!(
            Inst::StpPost64 {
                rt1: X19,
                rt2: X20,
                rn: SP,
                offset: 32
            }
            .encode(),
            0xA88253F3
        );
    }
    #[test]
    fn ldp_x19_x20_sp_pre_m32() {
        assert_eq!(
            Inst::LdpPre64 {
                rt1: X19,
                rt2: X20,
                rn: SP,
                offset: -32
            }
            .encode(),
            0xA9FE53F3
        );
    }
    #[test]
    fn stp_x19_x20_sp_16() {
        assert_eq!(
            Inst::StpOff64 {
                rt1: X19,
                rt2: X20,
                rn: SP,
                offset: 16
            }
            .encode(),
            0xA90153F3
        );
    }
    #[test]
    fn ldp_x19_x20_sp_16() {
        assert_eq!(
            Inst::LdpOff64 {
                rt1: X19,
                rt2: X20,
                rn: SP,
                offset: 16
            }
            .encode(),
            0xA94153F3
        );
    }
    #[test]
    fn ldp_w9_w8_x8() {
        assert_eq!(
            Inst::LdpOff32 {
                rt1: W9,
                rt2: W8,
                rn: X8,
                offset: 0
            }
            .encode(),
            0x29402109
        );
    }
    #[test]
    fn stp_w1_w2_sp_16() {
        assert_eq!(
            Inst::StpOff32 {
                rt1: W1,
                rt2: W2,
                rn: SP,
                offset: 16
            }
            .encode(),
            0x29020BE1
        );
    }
    #[test]
    fn ldp_w9_w8_sp_post_8() {
        assert_eq!(
            Inst::LdpPost32 {
                rt1: W9,
                rt2: W8,
                rn: SP,
                offset: 8
            }
            .encode(),
            0x28C123E9
        );
    }
    #[test]
    fn ldp_w9_w8_sp_pre_m8() {
        assert_eq!(
            Inst::LdpPre32 {
                rt1: W9,
                rt2: W8,
                rn: SP,
                offset: -8
            }
            .encode(),
            0x29FF23E9
        );
    }
    #[test]
    fn ldp_d8_d9_sp_pre_m16() {
        assert_eq!(
            Inst::LdpFpPre64 {
                rt1: D8,
                rt2: D9,
                rn: SP,
                offset: -16
            }
            .encode(),
            0x6DFF27E8
        );
    }
    #[test]
    fn stp_d10_d11_sp_post_16() {
        assert_eq!(
            Inst::StpFpPost64 {
                rt1: D10,
                rt2: D11,
                rn: SP,
                offset: 16
            }
            .encode(),
            0x6C812FEA
        );
    }
    #[test]
    fn ldp_d12_d13_sp_32() {
        assert_eq!(
            Inst::LdpFpOff64 {
                rt1: D12,
                rt2: D13,
                rn: SP,
                offset: 32
            }
            .encode(),
            0x6D4237EC
        );
    }
    #[test]
    fn stp_s0_s1_sp_post_8() {
        assert_eq!(
            Inst::StpFpPost32 {
                rt1: S0,
                rt2: S1,
                rn: SP,
                offset: 8
            }
            .encode(),
            0x2C8107E0
        );
    }
    #[test]
    fn ldp_s2_s3_sp_pre_m8() {
        assert_eq!(
            Inst::LdpFpPre32 {
                rt1: S2,
                rt2: S3,
                rn: SP,
                offset: -8
            }
            .encode(),
            0x2DFF0FE2
        );
    }
    #[test]
    fn ldp_s4_s5_sp_16() {
        assert_eq!(
            Inst::LdpFpOff32 {
                rt1: S4,
                rt2: S5,
                rn: SP,
                offset: 16
            }
            .encode(),
            0x2D4217E4
        );
    }
    #[test]
    fn stp_q0_q1_sp_pre_m32() {
        assert_eq!(
            Inst::StpFpPre128 {
                rt1: FpReg::new(0),
                rt2: FpReg::new(1),
                rn: SP,
                offset: -32
            }
            .encode(),
            0xADBF07E0
        );
    }
    #[test]
    fn ldp_q2_q3_sp_post_32() {
        assert_eq!(
            Inst::LdpFpPost128 {
                rt1: FpReg::new(2),
                rt2: FpReg::new(3),
                rn: SP,
                offset: 32
            }
            .encode(),
            0xACC10FE2
        );
    }
    #[test]
    fn ldp_q4_q5_sp_64() {
        assert_eq!(
            Inst::LdpFpOff128 {
                rt1: FpReg::new(4),
                rt2: FpReg::new(5),
                rn: SP,
                offset: 64
            }
            .encode(),
            0xAD4217E4
        );
    }

    // ---- Load/Store (pre/post-index) ----

    #[test]
    fn ldr_w0_x1_post_4() {
        assert_eq!(
            Inst::LdrPost32 {
                rt: W0,
                rn: X1,
                offset: 4
            }
            .encode(),
            0xB8404420
        );
    }
    #[test]
    fn str_w2_x3_pre_m4() {
        assert_eq!(
            Inst::StrPre32 {
                rt: W2,
                rn: X3,
                offset: -4
            }
            .encode(),
            0xB81FCC62
        );
    }
    #[test]
    fn ldr_x0_x1_pre_8() {
        assert_eq!(
            Inst::LdrPre64 {
                rt: X0,
                rn: X1,
                offset: 8
            }
            .encode(),
            0xF8408C20
        );
    }
    #[test]
    fn str_x0_x1_post_8() {
        assert_eq!(
            Inst::StrPost64 {
                rt: X0,
                rn: X1,
                offset: 8
            }
            .encode(),
            0xF8008420
        );
    }

    // ---- FP arithmetic ----

    #[test]
    fn fadd_d0_d1_d2() {
        assert_eq!(
            Inst::FaddD {
                rd: D0,
                rn: D1,
                rm: D2
            }
            .encode(),
            0x1E622820
        );
    }
    #[test]
    fn fsub_d3_d4_d5() {
        assert_eq!(
            Inst::FsubD {
                rd: D3,
                rn: D4,
                rm: D5
            }
            .encode(),
            0x1E653883
        );
    }
    #[test]
    fn fmul_d6_d7_d8() {
        assert_eq!(
            Inst::FmulD {
                rd: D6,
                rn: D7,
                rm: D8
            }
            .encode(),
            0x1E6808E6
        );
    }
    #[test]
    fn fdiv_d9_d10_d11() {
        assert_eq!(
            Inst::FdivD {
                rd: D9,
                rn: D10,
                rm: D11
            }
            .encode(),
            0x1E6B1949
        );
    }
    #[test]
    fn fadd_s0_s1_s2() {
        assert_eq!(
            Inst::FaddS {
                rd: S0,
                rn: S1,
                rm: S2
            }
            .encode(),
            0x1E222820
        );
    }
    #[test]
    fn fadd_4s_v0_v0_v1() {
        assert_eq!(
            Inst::FaddV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x4E21D400
        );
    }
    #[test]
    fn add_4s_v0_v1_v2() {
        assert_eq!(
            Inst::AddV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x4EA28420
        );
    }
    #[test]
    fn addp_2d_v0_v1_v2() {
        assert_eq!(
            Inst::AddpV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x4EE2BC20
        );
    }
    #[test]
    fn addp_4s_v0_v1_v2() {
        assert_eq!(
            Inst::AddpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x4EA2BC20
        );
    }
    #[test]
    fn addp_8h_v0_v1_v2() {
        assert_eq!(
            Inst::AddpV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x4E62BC20
        );
    }
    #[test]
    fn addp_16b_v6_v7_v8() {
        assert_eq!(
            Inst::AddpV16B {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
            .encode(),
            0x4E28BCE6
        );
    }
    #[test]
    fn fmax_4s_v0_v0_v1() {
        assert_eq!(
            Inst::FmaxV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x4E21F400
        );
    }
    #[test]
    fn fmin_4s_v2_v3_v4() {
        assert_eq!(
            Inst::FminV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
            .encode(),
            0x4EA4F462
        );
    }
    #[test]
    fn fmaxnm_4s_v0_v1_v2() {
        assert_eq!(
            Inst::FmaxnmV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x4E22C420
        );
    }
    #[test]
    fn fminnm_4s_v3_v4_v5() {
        assert_eq!(
            Inst::FminnmV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x4EA5C483
        );
    }
    #[test]
    fn smax_4s_v5_v6_v7() {
        assert_eq!(
            Inst::SmaxV4S {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                rm: FpReg::new(7)
            }
            .encode(),
            0x4EA764C5
        );
    }
    #[test]
    fn smaxp_4s_v6_v7_v8() {
        assert_eq!(
            Inst::SmaxpV4S {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
            .encode(),
            0x4EA8A4E6
        );
    }
    #[test]
    fn smaxp_8h_v6_v7_v8() {
        assert_eq!(
            Inst::SmaxpV8H {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
            .encode(),
            0x4E68A4E6
        );
    }
    #[test]
    fn smaxp_16b_v6_v7_v8() {
        assert_eq!(
            Inst::SmaxpV16B {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
            .encode(),
            0x4E28A4E6
        );
    }
    #[test]
    fn smin_4s_v8_v9_v10() {
        assert_eq!(
            Inst::SminV4S {
                rd: FpReg::new(8),
                rn: FpReg::new(9),
                rm: FpReg::new(10)
            }
            .encode(),
            0x4EAA6D28
        );
    }
    #[test]
    fn sminp_4s_v9_v10_v11() {
        assert_eq!(
            Inst::SminpV4S {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
            .encode(),
            0x4EABAD49
        );
    }
    #[test]
    fn sminp_8h_v9_v10_v11() {
        assert_eq!(
            Inst::SminpV8H {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
            .encode(),
            0x4E6BAD49
        );
    }
    #[test]
    fn sminp_16b_v9_v10_v11() {
        assert_eq!(
            Inst::SminpV16B {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
            .encode(),
            0x4E2BAD49
        );
    }
    #[test]
    fn umax_4s_v0_v0_v1() {
        assert_eq!(
            Inst::UmaxV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x6EA16400
        );
    }
    #[test]
    fn umaxp_4s_v0_v1_v2() {
        assert_eq!(
            Inst::UmaxpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6EA2A420
        );
    }
    #[test]
    fn umaxp_8h_v0_v1_v2() {
        assert_eq!(
            Inst::UmaxpV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6E62A420
        );
    }
    #[test]
    fn umaxp_16b_v0_v1_v2() {
        assert_eq!(
            Inst::UmaxpV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6E22A420
        );
    }
    #[test]
    fn umin_4s_v2_v3_v4() {
        assert_eq!(
            Inst::UminV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
            .encode(),
            0x6EA46C62
        );
    }
    #[test]
    fn uminp_4s_v3_v4_v5() {
        assert_eq!(
            Inst::UminpV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6EA5AC83
        );
    }
    #[test]
    fn uminp_8h_v3_v4_v5() {
        assert_eq!(
            Inst::UminpV8H {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6E65AC83
        );
    }
    #[test]
    fn uminp_16b_v3_v4_v5() {
        assert_eq!(
            Inst::UminpV16B {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6E25AC83
        );
    }
    #[test]
    fn addv_4s_s0_v0() {
        assert_eq!(
            Inst::AddvV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4EB1B800
        );
    }
    #[test]
    fn addv_16b_b0_v0() {
        assert_eq!(
            Inst::AddvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4E31B800
        );
    }
    #[test]
    fn addv_8h_h0_v0() {
        assert_eq!(
            Inst::AddvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4E71B800
        );
    }
    #[test]
    fn faddp_4s_v0_v1_v2() {
        assert_eq!(
            Inst::FaddpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6E22D420
        );
    }
    #[test]
    fn faddp_2d_v0_v1_v2() {
        assert_eq!(
            Inst::FaddpV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6E62D420
        );
    }
    #[test]
    fn fmaxp_2d_v3_v4_v5() {
        assert_eq!(
            Inst::FmaxpV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6E65F483
        );
    }
    #[test]
    fn fmaxp_4s_v0_v1_v2() {
        assert_eq!(
            Inst::FmaxpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6E22F420
        );
    }
    #[test]
    fn fminp_2d_v6_v7_v8() {
        assert_eq!(
            Inst::FminpV2D {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
            .encode(),
            0x6EE8F4E6
        );
    }
    #[test]
    fn fminp_4s_v3_v4_v5() {
        assert_eq!(
            Inst::FminpV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6EA5F483
        );
    }
    #[test]
    fn fmaxnmp_4s_v0_v1_v2() {
        assert_eq!(
            Inst::FmaxnmpV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6E22C420
        );
    }
    #[test]
    fn fmaxnmp_2d_v0_v1_v2() {
        assert_eq!(
            Inst::FmaxnmpV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6E62C420
        );
    }
    #[test]
    fn fminnmp_4s_v3_v4_v5() {
        assert_eq!(
            Inst::FminnmpV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6EA5C483
        );
    }
    #[test]
    fn fminnmp_2d_v3_v4_v5() {
        assert_eq!(
            Inst::FminnmpV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6EE5C483
        );
    }
    #[test]
    fn fmla_4s_v0_v1_v2() {
        assert_eq!(
            Inst::FmlaV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x4E22CC20
        );
    }
    #[test]
    fn fmls_4s_v3_v4_v5() {
        assert_eq!(
            Inst::FmlsV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x4EA5CC83
        );
    }
    #[test]
    fn faddp_2s_s3_v4() {
        assert_eq!(
            Inst::FaddpV2S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
            .encode(),
            0x7E30D883
        );
    }
    #[test]
    fn faddp_2d_d0_v0() {
        assert_eq!(
            Inst::FaddpV2DScalar {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x7E70D800
        );
    }
    #[test]
    fn fmaxp_2d_d1_v2() {
        assert_eq!(
            Inst::FmaxpV2DScalar {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x7E70F841
        );
    }
    #[test]
    fn fminp_2d_d3_v4() {
        assert_eq!(
            Inst::FminpV2DScalar {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
            .encode(),
            0x7EF0F883
        );
    }
    #[test]
    fn fmaxnmp_2d_d0_v0() {
        assert_eq!(
            Inst::FmaxnmpV2DScalar {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x7E70C800
        );
    }
    #[test]
    fn fminnmp_2d_d1_v2() {
        assert_eq!(
            Inst::FminnmpV2DScalar {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x7EF0C841
        );
    }
    #[test]
    fn umaxv_4s_s1_v2() {
        assert_eq!(
            Inst::UmaxvV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x6EB0A841
        );
    }
    #[test]
    fn umaxv_16b_b0_v0() {
        assert_eq!(
            Inst::UmaxvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x6E30A800
        );
    }
    #[test]
    fn umaxv_8h_h0_v0() {
        assert_eq!(
            Inst::UmaxvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x6E70A800
        );
    }
    #[test]
    fn smaxv_4s_s3_v4() {
        assert_eq!(
            Inst::SmaxvV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
            .encode(),
            0x4EB0A883
        );
    }
    #[test]
    fn smaxv_16b_b0_v0() {
        assert_eq!(
            Inst::SmaxvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4E30A800
        );
    }
    #[test]
    fn smaxv_8h_h0_v0() {
        assert_eq!(
            Inst::SmaxvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4E70A800
        );
    }
    #[test]
    fn uminv_4s_s1_v2() {
        assert_eq!(
            Inst::UminvV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x6EB1A841
        );
    }
    #[test]
    fn uminv_16b_b0_v0() {
        assert_eq!(
            Inst::UminvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x6E31A800
        );
    }
    #[test]
    fn uminv_8h_h0_v0() {
        assert_eq!(
            Inst::UminvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x6E71A800
        );
    }
    #[test]
    fn sminv_4s_s3_v4() {
        assert_eq!(
            Inst::SminvV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
            .encode(),
            0x4EB1A883
        );
    }
    #[test]
    fn sminv_16b_b0_v0() {
        assert_eq!(
            Inst::SminvV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4E31A800
        );
    }
    #[test]
    fn sminv_8h_h0_v0() {
        assert_eq!(
            Inst::SminvV8H {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4E71A800
        );
    }
    #[test]
    fn fmaxv_4s_s1_v2() {
        assert_eq!(
            Inst::FmaxvV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x6E30F841
        );
    }
    #[test]
    fn fminv_4s_s3_v4() {
        assert_eq!(
            Inst::FminvV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
            .encode(),
            0x6EB0F883
        );
    }
    #[test]
    fn fmaxnmv_4s_s1_v2() {
        assert_eq!(
            Inst::FmaxnmvV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x6E30C841
        );
    }
    #[test]
    fn fminnmv_4s_s3_v4() {
        assert_eq!(
            Inst::FminnmvV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
            .encode(),
            0x6EB0C883
        );
    }
    #[test]
    fn fsub_4s_v0_v0_v1() {
        assert_eq!(
            Inst::FsubV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x4EA1D400
        );
    }
    #[test]
    fn sub_4s_v3_v4_v5() {
        assert_eq!(
            Inst::SubV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6EA58483
        );
    }
    #[test]
    fn fmul_4s_v0_v0_v1() {
        assert_eq!(
            Inst::FmulV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x6E21DC00
        );
    }
    #[test]
    fn fdiv_4s_v0_v0_v1() {
        assert_eq!(
            Inst::FdivV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x6E21FC00
        );
    }
    #[test]
    fn fabs_4s_v0_v0() {
        assert_eq!(
            Inst::FabsV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4EA0F800
        );
    }
    #[test]
    fn fabs_2d_v0_v0() {
        assert_eq!(
            Inst::FabsV2D {
                rd: FpReg::new(0),
                rn: FpReg::new(0)
            }
            .encode(),
            0x4EE0F800
        );
    }
    #[test]
    fn fneg_4s_v6_v7() {
        assert_eq!(
            Inst::FnegV4S {
                rd: FpReg::new(6),
                rn: FpReg::new(7)
            }
            .encode(),
            0x6EA0F8E6
        );
    }
    #[test]
    fn fneg_2d_v3_v4() {
        assert_eq!(
            Inst::FnegV2D {
                rd: FpReg::new(3),
                rn: FpReg::new(4)
            }
            .encode(),
            0x6EE0F883
        );
    }
    #[test]
    fn fsqrt_4s_v1_v2() {
        assert_eq!(
            Inst::FsqrtV4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x6EA1F841
        );
    }
    #[test]
    fn fsqrt_2d_v1_v2() {
        assert_eq!(
            Inst::FsqrtV2D {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x6EE1F841
        );
    }
    #[test]
    fn scvtf_4s_v0_v1() {
        assert_eq!(
            Inst::ScvtfV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1)
            }
            .encode(),
            0x4E21D820
        );
    }
    #[test]
    fn ucvtf_4s_v2_v3() {
        assert_eq!(
            Inst::UcvtfV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3)
            }
            .encode(),
            0x6E21D862
        );
    }
    #[test]
    fn fcvtzs_4s_v4_v5() {
        assert_eq!(
            Inst::FcvtzsV4S {
                rd: FpReg::new(4),
                rn: FpReg::new(5)
            }
            .encode(),
            0x4EA1B8A4
        );
    }
    #[test]
    fn fcvtzu_4s_v6_v7() {
        assert_eq!(
            Inst::FcvtzuV4S {
                rd: FpReg::new(6),
                rn: FpReg::new(7)
            }
            .encode(),
            0x6EA1B8E6
        );
    }
    #[test]
    fn frecpe_4s_v0_v1() {
        assert_eq!(
            Inst::FrecpeV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1)
            }
            .encode(),
            0x4EA1D820
        );
    }
    #[test]
    fn frecps_4s_v2_v3_v4() {
        assert_eq!(
            Inst::FrecpsV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
            .encode(),
            0x4E24FC62
        );
    }
    #[test]
    fn frsqrte_4s_v5_v6() {
        assert_eq!(
            Inst::FrsqrteV4S {
                rd: FpReg::new(5),
                rn: FpReg::new(6)
            }
            .encode(),
            0x6EA1D8C5
        );
    }
    #[test]
    fn frsqrts_4s_v7_v8_v9() {
        assert_eq!(
            Inst::FrsqrtsV4S {
                rd: FpReg::new(7),
                rn: FpReg::new(8),
                rm: FpReg::new(9)
            }
            .encode(),
            0x4EA9FD07
        );
    }
    #[test]
    fn frintn_4s_v0_v1() {
        assert_eq!(
            Inst::FrintnV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1)
            }
            .encode(),
            0x4E218820
        );
    }
    #[test]
    fn frintm_4s_v2_v3() {
        assert_eq!(
            Inst::FrintmV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3)
            }
            .encode(),
            0x4E219862
        );
    }
    #[test]
    fn frintp_4s_v4_v5() {
        assert_eq!(
            Inst::FrintpV4S {
                rd: FpReg::new(4),
                rn: FpReg::new(5)
            }
            .encode(),
            0x4EA188A4
        );
    }
    #[test]
    fn frintz_4s_v6_v7() {
        assert_eq!(
            Inst::FrintzV4S {
                rd: FpReg::new(6),
                rn: FpReg::new(7)
            }
            .encode(),
            0x4EA198E6
        );
    }
    #[test]
    fn frinta_4s_v0_v1() {
        assert_eq!(
            Inst::FrintaV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1)
            }
            .encode(),
            0x6E218820
        );
    }
    #[test]
    fn frinti_4s_v2_v3() {
        assert_eq!(
            Inst::FrintiV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3)
            }
            .encode(),
            0x6EA19862
        );
    }
    #[test]
    fn mov_16b_v0_v2() {
        assert_eq!(
            Inst::MovV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(2)
            }
            .encode(),
            0x4EA21C40
        );
    }
    #[test]
    fn mov_8b_v1_v3() {
        assert_eq!(
            Inst::MovV8B {
                rd: FpReg::new(1),
                rn: FpReg::new(3)
            }
            .encode(),
            0x0EA31C61
        );
    }
    #[test]
    fn mov_4s_v4_v5() {
        assert_eq!(
            Inst::MovV4S {
                rd: FpReg::new(4),
                rn: FpReg::new(5)
            }
            .encode(),
            0x4EA51CA4
        );
    }
    #[test]
    fn mov_2d_v6_v7() {
        assert_eq!(
            Inst::MovV2D {
                rd: FpReg::new(6),
                rn: FpReg::new(7)
            }
            .encode(),
            0x4EA71CE6
        );
    }
    #[test]
    fn and_16b_v6_v7_v8() {
        assert_eq!(
            Inst::AndV16B {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
            .encode(),
            0x4E281CE6
        );
    }
    #[test]
    fn bic_16b_v5_v6_v7() {
        assert_eq!(
            Inst::BicV16B {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                rm: FpReg::new(7)
            }
            .encode(),
            0x4E671CC5
        );
    }
    #[test]
    fn bif_16b_v0_v1_v2() {
        assert_eq!(
            Inst::BifV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x6EE21C20
        );
    }
    #[test]
    fn bit_16b_v3_v4_v5() {
        assert_eq!(
            Inst::BitV16B {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6EA51C83
        );
    }
    #[test]
    fn bsl_16b_v6_v7_v8() {
        assert_eq!(
            Inst::BslV16B {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
            .encode(),
            0x6E681CE6
        );
    }
    #[test]
    fn cmeq_4s_v0_v0_v1() {
        assert_eq!(
            Inst::CmeqV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x6EA18C00
        );
    }
    #[test]
    fn fcmeq_4s_v0_v1_v2() {
        assert_eq!(
            Inst::FcmeqV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                rm: FpReg::new(2)
            }
            .encode(),
            0x4E22E420
        );
    }
    #[test]
    fn cmhs_4s_v0_v0_v1() {
        assert_eq!(
            Inst::CmhsV4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x6EA13C00
        );
    }
    #[test]
    fn cmhi_4s_v2_v3_v4() {
        assert_eq!(
            Inst::CmhiV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
            .encode(),
            0x6EA43462
        );
    }
    #[test]
    fn cmge_4s_v5_v6_v7() {
        assert_eq!(
            Inst::CmgeV4S {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                rm: FpReg::new(7)
            }
            .encode(),
            0x4EA73CC5
        );
    }
    #[test]
    fn fcmge_4s_v3_v4_v5() {
        assert_eq!(
            Inst::FcmgeV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x6E25E483
        );
    }
    #[test]
    fn cmgt_4s_v2_v3_v4() {
        assert_eq!(
            Inst::CmgtV4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
            .encode(),
            0x4EA43462
        );
    }
    #[test]
    fn fcmgt_4s_v6_v7_v8() {
        assert_eq!(
            Inst::FcmgtV4S {
                rd: FpReg::new(6),
                rn: FpReg::new(7),
                rm: FpReg::new(8)
            }
            .encode(),
            0x6EA8E4E6
        );
    }
    #[test]
    fn orr_16b_v9_v10_v11() {
        assert_eq!(
            Inst::OrrV16B {
                rd: FpReg::new(9),
                rn: FpReg::new(10),
                rm: FpReg::new(11)
            }
            .encode(),
            0x4EAB1D49
        );
    }
    #[test]
    fn eor_16b_v12_v13_v14() {
        assert_eq!(
            Inst::EorV16B {
                rd: FpReg::new(12),
                rn: FpReg::new(13),
                rm: FpReg::new(14)
            }
            .encode(),
            0x6E2E1DAC
        );
    }
    #[test]
    fn ext_16b_v0_v0_v0_8() {
        assert_eq!(
            Inst::ExtV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(0),
                index: 8
            }
            .encode(),
            0x6E004000
        );
    }
    #[test]
    fn rev64_4s_v1_v2() {
        assert_eq!(
            Inst::Rev64V4S {
                rd: FpReg::new(1),
                rn: FpReg::new(2)
            }
            .encode(),
            0x4EA00841
        );
    }
    #[test]
    fn zip1_4s_v0_v0_v1() {
        assert_eq!(
            Inst::Zip1V4S {
                rd: FpReg::new(0),
                rn: FpReg::new(0),
                rm: FpReg::new(1)
            }
            .encode(),
            0x4E813800
        );
    }
    #[test]
    fn zip2_4s_v2_v3_v4() {
        assert_eq!(
            Inst::Zip2V4S {
                rd: FpReg::new(2),
                rn: FpReg::new(3),
                rm: FpReg::new(4)
            }
            .encode(),
            0x4E847862
        );
    }
    #[test]
    fn uzp1_4s_v5_v6_v7() {
        assert_eq!(
            Inst::Uzp1V4S {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                rm: FpReg::new(7)
            }
            .encode(),
            0x4E8718C5
        );
    }
    #[test]
    fn uzp2_4s_v8_v9_v10() {
        assert_eq!(
            Inst::Uzp2V4S {
                rd: FpReg::new(8),
                rn: FpReg::new(9),
                rm: FpReg::new(10)
            }
            .encode(),
            0x4E8A5928
        );
    }
    #[test]
    fn trn1_4s_v11_v12_v13() {
        assert_eq!(
            Inst::Trn1V4S {
                rd: FpReg::new(11),
                rn: FpReg::new(12),
                rm: FpReg::new(13)
            }
            .encode(),
            0x4E8D298B
        );
    }
    #[test]
    fn trn2_4s_v3_v4_v5() {
        assert_eq!(
            Inst::Trn2V4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                rm: FpReg::new(5)
            }
            .encode(),
            0x4E856883
        );
    }
    #[test]
    fn tbl_16b_v0_v1_v2() {
        assert_eq!(
            Inst::TblV16B {
                rd: FpReg::new(0),
                table: FpReg::new(1),
                table_len: 1,
                index: FpReg::new(2)
            }
            .encode(),
            0x4E020020
        );
    }
    #[test]
    fn tbl_16b_v3_v4_v5_v6() {
        assert_eq!(
            Inst::TblV16B {
                rd: FpReg::new(3),
                table: FpReg::new(4),
                table_len: 2,
                index: FpReg::new(6)
            }
            .encode(),
            0x4E062083
        );
    }
    #[test]
    fn tbl_16b_v7_v8_v9_v10_v11() {
        assert_eq!(
            Inst::TblV16B {
                rd: FpReg::new(7),
                table: FpReg::new(8),
                table_len: 3,
                index: FpReg::new(11)
            }
            .encode(),
            0x4E0B4107
        );
    }
    #[test]
    fn tbl_16b_v12_v13_v14_v15_v16_v17() {
        assert_eq!(
            Inst::TblV16B {
                rd: FpReg::new(12),
                table: FpReg::new(13),
                table_len: 4,
                index: FpReg::new(17)
            }
            .encode(),
            0x4E1161AC
        );
    }
    #[test]
    fn tbx_16b_v18_v19_v20() {
        assert_eq!(
            Inst::TbxV16B {
                rd: FpReg::new(18),
                table: FpReg::new(19),
                table_len: 1,
                index: FpReg::new(20)
            }
            .encode(),
            0x4E141272
        );
    }
    #[test]
    fn tbx_16b_v21_v22_v23_v24() {
        assert_eq!(
            Inst::TbxV16B {
                rd: FpReg::new(21),
                table: FpReg::new(22),
                table_len: 2,
                index: FpReg::new(24)
            }
            .encode(),
            0x4E1832D5
        );
    }
    #[test]
    fn fmov_s1_s2() {
        assert_eq!(Inst::FmovRegS { rd: S1, rn: S2 }.encode(), 0x1E204041);
    }
    #[test]
    fn fmov_d1_d2() {
        assert_eq!(Inst::FmovRegD { rd: D1, rn: D2 }.encode(), 0x1E604041);
    }
    #[test]
    fn mov_s0_v1_lane2() {
        assert_eq!(
            Inst::MovFromLaneS {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                index: 2
            }
            .encode(),
            0x5E140420
        );
    }
    #[test]
    fn mov_d3_v4_lane1() {
        assert_eq!(
            Inst::MovFromLaneD {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                index: 1
            }
            .encode(),
            0x5E180483
        );
    }
    #[test]
    fn mov_s_v5_lane0_from_v6_lane0() {
        assert_eq!(
            Inst::MovLaneS {
                rd: FpReg::new(5),
                rd_index: 0,
                rn: FpReg::new(6),
                rn_index: 0
            }
            .encode(),
            0x6E0404C5
        );
    }
    #[test]
    fn mov_d_v7_lane1_from_v8_lane1() {
        assert_eq!(
            Inst::MovLaneD {
                rd: FpReg::new(7),
                rd_index: 1,
                rn: FpReg::new(8),
                rn_index: 1
            }
            .encode(),
            0x6E184507
        );
    }
    #[test]
    fn mov_h_v0_lane5_from_v1_lane0() {
        assert_eq!(
            Inst::MovLaneH {
                rd: FpReg::new(0),
                rd_index: 5,
                rn: FpReg::new(1),
                rn_index: 0
            }
            .encode(),
            0x6E160420
        );
    }
    #[test]
    fn mov_b_v0_lane7_from_v1_lane0() {
        assert_eq!(
            Inst::MovLaneB {
                rd: FpReg::new(0),
                rd_index: 7,
                rn: FpReg::new(1),
                rn_index: 0
            }
            .encode(),
            0x6E0F0420
        );
    }
    #[test]
    fn mov_s_w0_v1_lane2() {
        assert_eq!(
            Inst::MovFromLaneGpS {
                rd: W0,
                rn: FpReg::new(1),
                index: 2
            }
            .encode(),
            0x0E143C20
        );
    }
    #[test]
    fn mov_d_x0_v1_lane1() {
        assert_eq!(
            Inst::MovFromLaneGpD {
                rd: X0,
                rn: FpReg::new(1),
                index: 1
            }
            .encode(),
            0x4E183C20
        );
    }
    #[test]
    fn umov_h_w1_v2_lane5() {
        assert_eq!(
            Inst::UmovFromLaneH {
                rd: W1,
                rn: FpReg::new(2),
                index: 5
            }
            .encode(),
            0x0E163C41
        );
    }
    #[test]
    fn umov_b_w3_v4_lane7() {
        assert_eq!(
            Inst::UmovFromLaneB {
                rd: W3,
                rn: FpReg::new(4),
                index: 7
            }
            .encode(),
            0x0E0F3C83
        );
    }
    #[test]
    fn smov_h_w1_v2_lane3() {
        assert_eq!(
            Inst::SmovFromLaneH {
                rd: W1,
                rn: FpReg::new(2),
                index: 3
            }
            .encode(),
            0x0E0E2C41
        );
    }
    #[test]
    fn smov_b_w0_v0_lane0() {
        assert_eq!(
            Inst::SmovFromLaneB {
                rd: W0,
                rn: FpReg::new(0),
                index: 0
            }
            .encode(),
            0x0E012C00
        );
    }
    #[test]
    fn mov_s_v5_lane1_w6() {
        assert_eq!(
            Inst::MovLaneFromGpS {
                rd: FpReg::new(5),
                rd_index: 1,
                rn: W6
            }
            .encode(),
            0x4E0C1CC5
        );
    }
    #[test]
    fn mov_d_v0_lane1_x1() {
        assert_eq!(
            Inst::MovLaneFromGpD {
                rd: FpReg::new(0),
                rd_index: 1,
                rn: X1
            }
            .encode(),
            0x4E181C20
        );
    }
    #[test]
    fn mov_h_v7_lane5_w8() {
        assert_eq!(
            Inst::MovLaneFromGpH {
                rd: FpReg::new(7),
                rd_index: 5,
                rn: W8
            }
            .encode(),
            0x4E161D07
        );
    }
    #[test]
    fn mov_b_v9_lane7_w10() {
        assert_eq!(
            Inst::MovLaneFromGpB {
                rd: FpReg::new(9),
                rd_index: 7,
                rn: W10
            }
            .encode(),
            0x4E0F1D49
        );
    }
    #[test]
    fn dup_16b_v0_v1_lane15() {
        assert_eq!(
            Inst::DupV16B {
                rd: FpReg::new(0),
                rn: FpReg::new(1),
                index: 15
            }
            .encode(),
            0x4E1F0420
        );
    }
    #[test]
    fn dup_8h_v1_v2_lane5() {
        assert_eq!(
            Inst::DupV8H {
                rd: FpReg::new(1),
                rn: FpReg::new(2),
                index: 5
            }
            .encode(),
            0x4E160441
        );
    }
    #[test]
    fn dup_4s_v3_v4_lane2() {
        assert_eq!(
            Inst::DupV4S {
                rd: FpReg::new(3),
                rn: FpReg::new(4),
                index: 2
            }
            .encode(),
            0x4E140483
        );
    }
    #[test]
    fn dup_2d_v5_v6_lane1() {
        assert_eq!(
            Inst::DupV2D {
                rd: FpReg::new(5),
                rn: FpReg::new(6),
                index: 1
            }
            .encode(),
            0x4E1804C5
        );
    }
    #[test]
    fn fsub_s3_s4_s5() {
        assert_eq!(
            Inst::FsubS {
                rd: S3,
                rn: S4,
                rm: S5
            }
            .encode(),
            0x1E253883
        );
    }
    #[test]
    fn fneg_d0_d1() {
        assert_eq!(Inst::FnegD { rd: D0, rn: D1 }.encode(), 0x1E614020);
    }
    #[test]
    fn fabs_d0_d1() {
        assert_eq!(Inst::FabsD { rd: D0, rn: D1 }.encode(), 0x1E60C020);
    }
    #[test]
    fn fsqrt_d0_d1() {
        assert_eq!(Inst::FsqrtD { rd: D0, rn: D1 }.encode(), 0x1E61C020);
    }
    #[test]
    fn fcmp_d0_d1() {
        assert_eq!(Inst::FcmpD { rn: D0, rm: D1 }.encode(), 0x1E612000);
    }
    #[test]
    fn fmov_d2_imm_3_5() {
        assert_eq!(Inst::FmovImmD { rd: D2, imm8: 12 }.encode(), 0x1E619002);
    }
    #[test]
    fn fmov_d1_imm_10() {
        assert_eq!(Inst::FmovImmD { rd: D1, imm8: 36 }.encode(), 0x1E649001);
    }
    #[test]
    fn fmov_d1_imm_neg_1() {
        assert_eq!(Inst::FmovImmD { rd: D1, imm8: 240 }.encode(), 0x1E7E1001);
    }
    #[test]
    fn fcsel_d0_d0_d1_mi() {
        assert_eq!(
            Inst::FcselD {
                rd: D0,
                rn: D0,
                rm: D1,
                cond: Cond::MI
            }
            .encode(),
            0x1E614C00
        );
    }
    #[test]
    fn fmadd_d0_d1_d2_d3() {
        assert_eq!(
            Inst::FmaddD {
                rd: D0,
                rn: D1,
                rm: D2,
                ra: D3
            }
            .encode(),
            0x1F420C20
        );
    }

    // Single-precision FP unary/compare/fmadd
    #[test]
    fn fneg_s0_s1() {
        assert_eq!(Inst::FnegS { rd: S0, rn: S1 }.encode(), 0x1E214020);
    }
    #[test]
    fn fabs_s0_s1() {
        assert_eq!(Inst::FabsS { rd: S0, rn: S1 }.encode(), 0x1E20C020);
    }
    #[test]
    fn fsqrt_s0_s1() {
        assert_eq!(Inst::FsqrtS { rd: S0, rn: S1 }.encode(), 0x1E21C020);
    }
    #[test]
    fn fcmp_s0_s1() {
        assert_eq!(Inst::FcmpS { rn: S0, rm: S1 }.encode(), 0x1E212000);
    }
    #[test]
    fn fmov_s2_imm_3_5() {
        assert_eq!(Inst::FmovImmS { rd: S2, imm8: 12 }.encode(), 0x1E219002);
    }
    #[test]
    fn fcsel_s0_s0_s1_mi() {
        assert_eq!(
            Inst::FcselS {
                rd: S0,
                rn: S0,
                rm: S1,
                cond: Cond::MI
            }
            .encode(),
            0x1E214C00
        );
    }
    #[test]
    fn fmadd_s0_s1_s2_s3() {
        assert_eq!(
            Inst::FmaddS {
                rd: S0,
                rn: S1,
                rm: S2,
                ra: S3
            }
            .encode(),
            0x1F020C20
        );
    }

    // ---- FP / integer conversion ----

    #[test]
    fn fcvtzs_x0_d1() {
        assert_eq!(Inst::FcvtzsD { rd: X0, rn: D1 }.encode(), 0x9E780020);
    }
    #[test]
    fn scvtf_d0_x1() {
        assert_eq!(Inst::ScvtfD { rd: D0, rn: X1 }.encode(), 0x9E620020);
    }
    #[test]
    fn fmov_d0_x1() {
        assert_eq!(Inst::FmovToD { rd: D0, rn: X1 }.encode(), 0x9E670020);
    }
    #[test]
    fn fmov_s0_w0() {
        assert_eq!(Inst::FmovToS { rd: S0, rn: W0 }.encode(), 0x1E270000);
    }
    #[test]
    fn fmov_x0_d1() {
        assert_eq!(Inst::FmovFromD { rd: X0, rn: D1 }.encode(), 0x9E660020);
    }
    #[test]
    fn fmov_w0_s0() {
        assert_eq!(Inst::FmovFromS { rd: W0, rn: S0 }.encode(), 0x1E260000);
    }

    // ---- System ----

    #[test]
    fn svc_0x80() {
        assert_eq!(Inst::Svc { imm16: 0x80 }.encode(), 0xD4001001);
    }
    #[test]
    fn nop() {
        assert_eq!(Inst::Nop.encode(), 0xD503201F);
    }
    #[test]
    fn yield_() {
        assert_eq!(Inst::Yield.encode(), 0xD503203F);
    }
    #[test]
    fn wfe_() {
        assert_eq!(Inst::Wfe.encode(), 0xD503205F);
    }
    #[test]
    fn wfi_() {
        assert_eq!(Inst::Wfi.encode(), 0xD503207F);
    }
    #[test]
    fn sev_() {
        assert_eq!(Inst::Sev.encode(), 0xD503209F);
    }
    #[test]
    fn sevl_() {
        assert_eq!(Inst::Sevl.encode(), 0xD50320BF);
    }
    #[test]
    fn dmb_ish() {
        assert_eq!(
            Inst::Dmb {
                option: BarrierOpt::Ish
            }
            .encode(),
            0xD5033BBF
        );
    }
    #[test]
    fn dsb_ishst() {
        assert_eq!(
            Inst::Dsb {
                option: BarrierOpt::Ishst
            }
            .encode(),
            0xD5033A9F
        );
    }
    #[test]
    fn isb_sy() {
        assert_eq!(
            Inst::Isb {
                option: BarrierOpt::Sy
            }
            .encode(),
            0xD5033FDF
        );
    }
    #[test]
    fn brk_1() {
        assert_eq!(Inst::Brk { imm16: 1 }.encode(), 0xD4200020);
    }
}
