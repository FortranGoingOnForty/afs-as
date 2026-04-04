//! ARM64 register definitions.
//!
//! General purpose (X0-X30, W0-W30), FP/SIMD (D0-D31, S0-S31, Q0-Q31),
//! and special registers (SP, XZR, WZR).

/// A general-purpose register (integer).
///
/// ARM64 has 31 GP registers (R0-R30) accessible as either 64-bit X or 32-bit W.
/// R31 is context-dependent: stack pointer (SP) or zero register (XZR/WZR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpReg(u8);

impl GpReg {
    /// Create a GP register from its number (0-30, or 31 for SP/ZR).
    pub const fn new(n: u8) -> Self {
        assert!(n <= 31, "GP register number must be 0-31");
        Self(n)
    }

    /// The 5-bit encoding used in ARM64 instructions.
    pub const fn enc(self) -> u32 {
        self.0 as u32
    }

    /// Register number.
    pub const fn num(self) -> u8 {
        self.0
    }
}

// Named GP registers.
pub const X0: GpReg = GpReg::new(0);
pub const X1: GpReg = GpReg::new(1);
pub const X2: GpReg = GpReg::new(2);
pub const X3: GpReg = GpReg::new(3);
pub const X4: GpReg = GpReg::new(4);
pub const X5: GpReg = GpReg::new(5);
pub const X6: GpReg = GpReg::new(6);
pub const X7: GpReg = GpReg::new(7);
pub const X8: GpReg = GpReg::new(8);
pub const X9: GpReg = GpReg::new(9);
pub const X10: GpReg = GpReg::new(10);
pub const X11: GpReg = GpReg::new(11);
pub const X12: GpReg = GpReg::new(12);
pub const X13: GpReg = GpReg::new(13);
pub const X14: GpReg = GpReg::new(14);
pub const X15: GpReg = GpReg::new(15);
pub const X16: GpReg = GpReg::new(16);
pub const X17: GpReg = GpReg::new(17);
pub const X18: GpReg = GpReg::new(18);
pub const X19: GpReg = GpReg::new(19);
pub const X20: GpReg = GpReg::new(20);
pub const X21: GpReg = GpReg::new(21);
pub const X22: GpReg = GpReg::new(22);
pub const X23: GpReg = GpReg::new(23);
pub const X24: GpReg = GpReg::new(24);
pub const X25: GpReg = GpReg::new(25);
pub const X26: GpReg = GpReg::new(26);
pub const X27: GpReg = GpReg::new(27);
pub const X28: GpReg = GpReg::new(28);
pub const X29: GpReg = GpReg::new(29); // frame pointer
pub const X30: GpReg = GpReg::new(30); // link register
pub const XZR: GpReg = GpReg::new(31); // zero register
pub const SP: GpReg = GpReg::new(31);  // stack pointer (same encoding, different context)

// W aliases (same encoding, just signals 32-bit operation to the user).
pub const W0: GpReg = X0;
pub const W1: GpReg = X1;
pub const W2: GpReg = X2;
pub const W3: GpReg = X3;
pub const W4: GpReg = X4;
pub const W5: GpReg = X5;
pub const W6: GpReg = X6;
pub const W7: GpReg = X7;
pub const W8: GpReg = X8;
pub const W9: GpReg = X9;
pub const W10: GpReg = X10;
pub const W11: GpReg = X11;
pub const W12: GpReg = X12;
pub const W13: GpReg = X13;
pub const W14: GpReg = X14;
pub const W15: GpReg = X15;
pub const W16: GpReg = X16;
pub const W17: GpReg = X17;
pub const W18: GpReg = X18;
pub const W19: GpReg = X19;
pub const W20: GpReg = X20;
pub const W21: GpReg = X21;
pub const W22: GpReg = X22;
pub const W23: GpReg = X23;
pub const W24: GpReg = X24;
pub const W25: GpReg = X25;
pub const W26: GpReg = X26;
pub const W27: GpReg = X27;
pub const W28: GpReg = X28;
pub const W29: GpReg = X29;
pub const W30: GpReg = X30;
pub const WZR: GpReg = XZR;

/// A floating-point / SIMD register.
///
/// ARM64 has 32 FP/SIMD registers (V0-V31), accessible as:
/// - Qn (128-bit), Dn (64-bit double), Sn (32-bit single), Hn (16-bit half)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FpReg(u8);

impl FpReg {
    pub const fn new(n: u8) -> Self {
        assert!(n <= 31, "FP register number must be 0-31");
        Self(n)
    }

    pub const fn enc(self) -> u32 {
        self.0 as u32
    }

    pub const fn num(self) -> u8 {
        self.0
    }
}

