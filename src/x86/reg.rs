//! x86_64 register file with the full AT&T sub-register naming and
//! the REX interaction rules the encoder needs:
//!
//! - `spl/bpl/sil/dil` REQUIRE a REX prefix (any REX) to be encodable
//!   as byte registers — without REX those encodings mean `ah/ch/dh/bh`.
//! - `ah/ch/dh/bh` are FORBIDDEN in any instruction carrying a REX
//!   prefix.
//! - r8–r15 (and xmm8–15) set the relevant REX extension bit.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Width {
    B, // 8-bit
    W, // 16-bit
    L, // 32-bit
    Q, // 64-bit
    X, // 128-bit (xmm)
}

impl Width {
    pub fn bytes(self) -> u8 {
        match self {
            Width::B => 1,
            Width::W => 2,
            Width::L => 4,
            Width::Q => 8,
            Width::X => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegClass {
    Gp,
    /// ah/ch/dh/bh — encodings 4..7 without REX; REX-forbidden.
    GpHigh8,
    Xmm,
    /// x87 stack registers %st, %st(0)..%st(7) — the closed long-double
    /// arm (fstp %st(0), fucomip %st(1)). Width is nominal.
    St,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Reg {
    /// Architectural number 0-15. For GpHigh8 this is the number of
    /// the LOW register family (ah -> 0/rax family) — the ENCODING is
    /// `num + 4` and only valid without REX.
    pub num: u8,
    pub width: Width,
    pub class: RegClass,
    /// spl/bpl/sil/dil: byte register that needs a REX prefix (even
    /// an "empty" 0x40) to mean what it says.
    pub forces_rex: bool,
}

impl Reg {
    /// The 3-bit field for ModRM/SIB, before REX extension.
    pub fn low3(self) -> u8 {
        match self.class {
            RegClass::GpHigh8 => self.num + 4,
            _ => self.num & 0x7,
        }
    }

    /// Whether this register sets the REX extension bit (B/R/X
    /// depending on position).
    pub fn rex_bit(self) -> bool {
        self.class != RegClass::GpHigh8 && self.num >= 8
    }

    /// REX-forbidden: ah/ch/dh/bh.
    pub fn rex_forbidden(self) -> bool {
        self.class == RegClass::GpHigh8
    }

    /// Parse an AT&T register name WITHOUT the leading `%`.
    pub fn parse(name: &str) -> Option<Reg> {
        let gp = |num: u8, width: Width| -> Option<Reg> {
            Some(Reg {
                num,
                width,
                class: RegClass::Gp,
                forces_rex: false,
            })
        };
        // 64-bit
        const Q: [&str; 16] = [
            "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11",
            "r12", "r13", "r14", "r15",
        ];
        // 32-bit
        const L: [&str; 16] = [
            "eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "r8d", "r9d", "r10d", "r11d",
            "r12d", "r13d", "r14d", "r15d",
        ];
        // 16-bit
        const W: [&str; 16] = [
            "ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w",
            "r13w", "r14w", "r15w",
        ];
        // 8-bit low
        const B: [&str; 16] = [
            "al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil", "r8b", "r9b", "r10b", "r11b",
            "r12b", "r13b", "r14b", "r15b",
        ];
        for (i, n) in Q.iter().enumerate() {
            if *n == name {
                return gp(i as u8, Width::Q);
            }
        }
        for (i, n) in L.iter().enumerate() {
            if *n == name {
                return gp(i as u8, Width::L);
            }
        }
        for (i, n) in W.iter().enumerate() {
            if *n == name {
                return gp(i as u8, Width::W);
            }
        }
        for (i, n) in B.iter().enumerate() {
            if *n == name {
                let mut r = gp(i as u8, Width::B)?;
                // spl/bpl/sil/dil (4..=7) need REX to be themselves.
                if (4..=7).contains(&i) {
                    r.forces_rex = true;
                }
                return Some(r);
            }
        }
        // 8-bit high
        for (i, n) in ["ah", "ch", "dh", "bh"].iter().enumerate() {
            if *n == name {
                return Some(Reg {
                    num: i as u8,
                    width: Width::B,
                    class: RegClass::GpHigh8,
                    forces_rex: false,
                });
            }
        }
        // x87: st, st(0)..st(7)
        if name == "st" {
            return Some(Reg {
                num: 0,
                width: Width::X,
                class: RegClass::St,
                forces_rex: false,
            });
        }
        if let Some(rest) = name.strip_prefix("st(") {
            if let Some(digit) = rest.strip_suffix(')') {
                if let Ok(n) = digit.parse::<u8>() {
                    if n < 8 && digit.len() == 1 {
                        return Some(Reg {
                            num: n,
                            width: Width::X,
                            class: RegClass::St,
                            forces_rex: false,
                        });
                    }
                }
            }
            return None;
        }
        // xmm
        if let Some(rest) = name.strip_prefix("xmm") {
            if let Ok(n) = rest.parse::<u8>() {
                if n < 16 && (rest.len() == 1 || !rest.starts_with('0')) {
                    return Some(Reg {
                        num: n,
                        width: Width::X,
                        class: RegClass::Xmm,
                        forces_rex: false,
                    });
                }
            }
        }
        None
    }
}

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Canonical AT&T name, for diagnostics.
        const Q: [&str; 16] = [
            "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11",
            "r12", "r13", "r14", "r15",
        ];
        const L: [&str; 16] = [
            "eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi", "r8d", "r9d", "r10d", "r11d",
            "r12d", "r13d", "r14d", "r15d",
        ];
        const W: [&str; 16] = [
            "ax", "cx", "dx", "bx", "sp", "bp", "si", "di", "r8w", "r9w", "r10w", "r11w", "r12w",
            "r13w", "r14w", "r15w",
        ];
        const B: [&str; 16] = [
            "al", "cl", "dl", "bl", "spl", "bpl", "sil", "dil", "r8b", "r9b", "r10b", "r11b",
            "r12b", "r13b", "r14b", "r15b",
        ];
        let name = match (self.class, self.width) {
            (RegClass::GpHigh8, _) => ["ah", "ch", "dh", "bh"][self.num as usize].to_string(),
            (RegClass::Xmm, _) => format!("xmm{}", self.num),
            (RegClass::St, _) => format!("st({})", self.num),
            (RegClass::Gp, Width::Q) => Q[self.num as usize].to_string(),
            (RegClass::Gp, Width::L) => L[self.num as usize].to_string(),
            (RegClass::Gp, Width::W) => W[self.num as usize].to_string(),
            (RegClass::Gp, Width::B) => B[self.num as usize].to_string(),
            (RegClass::Gp, Width::X) => unreachable!("GP register with X width"),
        };
        write!(f, "%{}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sixtyfour_gp_names_parse() {
        // Every (family, width) pair maps to the right number/width.
        for (i, name) in ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"]
            .iter()
            .enumerate()
        {
            let r = Reg::parse(name).unwrap();
            assert_eq!((r.num, r.width, r.class), (i as u8, Width::Q, RegClass::Gp));
        }
        for i in 8..16u8 {
            assert_eq!(Reg::parse(&format!("r{}", i)).unwrap().num, i);
            assert_eq!(Reg::parse(&format!("r{}d", i)).unwrap().width, Width::L);
            assert_eq!(Reg::parse(&format!("r{}w", i)).unwrap().width, Width::W);
            assert_eq!(Reg::parse(&format!("r{}b", i)).unwrap().width, Width::B);
        }
        assert_eq!(Reg::parse("eax").unwrap().width, Width::L);
        assert_eq!(Reg::parse("ax").unwrap().width, Width::W);
        assert_eq!(Reg::parse("al").unwrap().width, Width::B);
    }

    #[test]
    fn rex_semantics() {
        // spl/bpl/sil/dil force REX; low3 is 4..7.
        for (i, name) in ["spl", "bpl", "sil", "dil"].iter().enumerate() {
            let r = Reg::parse(name).unwrap();
            assert!(r.forces_rex, "{} must force REX", name);
            assert!(!r.rex_forbidden());
            assert_eq!(r.low3(), 4 + i as u8);
            assert!(!r.rex_bit());
        }
        // ah/ch/dh/bh are REX-forbidden and encode as 4..7.
        for (i, name) in ["ah", "ch", "dh", "bh"].iter().enumerate() {
            let r = Reg::parse(name).unwrap();
            assert!(r.rex_forbidden(), "{} is REX-forbidden", name);
            assert!(!r.forces_rex);
            assert_eq!(r.low3(), 4 + i as u8);
            assert!(!r.rex_bit());
        }
        // r8+ set the extension bit and encode low3 0..7.
        let r9b = Reg::parse("r9b").unwrap();
        assert!(r9b.rex_bit());
        assert_eq!(r9b.low3(), 1);
        assert!(!r9b.forces_rex);
        // al does none of it.
        let al = Reg::parse("al").unwrap();
        assert!(!al.rex_bit() && !al.forces_rex && !al.rex_forbidden());
    }

    #[test]
    fn xmm_names() {
        for n in 0..16u8 {
            let r = Reg::parse(&format!("xmm{}", n)).unwrap();
            assert_eq!((r.num, r.width, r.class), (n, Width::X, RegClass::Xmm));
            assert_eq!(r.rex_bit(), n >= 8);
        }
        assert!(Reg::parse("xmm16").is_none());
        assert!(Reg::parse("xmm01").is_none());
    }

    #[test]
    fn junk_rejected_and_display_roundtrips() {
        for junk in ["rip", "eip", "st0", "cr0", "", "rax8", "xmm"] {
            assert!(Reg::parse(junk).is_none(), "{:?} must not parse", junk);
        }
        for name in ["rax", "r13d", "sil", "ah", "xmm12", "bp"] {
            let r = Reg::parse(name).unwrap();
            assert_eq!(format!("{}", r), format!("%{}", name));
        }
    }
}
