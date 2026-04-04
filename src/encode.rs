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

/// An ARM64 instruction that can be encoded to its 4-byte binary form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    // ---- Data processing (register) ----

    /// ADD Xd, Xn, Xm  (sf=true for 64-bit, false for 32-bit)
    AddReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// SUB Xd, Xn, Xm
    SubReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// ADDS Xd, Xn, Xm  (sets flags)
    AddsReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// SUBS Xd, Xn, Xm  (sets flags)
    SubsReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// MUL Xd, Xn, Xm  (alias for MADD Xd, Xn, Xm, XZR)
    Mul { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// SDIV Xd, Xn, Xm
    Sdiv { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// UDIV Xd, Xn, Xm
    Udiv { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },

    // ---- Logic (register) ----

    /// AND Xd, Xn, Xm
    AndReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// ORR Xd, Xn, Xm
    OrrReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// ORN Xd, Xn, Xm
    OrnReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// EOR Xd, Xn, Xm
    EorReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },
    /// ANDS Xd, Xn, Xm  (sets flags — TST is ANDS with XZR dest)
    AndsReg { rd: GpReg, rn: GpReg, rm: GpReg, sf: bool },

    // ---- Data processing (immediate) ----

    /// ADD Xd, Xn, #imm12{, LSL #12}
    AddImm { rd: GpReg, rn: GpReg, imm12: u16, shift: bool, sf: bool },
    /// SUB Xd, Xn, #imm12{, LSL #12}
    SubImm { rd: GpReg, rn: GpReg, imm12: u16, shift: bool, sf: bool },
    /// ADDS Xd, Xn, #imm12{, LSL #12}
    AddsImm { rd: GpReg, rn: GpReg, imm12: u16, shift: bool, sf: bool },
    /// SUBS Xd, Xn, #imm12{, LSL #12}
    SubsImm { rd: GpReg, rn: GpReg, imm12: u16, shift: bool, sf: bool },

    // ---- Move (wide immediate) ----

    /// MOVZ Xd, #imm16{, LSL #shift}  (shift = 0, 16, 32, 48)
    Movz { rd: GpReg, imm16: u16, shift: u8, sf: bool },
    /// MOVK Xd, #imm16{, LSL #shift}  (keep other bits)
    Movk { rd: GpReg, imm16: u16, shift: u8, sf: bool },
    /// MOVN Xd, #imm16{, LSL #shift}  (move NOT)
    Movn { rd: GpReg, imm16: u16, shift: u8, sf: bool },

    // ---- Shifts (aliases for UBFM/SBFM/EXTR) ----

    /// LSL Xd, Xn, #amount  (alias for UBFM)
    LslImm { rd: GpReg, rn: GpReg, amount: u8, sf: bool },
    /// LSR Xd, Xn, #amount  (alias for UBFM)
    LsrImm { rd: GpReg, rn: GpReg, amount: u8, sf: bool },
    /// ASR Xd, Xn, #amount  (alias for SBFM)
    AsrImm { rd: GpReg, rn: GpReg, amount: u8, sf: bool },

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
    /// RET {Xn}
    Ret { rn: GpReg },
    /// BR Xn  (indirect branch)
    Br { rn: GpReg },
    /// BLR Xn  (indirect call)
    Blr { rn: GpReg },
    /// CSINC Xd, Xn, Xm, cond
    Csinc { rd: GpReg, rn: GpReg, rm: GpReg, cond: Cond, sf: bool },

    // ---- Address generation ----

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
    /// LDR Xt, [Xn, Rm{, extend}]
    LdrReg64 { rt: GpReg, rn: GpReg, rm: GpReg, extend: AddrExtend, shift: bool },
    /// LDR Wt, [Xn, Rm{, extend}]
    LdrReg32 { rt: GpReg, rn: GpReg, rm: GpReg, extend: AddrExtend, shift: bool },
    /// STR Xt, [Xn, Rm{, extend}]
    StrReg64 { rt: GpReg, rn: GpReg, rm: GpReg, extend: AddrExtend, shift: bool },
    /// STR Wt, [Xn, Rm{, extend}]
    StrReg32 { rt: GpReg, rn: GpReg, rm: GpReg, extend: AddrExtend, shift: bool },
    /// LDRB Wt, [Xn, #offset]  (byte load, zero-extend)
    Ldrb { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRH Wt, [Xn, #offset]  (halfword load, zero-extend)
    Ldrh { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRSW Xt, [Xn, #offset]  (32-bit load, sign-extend to 64)
    Ldrsw { rt: GpReg, rn: GpReg, offset: u16 },
    /// LDRB Wt, [Xn, Rm{, extend}]
    LdrbReg { rt: GpReg, rn: GpReg, rm: GpReg, extend: AddrExtend, shift: bool },
    /// LDRH Wt, [Xn, Rm{, extend}]
    LdrhReg { rt: GpReg, rn: GpReg, rm: GpReg, extend: AddrExtend, shift: bool },
    /// LDRSW Xt, [Xn, Rm{, extend}]
    LdrswReg { rt: GpReg, rn: GpReg, rm: GpReg, extend: AddrExtend, shift: bool },

    /// LDR Xt, label
    LdrLit64 { rt: GpReg, offset: i32 },
    /// LDR Wt, label
    LdrLit32 { rt: GpReg, offset: i32 },
    /// LDRSW Xt, label
    LdrswLit { rt: GpReg, offset: i32 },

    // ---- Load/Store (pre-index) ----

    /// LDR Xt, [Xn, #offset]!  (pre-index, 64-bit)
    LdrPre64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STR Xt, [Xn, #offset]!  (pre-index, 64-bit)
    StrPre64 { rt: GpReg, rn: GpReg, offset: i16 },

    // ---- Load/Store (post-index) ----

    /// LDR Xt, [Xn], #offset  (post-index, 64-bit)
    LdrPost64 { rt: GpReg, rn: GpReg, offset: i16 },
    /// STR Xt, [Xn], #offset  (post-index, 64-bit)
    StrPost64 { rt: GpReg, rn: GpReg, offset: i16 },

    // ---- Load/Store pair ----

    /// STP Xt1, Xt2, [Xn, #offset]  (signed offset, 64-bit)
    StpOff64 { rt1: GpReg, rt2: GpReg, rn: GpReg, offset: i16 },
    /// LDP Xt1, Xt2, [Xn, #offset]  (signed offset, 64-bit)
    LdpOff64 { rt1: GpReg, rt2: GpReg, rn: GpReg, offset: i16 },
    /// STP Xt1, Xt2, [Xn, #offset]!  (pre-index, 64-bit)
    StpPre64 { rt1: GpReg, rt2: GpReg, rn: GpReg, offset: i16 },
    /// STP Xt1, Xt2, [Xn], #offset  (post-index, 64-bit)
    StpPost64 { rt1: GpReg, rt2: GpReg, rn: GpReg, offset: i16 },
    /// LDP Xt1, Xt2, [Xn, #offset]!  (pre-index, 64-bit)
    LdpPre64 { rt1: GpReg, rt2: GpReg, rn: GpReg, offset: i16 },
    /// LDP Xt1, Xt2, [Xn], #offset  (post-index, 64-bit)
    LdpPost64 { rt1: GpReg, rt2: GpReg, rn: GpReg, offset: i16 },

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
    /// FSUB Sd, Sn, Sm  (single)
    FsubS { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FMUL Sd, Sn, Sm  (single)
    FmulS { rd: FpReg, rn: FpReg, rm: FpReg },
    /// FDIV Sd, Sn, Sm  (single)
    FdivS { rd: FpReg, rn: FpReg, rm: FpReg },
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
    /// FMADD Dd, Dn, Dm, Da  (Dd = Da + Dn*Dm)
    FmaddD { rd: FpReg, rn: FpReg, rm: FpReg, ra: FpReg },
    /// FMADD Sd, Sn, Sm, Sa
    FmaddS { rd: FpReg, rn: FpReg, rm: FpReg, ra: FpReg },

    // ---- FP / integer conversion ----

    /// FCVTZS Xd, Dn  (double -> signed 64-bit int, truncate toward zero)
    FcvtzsD { rd: GpReg, rn: FpReg },
    /// SCVTF Dd, Xn  (signed 64-bit int -> double)
    ScvtfD { rd: FpReg, rn: GpReg },
    /// FMOV Dd, Xn  (move bits GP -> FP, no conversion)
    FmovToD { rd: FpReg, rn: GpReg },
    /// FMOV Xd, Dn  (move bits FP -> GP, no conversion)
    FmovFromD { rd: GpReg, rn: FpReg },

    // ---- System ----

    /// SVC #imm16
    Svc { imm16: u16 },
    /// NOP
    Nop,
    /// BRK #imm16
    Brk { imm16: u16 },
}

impl Inst {
    /// Encode this instruction into its 32-bit binary representation.
    pub fn encode(&self) -> u32 {
        match self {
            // ---- Data processing (register) ----
            Inst::AddReg { rd, rn, rm, sf } =>
                dp_reg(*sf, 0b00, 0b01011, 0b00, *rm, 0, *rn, *rd),
            Inst::SubReg { rd, rn, rm, sf } =>
                dp_reg(*sf, 0b10, 0b01011, 0b00, *rm, 0, *rn, *rd),
            Inst::AddsReg { rd, rn, rm, sf } =>
                dp_reg(*sf, 0b01, 0b01011, 0b00, *rm, 0, *rn, *rd),
            Inst::SubsReg { rd, rn, rm, sf } =>
                dp_reg(*sf, 0b11, 0b01011, 0b00, *rm, 0, *rn, *rd),

            // MUL: alias for MADD Xd, Xn, Xm, XZR
            // sf|00|11011|000|Rm|0|Ra(11111)|Rn|Rd
            Inst::Mul { rd, rn, rm, sf } => {
                let s = (*sf as u32) << 31;
                s | (0b00_11011_000 << 21) | (rm.enc() << 16) | (0b0_11111 << 10)
                    | (rn.enc() << 5) | rd.enc()
            }
            // SDIV: sf|0|0|11010110|Rm|00001|1|Rn|Rd
            Inst::Sdiv { rd, rn, rm, sf } => {
                let s = (*sf as u32) << 31;
                s | (0b0_0_11010110 << 21) | (rm.enc() << 16) | (0b000011 << 10)
                    | (rn.enc() << 5) | rd.enc()
            }
            // UDIV: sf|0|0|11010110|Rm|00001|0|Rn|Rd
            Inst::Udiv { rd, rn, rm, sf } => {
                let s = (*sf as u32) << 31;
                s | (0b0_0_11010110 << 21) | (rm.enc() << 16) | (0b000010 << 10)
                    | (rn.enc() << 5) | rd.enc()
            }

            // ---- Logic (register) ----
            Inst::AndReg { rd, rn, rm, sf } => logic_reg(*sf, 0b00, false, *rm, *rn, *rd),
            Inst::OrrReg { rd, rn, rm, sf } => logic_reg(*sf, 0b01, false, *rm, *rn, *rd),
            Inst::OrnReg { rd, rn, rm, sf } => logic_reg(*sf, 0b01, true, *rm, *rn, *rd),
            Inst::EorReg { rd, rn, rm, sf } => logic_reg(*sf, 0b10, false, *rm, *rn, *rd),
            Inst::AndsReg { rd, rn, rm, sf } => logic_reg(*sf, 0b11, false, *rm, *rn, *rd),

            // ---- Data processing (immediate) ----
            Inst::AddImm { rd, rn, imm12, shift, sf } =>
                dp_imm(*sf, 0b00, *imm12, *shift, *rn, *rd),
            Inst::SubImm { rd, rn, imm12, shift, sf } =>
                dp_imm(*sf, 0b10, *imm12, *shift, *rn, *rd),
            Inst::AddsImm { rd, rn, imm12, shift, sf } =>
                dp_imm(*sf, 0b01, *imm12, *shift, *rn, *rd),
            Inst::SubsImm { rd, rn, imm12, shift, sf } =>
                dp_imm(*sf, 0b11, *imm12, *shift, *rn, *rd),

            // ---- Move (wide immediate) ----
            Inst::Movz { rd, imm16, shift, sf } => mov_wide(*sf, 0b10, *imm16, *shift, *rd),
            Inst::Movk { rd, imm16, shift, sf } => mov_wide(*sf, 0b11, *imm16, *shift, *rd),
            Inst::Movn { rd, imm16, shift, sf } => mov_wide(*sf, 0b00, *imm16, *shift, *rd),

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
            Inst::Ret { rn } => 0xD65F0000 | (rn.enc() << 5),
            Inst::Br { rn } => 0xD61F0000 | (rn.enc() << 5),
            Inst::Blr { rn } => 0xD63F0000 | (rn.enc() << 5),
            Inst::Csinc { rd, rn, rm, cond, sf } => csel(*sf, 0b01, *rm, *cond, *rn, *rd),

            // ---- Address generation ----
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
            Inst::LdrReg64 { rt, rn, rm, extend, shift } =>
                ldst_reg(0b11, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrReg32 { rt, rn, rm, extend, shift } =>
                ldst_reg(0b10, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::StrReg64 { rt, rn, rm, extend, shift } =>
                ldst_reg(0b11, 0b00, *rm, *extend, *shift, *rn, *rt),
            Inst::StrReg32 { rt, rn, rm, extend, shift } =>
                ldst_reg(0b10, 0b00, *rm, *extend, *shift, *rn, *rt),
            Inst::Ldrb { rt, rn, offset } => {
                let uoff = *offset as u32;
                (0b00_111_0_01_01 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldrh { rt, rn, offset } => {
                let uoff = (*offset / 2) as u32;
                (0b01_111_0_01_01 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::Ldrsw { rt, rn, offset } => {
                let uoff = (*offset / 4) as u32;
                (0b10_111_0_01_10 << 22) | (uoff << 10) | (rn.enc() << 5) | rt.enc()
            }
            Inst::LdrbReg { rt, rn, rm, extend, shift } =>
                ldst_reg(0b00, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrhReg { rt, rn, rm, extend, shift } =>
                ldst_reg(0b01, 0b01, *rm, *extend, *shift, *rn, *rt),
            Inst::LdrswReg { rt, rn, rm, extend, shift } =>
                ldst_reg(0b10, 0b10, *rm, *extend, *shift, *rn, *rt),

            Inst::LdrLit64 { rt, offset } => ldr_lit(0b01, *offset, *rt),
            Inst::LdrLit32 { rt, offset } => ldr_lit(0b00, *offset, *rt),
            Inst::LdrswLit { rt, offset } => ldr_lit(0b10, *offset, *rt),

            // ---- Load/Store (pre/post-index) ----
            Inst::LdrPre64 { rt, rn, offset } => ldst_idx(0b11, 0b01, *offset, 0b11, *rn, *rt),
            Inst::StrPre64 { rt, rn, offset } => ldst_idx(0b11, 0b00, *offset, 0b11, *rn, *rt),
            Inst::LdrPost64 { rt, rn, offset } => ldst_idx(0b11, 0b01, *offset, 0b01, *rn, *rt),
            Inst::StrPost64 { rt, rn, offset } => ldst_idx(0b11, 0b00, *offset, 0b01, *rn, *rt),

            // ---- Load/Store pair ----
            Inst::StpOff64 { rt1, rt2, rn, offset } =>
                ldp_stp(0b10, 0b010, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpOff64 { rt1, rt2, rn, offset } =>
                ldp_stp(0b10, 0b010, 1, *offset, *rt2, *rn, *rt1),
            Inst::StpPre64 { rt1, rt2, rn, offset } =>
                ldp_stp(0b10, 0b011, 0, *offset, *rt2, *rn, *rt1),
            Inst::StpPost64 { rt1, rt2, rn, offset } =>
                ldp_stp(0b10, 0b001, 0, *offset, *rt2, *rn, *rt1),
            Inst::LdpPre64 { rt1, rt2, rn, offset } =>
                ldp_stp(0b10, 0b011, 1, *offset, *rt2, *rn, *rt1),
            Inst::LdpPost64 { rt1, rt2, rn, offset } =>
                ldp_stp(0b10, 0b001, 1, *offset, *rt2, *rn, *rt1),

            // ---- FP arithmetic ----
            Inst::FaddD { rd, rn, rm } => fp_arith(0b01, 0b0010, *rm, *rn, *rd),
            Inst::FsubD { rd, rn, rm } => fp_arith(0b01, 0b0011, *rm, *rn, *rd),
            Inst::FmulD { rd, rn, rm } => fp_arith(0b01, 0b0000, *rm, *rn, *rd),
            Inst::FdivD { rd, rn, rm } => fp_arith(0b01, 0b0001, *rm, *rn, *rd),
            Inst::FaddS { rd, rn, rm } => fp_arith(0b00, 0b0010, *rm, *rn, *rd),
            Inst::FsubS { rd, rn, rm } => fp_arith(0b00, 0b0011, *rm, *rn, *rd),
            Inst::FmulS { rd, rn, rm } => fp_arith(0b00, 0b0000, *rm, *rn, *rd),
            Inst::FdivS { rd, rn, rm } => fp_arith(0b00, 0b0001, *rm, *rn, *rd),

            Inst::FnegD { rd, rn } => fp_unary(0b01, 0b0000_10, *rn, *rd),
            Inst::FnegS { rd, rn } => fp_unary(0b00, 0b0000_10, *rn, *rd),
            Inst::FabsD { rd, rn } => fp_unary(0b01, 0b0000_01, *rn, *rd),
            Inst::FabsS { rd, rn } => fp_unary(0b00, 0b0000_01, *rn, *rd),
            Inst::FsqrtD { rd, rn } => fp_unary(0b01, 0b0000_11, *rn, *rd),
            Inst::FsqrtS { rd, rn } => fp_unary(0b00, 0b0000_11, *rn, *rd),
            Inst::FcmpD { rn, rm } => fp_cmp(0b01, *rn, *rm),
            Inst::FcmpS { rn, rm } => fp_cmp(0b00, *rn, *rm),
            Inst::FmaddD { rd, rn, rm, ra } => fp_madd(0b01, *rd, *rn, *rm, *ra),
            Inst::FmaddS { rd, rn, rm, ra } => fp_madd(0b00, *rd, *rn, *rm, *ra),

            // ---- FP / integer conversion ----
            Inst::FcvtzsD { rd, rn } => {
                (0b1_00_11110_01_1 << 21) | (0b11_000 << 16)
                    | (rn.enc() << 5) | rd.enc()
            }
            Inst::ScvtfD { rd, rn } => {
                (0b1_00_11110_01_1 << 21) | (0b00_010 << 16)
                    | (rn.enc() << 5) | rd.enc()
            }
            Inst::FmovToD { rd, rn } => {
                (0b1_00_11110_01_1 << 21) | (0b00_111 << 16)
                    | (rn.enc() << 5) | rd.enc()
            }
            Inst::FmovFromD { rd, rn } => {
                (0b1_00_11110_01_1 << 21) | (0b00_110 << 16)
                    | (rn.enc() << 5) | rd.enc()
            }

            // ---- System ----
            Inst::Svc { imm16 } => {
                (0b11010100_000 << 21) | ((*imm16 as u32) << 5) | 0b000_01
            }
            Inst::Nop => 0xD503201F,
            Inst::Brk { imm16 } => {
                (0b11010100_001 << 21) | ((*imm16 as u32) << 5)
            }
        }
    }
}

// ---- Encoding helpers ----

#[allow(clippy::too_many_arguments)]
fn dp_reg(sf: bool, opc: u32, fixed: u32, shift: u32, rm: GpReg, imm6: u32, rn: GpReg, rd: GpReg) -> u32 {
    ((sf as u32) << 31) | (opc << 29) | (fixed << 24) | (shift << 22)
        | (rm.enc() << 16) | (imm6 << 10) | (rn.enc() << 5) | rd.enc()
}

fn logic_reg(sf: bool, opc: u32, n: bool, rm: GpReg, rn: GpReg, rd: GpReg) -> u32 {
    ((sf as u32) << 31) | (opc << 29) | (0b01010 << 24) | ((n as u32) << 21)
        | (rm.enc() << 16) | (rn.enc() << 5) | rd.enc()
}

fn dp_imm(sf: bool, op: u32, imm12: u16, shift: bool, rn: GpReg, rd: GpReg) -> u32 {
    ((sf as u32) << 31) | (op << 29) | (0b100010 << 23) | ((shift as u32) << 22)
        | ((imm12 as u32) << 10) | (rn.enc() << 5) | rd.enc()
}

fn mov_wide(sf: bool, opc: u32, imm16: u16, shift: u8, rd: GpReg) -> u32 {
    let hw = (shift / 16) as u32;
    ((sf as u32) << 31) | (opc << 29) | (0b100101 << 23) | (hw << 21)
        | ((imm16 as u32) << 5) | rd.enc()
}

fn bitfield(sf: bool, opc: u32, immr: u8, imms: u8, rn: GpReg, rd: GpReg) -> u32 {
    let n = sf as u32;
    ((sf as u32) << 31) | (opc << 29) | (0b100110 << 23) | (n << 22)
        | ((immr as u32) << 16) | ((imms as u32) << 10)
        | (rn.enc() << 5) | rd.enc()
}

fn csel(sf: bool, op: u32, rm: GpReg, cond: Cond, rn: GpReg, rd: GpReg) -> u32 {
    ((sf as u32) << 31) | (0b0011010100 << 21) | (rm.enc() << 16)
        | (cond.enc() << 12) | (op << 10) | (rn.enc() << 5) | rd.enc()
}

fn ldst_idx(size: u32, opc: u32, offset: i16, idx: u32, rn: GpReg, rt: GpReg) -> u32 {
    let imm9 = (offset as u32) & 0x1FF;
    (size << 30) | (0b111_0_00 << 24) | (opc << 22)
        | (imm9 << 12) | (idx << 10) | (rn.enc() << 5) | rt.enc()
}

fn ldst_reg(size: u32, opc: u32, rm: GpReg, extend: AddrExtend, shift: bool, rn: GpReg, rt: GpReg) -> u32 {
    (size << 30) | (0b111 << 27) | (opc << 22) | (1 << 21) | (rm.enc() << 16)
        | (extend.enc() << 13) | ((shift as u32) << 12) | (0b10 << 10)
        | (rn.enc() << 5) | rt.enc()
}

fn ldr_lit(opc: u32, offset: i32, rt: GpReg) -> u32 {
    let imm19 = ((offset >> 2) as u32) & 0x7FFFF;
    (opc << 30) | (0b011 << 27) | (imm19 << 5) | rt.enc()
}

/// Load/store pair.
/// Format: opc(2)|101|mode(3)|L(1)|imm7(7)|Rt2(5)|Rn(5)|Rt1(5)
/// mode: 001=post-index, 010=signed-offset, 011=pre-index
fn ldp_stp(opc: u32, mode: u32, l: u32, offset: i16, rt2: GpReg, rn: GpReg, rt1: GpReg) -> u32 {
    let imm7 = ((offset / 8) as u32) & 0x7F;
    (opc << 30) | (0b101 << 27) | (mode << 23) | (l << 22)
        | (imm7 << 15) | (rt2.enc() << 10) | (rn.enc() << 5) | rt1.enc()
}

/// FP one-source (FNEG, FABS, FSQRT).
/// Format: 0|00|11110|ftype(2)|1|opcode(6)|10000|Rn(5)|Rd(5)
fn fp_unary(ftype: u32, opcode: u32, rn: FpReg, rd: FpReg) -> u32 {
    (0b000_11110 << 24) | (ftype << 22) | (1 << 21)
        | (opcode << 15) | (0b10000 << 10) | (rn.enc() << 5) | rd.enc()
}

/// FCMP Rn, Rm.
/// Format: 0|00|11110|ftype(2)|1|Rm(5)|00|1000|Rn(5)|00000
fn fp_cmp(ftype: u32, rn: FpReg, rm: FpReg) -> u32 {
    (0b000_11110 << 24) | (ftype << 22) | (1 << 21)
        | (rm.enc() << 16) | (0b00_1000 << 10) | (rn.enc() << 5)
}

/// FMADD Rd, Rn, Rm, Ra.
/// Format: 0|00|11111|ftype(2)|0|Rm(5)|0|Ra(5)|Rn(5)|Rd(5)
fn fp_madd(ftype: u32, rd: FpReg, rn: FpReg, rm: FpReg, ra: FpReg) -> u32 {
    (0b000_11111 << 24) | (ftype << 22)
        | (rm.enc() << 16) | (ra.enc() << 10) | (rn.enc() << 5) | rd.enc()
}

/// FP two-source arithmetic (FADD, FSUB, FMUL, FDIV).
fn fp_arith(ftype: u32, opcode: u32, rm: FpReg, rn: FpReg, rd: FpReg) -> u32 {
    (0b000_11110 << 24) | (ftype << 22) | (1 << 21)
        | (rm.enc() << 16) | (opcode << 12) | (0b10 << 10)
        | (rn.enc() << 5) | rd.enc()
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

    #[test] fn add_x0_x1_x2()   { assert_eq!(Inst::AddReg  { rd: X0, rn: X1, rm: X2, sf: true  }.encode(), 0x8B020020); }
    #[test] fn sub_x3_x4_x5()   { assert_eq!(Inst::SubReg  { rd: X3, rn: X4, rm: X5, sf: true  }.encode(), 0xCB050083); }
    #[test] fn add_w0_w1_w2()   { assert_eq!(Inst::AddReg  { rd: W0, rn: W1, rm: W2, sf: false }.encode(), 0x0B020020); }
    #[test] fn sub_w3_w4_w5()   { assert_eq!(Inst::SubReg  { rd: W3, rn: W4, rm: W5, sf: false }.encode(), 0x4B050083); }
    #[test] fn mul_x6_x7_x8()   { assert_eq!(Inst::Mul     { rd: X6, rn: X7, rm: X8, sf: true  }.encode(), 0x9B087CE6); }
    #[test] fn sdiv_x9_x10_x11(){ assert_eq!(Inst::Sdiv    { rd: X9, rn: X10, rm: X11, sf: true }.encode(), 0x9ACB0D49); }
    #[test] fn udiv_x12_x13_x14(){ assert_eq!(Inst::Udiv   { rd: X12, rn: X13, rm: X14, sf: true }.encode(), 0x9ACE09AC); }

    // ---- Logic (register) ----

    #[test] fn and_x0_x1_x2() { assert_eq!(Inst::AndReg  { rd: X0, rn: X1, rm: X2, sf: true }.encode(), 0x8A020020); }
    #[test] fn orr_x0_x1_x2() { assert_eq!(Inst::OrrReg  { rd: X0, rn: X1, rm: X2, sf: true }.encode(), 0xAA020020); }
    #[test] fn orn_x0_xzr_x1() { assert_eq!(Inst::OrnReg { rd: X0, rn: XZR, rm: X1, sf: true }.encode(), 0xAA2103E0); }
    #[test] fn eor_x0_x1_x2() { assert_eq!(Inst::EorReg  { rd: X0, rn: X1, rm: X2, sf: true }.encode(), 0xCA020020); }

    // ---- Data processing (immediate) ----

    #[test] fn add_x0_x1_42()      { assert_eq!(Inst::AddImm { rd: X0, rn: X1, imm12: 42, shift: false, sf: true }.encode(), 0x9100A820); }
    #[test] fn sub_x0_x1_42()      { assert_eq!(Inst::SubImm { rd: X0, rn: X1, imm12: 42, shift: false, sf: true }.encode(), 0xD100A820); }
    #[test] fn add_x0_x1_42_lsl12(){ assert_eq!(Inst::AddImm { rd: X0, rn: X1, imm12: 42, shift: true,  sf: true }.encode(), 0x9140A820); }

    // ---- Move (wide immediate) ----

    #[test] fn movz_x0_0x1234()       { assert_eq!(Inst::Movz { rd: X0, imm16: 0x1234, shift: 0,  sf: true }.encode(), 0xD2824680); }
    #[test] fn movz_x0_0x5678_lsl16() { assert_eq!(Inst::Movz { rd: X0, imm16: 0x5678, shift: 16, sf: true }.encode(), 0xD2AACF00); }
    #[test] fn movk_x0_0xabcd_lsl32() { assert_eq!(Inst::Movk { rd: X0, imm16: 0xABCD, shift: 32, sf: true }.encode(), 0xF2D579A0); }
    #[test] fn movn_x0_0()            { assert_eq!(Inst::Movn { rd: X0, imm16: 0,      shift: 0,  sf: true }.encode(), 0x92800000); }

    // ---- Compare (SUBS/ADDS/ANDS to XZR) ----

    #[test] fn cmp_x0_x1() { assert_eq!(Inst::SubsReg { rd: XZR, rn: X0, rm: X1, sf: true }.encode(), 0xEB01001F); }
    #[test] fn cmn_x0_x1() { assert_eq!(Inst::AddsReg { rd: XZR, rn: X0, rm: X1, sf: true }.encode(), 0xAB01001F); }
    #[test] fn tst_x0_x1() { assert_eq!(Inst::AndsReg { rd: XZR, rn: X0, rm: X1, sf: true }.encode(), 0xEA01001F); }

    // ---- Shifts ----

    #[test] fn lsl_x0_x1_3() { assert_eq!(Inst::LslImm { rd: X0, rn: X1, amount: 3, sf: true }.encode(), 0xD37DF020); }
    #[test] fn lsr_x0_x1_3() { assert_eq!(Inst::LsrImm { rd: X0, rn: X1, amount: 3, sf: true }.encode(), 0xD343FC20); }
    #[test] fn asr_x0_x1_3() { assert_eq!(Inst::AsrImm { rd: X0, rn: X1, amount: 3, sf: true }.encode(), 0x9343FC20); }

    // W-register shifts (32-bit, different immr/imms field widths)
    #[test] fn lsl_w0_w1_3()  { assert_eq!(Inst::LslImm { rd: W0, rn: W1, amount: 3, sf: false }.encode(), 0x531D7020); }
    #[test] fn lsr_w5_w6_8()  { assert_eq!(Inst::LsrImm { rd: W5, rn: W6, amount: 8, sf: false }.encode(), 0x53087CC5); }
    #[test] fn asr_w5_w6_15() { assert_eq!(Inst::AsrImm { rd: W5, rn: W6, amount: 15, sf: false }.encode(), 0x130F7CC5); }

    // ---- Branches ----

    #[test] fn b_plus4()        { assert_eq!(Inst::B  { offset: 4 }.encode(),   0x14000001); }
    #[test] fn b_minus8()       { assert_eq!(Inst::B  { offset: -8 }.encode(),  0x17FFFFFE); }
    #[test] fn bl_plus16()      { assert_eq!(Inst::Bl { offset: 16 }.encode(),  0x94000004); }
    #[test] fn b_eq_plus8()     { assert_eq!(Inst::BCond { cond: Cond::EQ, offset: 8  }.encode(), 0x54000040); }
    #[test] fn b_ne_plus12()    { assert_eq!(Inst::BCond { cond: Cond::NE, offset: 12 }.encode(), 0x54000061); }
    #[test] fn b_ge_plus16()    { assert_eq!(Inst::BCond { cond: Cond::GE, offset: 16 }.encode(), 0x5400008A); }
    #[test] fn cbz_x0_plus8()   { assert_eq!(Inst::Cbz  { rt: X0, offset: 8,  sf: true }.encode(), 0xB4000040); }
    #[test] fn cbnz_x1_plus12() { assert_eq!(Inst::Cbnz { rt: X1, offset: 12, sf: true }.encode(), 0xB5000061); }
    #[test] fn ret_x30()        { assert_eq!(Inst::Ret { rn: X30 }.encode(), 0xD65F03C0); }
    #[test] fn br_x16()         { assert_eq!(Inst::Br  { rn: X16 }.encode(), 0xD61F0200); }
    #[test] fn blr_x17()        { assert_eq!(Inst::Blr { rn: X17 }.encode(), 0xD63F0220); }
    #[test] fn csinc_x2_x3_x3_ne() { assert_eq!(Inst::Csinc { rd: X2, rn: X3, rm: X3, cond: Cond::NE, sf: true }.encode(), 0x9A831462); }

    // ---- Address generation ----

    #[test] fn adrp_x0_0() { assert_eq!(Inst::Adrp { rd: X0, imm: 0 }.encode(), 0x90000000); }

    // ---- Load/Store (unsigned offset) ----

    #[test] fn ldr_x0_x1()      { assert_eq!(Inst::LdrImm64 { rt: X0, rn: X1, offset: 0  }.encode(), 0xF9400020); }
    #[test] fn ldr_x0_x1_8()    { assert_eq!(Inst::LdrImm64 { rt: X0, rn: X1, offset: 8  }.encode(), 0xF9400420); }
    #[test] fn str_x2_x3_16()   { assert_eq!(Inst::StrImm64 { rt: X2, rn: X3, offset: 16 }.encode(), 0xF9000862); }
    #[test] fn ldr_w4_x5_4()    { assert_eq!(Inst::LdrImm32 { rt: W4, rn: X5, offset: 4  }.encode(), 0xB94004A4); }
    #[test] fn str_w6_x7_8()    { assert_eq!(Inst::StrImm32 { rt: W6, rn: X7, offset: 8  }.encode(), 0xB90008E6); }
    #[test] fn ldr_x0_x1_x2()   { assert_eq!(Inst::LdrReg64 { rt: X0, rn: X1, rm: X2, extend: AddrExtend::Lsl, shift: false }.encode(), 0xF8626820); }
    #[test] fn ldr_x3_x4_x5_lsl3() { assert_eq!(Inst::LdrReg64 { rt: X3, rn: X4, rm: X5, extend: AddrExtend::Lsl, shift: true }.encode(), 0xF8657883); }
    #[test] fn ldr_x6_x7_w8_uxtw3() { assert_eq!(Inst::LdrReg64 { rt: X6, rn: X7, rm: W8, extend: AddrExtend::Uxtw, shift: true }.encode(), 0xF86858E6); }
    #[test] fn ldr_x9_x10_x11_sxtx3() { assert_eq!(Inst::LdrReg64 { rt: X9, rn: X10, rm: X11, extend: AddrExtend::Sxtx, shift: true }.encode(), 0xF86BF949); }
    #[test] fn str_x12_x13_x14() { assert_eq!(Inst::StrReg64 { rt: X12, rn: X13, rm: X14, extend: AddrExtend::Lsl, shift: false }.encode(), 0xF82E69AC); }
    #[test] fn ldrb_w8_x9_1()   { assert_eq!(Inst::Ldrb     { rt: W8, rn: X9, offset: 1  }.encode(), 0x39400528); }
    #[test] fn ldrh_w10_x11_2() { assert_eq!(Inst::Ldrh     { rt: W10, rn: X11, offset: 2 }.encode(), 0x7940056A); }
    #[test] fn ldrsw_x0_x1_4()  { assert_eq!(Inst::Ldrsw    { rt: X0, rn: X1, offset: 4   }.encode(), 0xB9800420); }
    #[test] fn ldrb_w0_x1_x2()  { assert_eq!(Inst::LdrbReg  { rt: W0, rn: X1, rm: X2, extend: AddrExtend::Lsl, shift: false }.encode(), 0x38626820); }
    #[test] fn ldrh_w3_x4_w5_uxtw1() { assert_eq!(Inst::LdrhReg { rt: W3, rn: X4, rm: W5, extend: AddrExtend::Uxtw, shift: true }.encode(), 0x78655883); }
    #[test] fn ldrsw_x6_x7_w8_sxtw2() { assert_eq!(Inst::LdrswReg { rt: X6, rn: X7, rm: W8, extend: AddrExtend::Sxtw, shift: true }.encode(), 0xB8A8D8E6); }
    #[test] fn ldr_lit64_x0_plus8() { assert_eq!(Inst::LdrLit64 { rt: X0, offset: 8 }.encode(), 0x58000040); }
    #[test] fn ldr_lit32_w0_plus12() { assert_eq!(Inst::LdrLit32 { rt: W0, offset: 12 }.encode(), 0x18000060); }
    #[test] fn ldrsw_lit_x1_plus8() { assert_eq!(Inst::LdrswLit { rt: X1, offset: 8 }.encode(), 0x98000041); }

    // ---- Load/Store pair ----

    #[test] fn stp_x29_x30_sp_pre_m16() { assert_eq!(Inst::StpPre64  { rt1: X29, rt2: X30, rn: SP, offset: -16 }.encode(), 0xA9BF7BFD); }
    #[test] fn stp_x29_x30_sp_post_16() { assert_eq!(Inst::StpPost64 { rt1: X29, rt2: X30, rn: SP, offset: 16  }.encode(), 0xA8817BFD); }
    #[test] fn ldp_x29_x30_sp_pre_m16() { assert_eq!(Inst::LdpPre64  { rt1: X29, rt2: X30, rn: SP, offset: -16 }.encode(), 0xA9FF7BFD); }
    #[test] fn ldp_x29_x30_sp_post_16() { assert_eq!(Inst::LdpPost64 { rt1: X29, rt2: X30, rn: SP, offset: 16  }.encode(), 0xA8C17BFD); }
    #[test] fn stp_x19_x20_sp_post_32() { assert_eq!(Inst::StpPost64 { rt1: X19, rt2: X20, rn: SP, offset: 32  }.encode(), 0xA88253F3); }
    #[test] fn ldp_x19_x20_sp_pre_m32() { assert_eq!(Inst::LdpPre64  { rt1: X19, rt2: X20, rn: SP, offset: -32 }.encode(), 0xA9FE53F3); }
    #[test] fn stp_x19_x20_sp_16()      { assert_eq!(Inst::StpOff64  { rt1: X19, rt2: X20, rn: SP, offset: 16  }.encode(), 0xA90153F3); }
    #[test] fn ldp_x19_x20_sp_16()      { assert_eq!(Inst::LdpOff64  { rt1: X19, rt2: X20, rn: SP, offset: 16  }.encode(), 0xA94153F3); }

    // ---- Load/Store (pre/post-index) ----

    #[test] fn ldr_x0_x1_pre_8()  { assert_eq!(Inst::LdrPre64  { rt: X0, rn: X1, offset: 8 }.encode(), 0xF8408C20); }
    #[test] fn str_x0_x1_post_8() { assert_eq!(Inst::StrPost64 { rt: X0, rn: X1, offset: 8 }.encode(), 0xF8008420); }

    // ---- FP arithmetic ----

    #[test] fn fadd_d0_d1_d2()   { assert_eq!(Inst::FaddD { rd: D0, rn: D1, rm: D2  }.encode(), 0x1E622820); }
    #[test] fn fsub_d3_d4_d5()   { assert_eq!(Inst::FsubD { rd: D3, rn: D4, rm: D5  }.encode(), 0x1E653883); }
    #[test] fn fmul_d6_d7_d8()   { assert_eq!(Inst::FmulD { rd: D6, rn: D7, rm: D8  }.encode(), 0x1E6808E6); }
    #[test] fn fdiv_d9_d10_d11() { assert_eq!(Inst::FdivD { rd: D9, rn: D10, rm: D11 }.encode(), 0x1E6B1949); }
    #[test] fn fadd_s0_s1_s2()   { assert_eq!(Inst::FaddS { rd: S0, rn: S1, rm: S2  }.encode(), 0x1E222820); }
    #[test] fn fsub_s3_s4_s5()   { assert_eq!(Inst::FsubS { rd: S3, rn: S4, rm: S5  }.encode(), 0x1E253883); }
    #[test] fn fneg_d0_d1()      { assert_eq!(Inst::FnegD  { rd: D0, rn: D1 }.encode(), 0x1E614020); }
    #[test] fn fabs_d0_d1()      { assert_eq!(Inst::FabsD  { rd: D0, rn: D1 }.encode(), 0x1E60C020); }
    #[test] fn fsqrt_d0_d1()     { assert_eq!(Inst::FsqrtD { rd: D0, rn: D1 }.encode(), 0x1E61C020); }
    #[test] fn fcmp_d0_d1()      { assert_eq!(Inst::FcmpD  { rn: D0, rm: D1 }.encode(), 0x1E612000); }
    #[test] fn fmadd_d0_d1_d2_d3() { assert_eq!(Inst::FmaddD { rd: D0, rn: D1, rm: D2, ra: D3 }.encode(), 0x1F420C20); }

    // Single-precision FP unary/compare/fmadd
    #[test] fn fneg_s0_s1()         { assert_eq!(Inst::FnegS  { rd: S0, rn: S1 }.encode(), 0x1E214020); }
    #[test] fn fabs_s0_s1()         { assert_eq!(Inst::FabsS  { rd: S0, rn: S1 }.encode(), 0x1E20C020); }
    #[test] fn fsqrt_s0_s1()        { assert_eq!(Inst::FsqrtS { rd: S0, rn: S1 }.encode(), 0x1E21C020); }
    #[test] fn fcmp_s0_s1()         { assert_eq!(Inst::FcmpS  { rn: S0, rm: S1 }.encode(), 0x1E212000); }
    #[test] fn fmadd_s0_s1_s2_s3()  { assert_eq!(Inst::FmaddS { rd: S0, rn: S1, rm: S2, ra: S3 }.encode(), 0x1F020C20); }

    // ---- FP / integer conversion ----

    #[test] fn fcvtzs_x0_d1() { assert_eq!(Inst::FcvtzsD   { rd: X0, rn: D1 }.encode(), 0x9E780020); }
    #[test] fn scvtf_d0_x1()  { assert_eq!(Inst::ScvtfD    { rd: D0, rn: X1 }.encode(), 0x9E620020); }
    #[test] fn fmov_d0_x1()   { assert_eq!(Inst::FmovToD   { rd: D0, rn: X1 }.encode(), 0x9E670020); }
    #[test] fn fmov_x0_d1()   { assert_eq!(Inst::FmovFromD { rd: X0, rn: D1 }.encode(), 0x9E660020); }

    // ---- System ----

    #[test] fn svc_0x80() { assert_eq!(Inst::Svc { imm16: 0x80 }.encode(), 0xD4001001); }
    #[test] fn nop()      { assert_eq!(Inst::Nop.encode(),                  0xD503201F); }
    #[test] fn brk_1()    { assert_eq!(Inst::Brk { imm16: 1 }.encode(),    0xD4200020); }
}