pub const D0: FpReg = FpReg::new(0);
pub const D1: FpReg = FpReg::new(1);
pub const D2: FpReg = FpReg::new(2);
pub const D3: FpReg = FpReg::new(3);
pub const D4: FpReg = FpReg::new(4);
pub const D5: FpReg = FpReg::new(5);
pub const D6: FpReg = FpReg::new(6);
pub const D7: FpReg = FpReg::new(7);
pub const D8: FpReg = FpReg::new(8);
pub const D9: FpReg = FpReg::new(9);
pub const D10: FpReg = FpReg::new(10);
pub const D11: FpReg = FpReg::new(11);
pub const D12: FpReg = FpReg::new(12);
pub const D13: FpReg = FpReg::new(13);
pub const D14: FpReg = FpReg::new(14);
pub const D15: FpReg = FpReg::new(15);
pub const D16: FpReg = FpReg::new(16);
pub const D17: FpReg = FpReg::new(17);
pub const D18: FpReg = FpReg::new(18);
pub const D19: FpReg = FpReg::new(19);
pub const D20: FpReg = FpReg::new(20);
pub const D21: FpReg = FpReg::new(21);
pub const D22: FpReg = FpReg::new(22);
pub const D23: FpReg = FpReg::new(23);
pub const D24: FpReg = FpReg::new(24);
pub const D25: FpReg = FpReg::new(25);
pub const D26: FpReg = FpReg::new(26);
pub const D27: FpReg = FpReg::new(27);
pub const D28: FpReg = FpReg::new(28);
pub const D29: FpReg = FpReg::new(29);
pub const D30: FpReg = FpReg::new(30);
pub const D31: FpReg = FpReg::new(31);

// S aliases (same encoding, 32-bit FP).
pub const S0: FpReg = D0;
pub const S1: FpReg = D1;
pub const S2: FpReg = D2;
pub const S3: FpReg = D3;
pub const S4: FpReg = D4;
pub const S5: FpReg = D5;
pub const S6: FpReg = D6;
pub const S7: FpReg = D7;
pub const S8: FpReg = D8;
pub const S9: FpReg = D9;
pub const S10: FpReg = D10;
pub const S11: FpReg = D11;
pub const S12: FpReg = D12;
pub const S13: FpReg = D13;
pub const S14: FpReg = D14;
pub const S15: FpReg = D15;
pub const S16: FpReg = D16;
pub const S17: FpReg = D17;
pub const S18: FpReg = D18;
pub const S19: FpReg = D19;
pub const S20: FpReg = D20;
pub const S21: FpReg = D21;
pub const S22: FpReg = D22;
pub const S23: FpReg = D23;
pub const S24: FpReg = D24;
pub const S25: FpReg = D25;
pub const S26: FpReg = D26;
pub const S27: FpReg = D27;
pub const S28: FpReg = D28;
pub const S29: FpReg = D29;
pub const S30: FpReg = D30;
pub const S31: FpReg = D31;

/// ARM64 condition codes for conditional branches and selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Cond {
    EQ = 0b0000, // equal (Z=1)
    NE = 0b0001, // not equal (Z=0)
    CS = 0b0010, // carry set / unsigned >= (C=1)
    CC = 0b0011, // carry clear / unsigned < (C=0)
    MI = 0b0100, // minus / negative (N=1)
    PL = 0b0101, // plus / positive or zero (N=0)
    VS = 0b0110, // overflow (V=1)
    VC = 0b0111, // no overflow (V=0)
    HI = 0b1000, // unsigned > (C=1 && Z=0)
    LS = 0b1001, // unsigned <= (C=0 || Z=1)
    GE = 0b1010, // signed >= (N==V)
    LT = 0b1011, // signed < (N!=V)
    GT = 0b1100, // signed > (Z=0 && N==V)
    LE = 0b1101, // signed <= (Z=1 || N!=V)
    AL = 0b1110, // always
    NV = 0b1111, // always (alternate encoding)
}

// Unsigned aliases.
pub const HS: Cond = Cond::CS; // higher or same
pub const LO: Cond = Cond::CC; // lower

impl Cond {
    pub const fn enc(self) -> u32 {
        self as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gp_reg_encoding() {
        assert_eq!(X0.enc(), 0);
        assert_eq!(X15.enc(), 15);
        assert_eq!(X30.enc(), 30);
        assert_eq!(XZR.enc(), 31);
        assert_eq!(SP.enc(), 31);
    }

    #[test]
    fn fp_reg_encoding() {
        assert_eq!(D0.enc(), 0);
        assert_eq!(D31.enc(), 31);
        assert_eq!(S0.enc(), 0); // same encoding as D0
    }

    #[test]
    fn w_aliases_match_x() {
        assert_eq!(W0.enc(), X0.enc());
        assert_eq!(W29.enc(), X29.enc());
        assert_eq!(WZR.enc(), XZR.enc());
    }

    #[test]
    fn condition_codes() {
        assert_eq!(Cond::EQ.enc(), 0);
        assert_eq!(Cond::NE.enc(), 1);
        assert_eq!(Cond::GE.enc(), 0b1010);
        assert_eq!(Cond::LT.enc(), 0b1011);
        assert_eq!(Cond::AL.enc(), 0b1110);
        assert_eq!(HS.enc(), Cond::CS.enc());
        assert_eq!(LO.enc(), Cond::CC.enc());
    }
}
