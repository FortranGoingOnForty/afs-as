//! Seeded x86_64 test-case generator shared by the differential fuzz
//! and generated-stress suites. Draws only from the implemented
//! dialect (every form here has a per-instruction differential case),
//! but explores the cross-product of registers, widths, boundary
//! immediates, memory shapes, and branch distances far beyond what
//! the backend corpus happens to emit.

// Shared by multiple suites; not every suite uses every generator.
#![allow(dead_code)]

use std::fmt::Write as _;

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0xA076_1D64_78BD_642F)
    }

    pub fn next_u32(&mut self) -> u32 {
        // xorshift* — deterministic, no deps.
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 32) as u32
    }

    pub fn bounded(&mut self, upper: u32) -> u32 {
        self.next_u32() % upper.max(1)
    }

    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.bounded(xs.len() as u32) as usize]
    }

    pub fn chance(&mut self, percent: u32) -> bool {
        self.bounded(100) < percent
    }
}

const GP64: &[&str] = &[
    "%rax", "%rbx", "%rcx", "%rdx", "%rsi", "%rdi", "%r8", "%r9", "%r10", "%r11", "%r12", "%r13",
    "%r14", "%r15",
];
const GP32: &[&str] = &[
    "%eax", "%ebx", "%ecx", "%edx", "%esi", "%edi", "%r8d", "%r9d", "%r10d", "%r11d", "%r12d",
    "%r13d", "%r14d", "%r15d",
];
const GP16: &[&str] = &["%ax", "%bx", "%cx", "%dx", "%si", "%di", "%r8w", "%r13w"];
// No high-8 registers: they cannot pair with REX operands and the
// generator does not track that constraint.
const GP8: &[&str] = &[
    "%al", "%bl", "%cl", "%dl", "%sil", "%dil", "%spl", "%r9b", "%r14b",
];
const XMM: &[&str] = &[
    "%xmm0", "%xmm1", "%xmm2", "%xmm3", "%xmm4", "%xmm5", "%xmm6", "%xmm7", "%xmm8", "%xmm9",
    "%xmm10", "%xmm11", "%xmm12", "%xmm13", "%xmm14", "%xmm15",
];
// Sign-extension and short-form boundaries, both sides.
const IMM_BOUNDARY: &[i64] = &[
    0,
    1,
    7,
    127,
    128,
    -1,
    -128,
    -129,
    255,
    256,
    32767,
    32768,
    2147483647,
    -2147483648,
    2147483648,
    -2147483649,
    789750225429331968,
    9223372036854775807,
    -9223372036854775808,
];
const DISPS: &[i64] = &[-256, -128, -8, 0, 4, 8, 127, 128, 1024];
const SCALES: &[u8] = &[1, 4, 8];
const CONDS: &[&str] = &["e", "ne", "l", "le", "g", "ge", "a", "ae", "b", "be"];

fn mem(rng: &mut Rng) -> String {
    match rng.bounded(4) {
        0 => format!("{}(%rbp)", rng.pick(DISPS)),
        1 => format!("({})", rng.pick(GP64)),
        2 => format!(
            "{}({},{},{})",
            rng.pick(DISPS),
            rng.pick(GP64),
            // No %rsp as index.
            rng.pick(&GP64[..4]),
            rng.pick(SCALES)
        ),
        _ => format!("{}(%rsp)", rng.pick(&[0i64, 8, 16, 64])),
    }
}

fn line(src: &mut String, text: &str) {
    let _ = writeln!(src, "    {}", text);
}

fn int_op(src: &mut String, rng: &mut Rng, seed: u64) {
    let (sfx, regs): (&str, &[&str]) = match rng.bounded(4) {
        0 => ("q", GP64),
        1 => ("l", GP32),
        2 => ("w", GP16),
        _ => ("b", GP8),
    };
    let stem = *rng.pick(&["mov", "add", "sub", "cmp", "and", "or", "xor", "adc", "sbb"]);
    match rng.bounded(5) {
        0 => line(
            src,
            &format!("{}{} {}, {}", stem, sfx, rng.pick(regs), rng.pick(regs)),
        ),
        1 => {
            // Immediate, clamped to the width so gas doesn't warn.
            // Only mov has a 64-bit immediate form; arith stems are
            // capped at imm32 even with the q suffix.
            let imm = *rng.pick(IMM_BOUNDARY);
            let imm = match sfx {
                "b" => imm & 0xff,
                "w" => imm & 0xffff,
                "l" => imm as i32 as i64,
                _ if stem != "mov" => imm as i32 as i64,
                _ => imm,
            };
            // mov to memory can't take a 64-bit immediate.
            if rng.chance(70) {
                line(
                    src,
                    &format!("{}{} ${}, {}", stem, sfx, imm, rng.pick(regs)),
                );
            } else {
                let imm32 = imm as i32;
                line(src, &format!("{}{} ${}, {}", stem, sfx, imm32, mem(rng)));
            }
        }
        2 => line(
            src,
            &format!("{}{} {}, {}", stem, sfx, rng.pick(regs), mem(rng)),
        ),
        3 => line(
            src,
            &format!("{}{} {}, {}", stem, sfx, mem(rng), rng.pick(regs)),
        ),
        _ => match rng.bounded(6) {
            0 => {
                line(
                    src,
                    &format!("test{} {}, {}", sfx, rng.pick(regs), rng.pick(regs)),
                );
                line(src, &format!("set{} {}", rng.pick(CONDS), rng.pick(GP8)));
            }
            1 => {
                let (op, reg): (&str, &str) = if rng.chance(50) {
                    (*rng.pick(&["shlq", "shrq", "sarq"]), *rng.pick(GP64))
                } else {
                    (*rng.pick(&["shll", "shrl", "sarl"]), *rng.pick(GP32))
                };
                if rng.chance(75) {
                    line(src, &format!("{} ${}, {}", op, rng.bounded(31).max(1), reg));
                } else if reg != "%ecx" && reg != "%rcx" {
                    line(src, &format!("{} %cl, {}", op, reg));
                }
            }
            2 => {
                line(
                    src,
                    &format!("movzbl {}, {}", rng.pick(GP8), rng.pick(GP32)),
                );
                line(
                    src,
                    &format!("movslq {}, {}", rng.pick(GP32), rng.pick(GP64)),
                );
            }
            3 => {
                line(src, "cqto");
                line(src, &format!("idivq {}", rng.pick(&GP64[1..])));
            }
            4 => line(
                src,
                &format!("imulq {}, {}", rng.pick(GP64), rng.pick(GP64)),
            ),
            _ => line(src, &format!("leaq {}, {}", mem(rng), rng.pick(GP64))),
        },
    }
    if rng.chance(10) {
        line(
            src,
            &format!("movabsq ${}, {}", rng.pick(IMM_BOUNDARY), rng.pick(GP64)),
        );
    }
    if rng.chance(8) {
        line(src, &format!("leaq lit_{}(%rip), {}", seed, rng.pick(GP64)));
    }
}

fn sse_op(src: &mut String, rng: &mut Rng, seed: u64) {
    match rng.bounded(6) {
        0 => {
            let op = *rng.pick(&["movss", "movsd", "movaps", "movups", "movdqa"]);
            // Keep movaps/movdqa off unaligned stack slots: gas
            // assembles them fine either way, alignment only matters
            // at run time, and we never execute these.
            if rng.chance(50) {
                line(src, &format!("{} {}, {}", op, mem(rng), rng.pick(XMM)));
            } else {
                line(src, &format!("{} {}, {}", op, rng.pick(XMM), mem(rng)));
            }
        }
        1 => {
            let op = *rng.pick(&[
                "addss", "addsd", "subss", "subsd", "mulss", "mulsd", "divss", "divsd", "minsd",
                "maxss", "sqrtss", "sqrtsd", "ucomiss", "ucomisd", "cvtss2sd", "cvtsd2ss",
            ]);
            line(src, &format!("{} {}, {}", op, rng.pick(XMM), rng.pick(XMM)));
        }
        2 => {
            let op = *rng.pick(&[
                "addps", "addpd", "mulps", "mulpd", "minps", "maxpd", "andps", "andpd", "xorps",
                "xorpd", "orps", "orpd", "sqrtps", "sqrtpd", "unpcklps", "unpcklpd",
            ]);
            line(src, &format!("{} {}, {}", op, rng.pick(XMM), rng.pick(XMM)));
        }
        3 => {
            let op = *rng.pick(&[
                "paddb",
                "paddw",
                "paddd",
                "paddq",
                "psubb",
                "psubw",
                "psubd",
                "pand",
                "pandn",
                "por",
                "pcmpgtd",
                "pmullw",
                "pmuludq",
                "punpcklbw",
                "punpcklwd",
                "punpcklqdq",
            ]);
            line(src, &format!("{} {}, {}", op, rng.pick(XMM), rng.pick(XMM)));
        }
        4 => match rng.bounded(6) {
            0 => line(
                src,
                &format!(
                    "pshufd ${}, {}, {}",
                    rng.bounded(256),
                    rng.pick(XMM),
                    rng.pick(XMM)
                ),
            ),
            3 => line(
                src,
                &format!(
                    "pshuflw ${}, {}, {}",
                    rng.bounded(256),
                    rng.pick(XMM),
                    rng.pick(XMM)
                ),
            ),
            4 => line(
                src,
                &format!("psrldq ${}, {}", rng.bounded(17), rng.pick(XMM)),
            ),
            1 => line(
                src,
                &format!(
                    "shufps ${}, {}, {}",
                    rng.bounded(256),
                    rng.pick(XMM),
                    rng.pick(XMM)
                ),
            ),
            2 => line(
                src,
                &format!(
                    "cmpps ${}, {}, {}",
                    rng.bounded(8),
                    rng.pick(XMM),
                    rng.pick(XMM)
                ),
            ),
            _ => {
                line(src, &format!("movd {}, {}", rng.pick(GP32), rng.pick(XMM)));
                line(src, &format!("movq {}, {}", rng.pick(XMM), rng.pick(GP64)));
            }
        },
        _ => match rng.bounded(4) {
            0 => line(
                src,
                &format!("cvtsi2sdq {}, {}", rng.pick(GP64), rng.pick(XMM)),
            ),
            1 => line(
                src,
                &format!("cvtsi2ssl {}, {}", rng.pick(GP32), rng.pick(XMM)),
            ),
            2 => line(
                src,
                &format!("cvttsd2siq {}, {}", rng.pick(XMM), rng.pick(GP64)),
            ),
            _ => line(src, &format!("movsd lit_{}(%rip), {}", seed, rng.pick(XMM))),
        },
    }
}

/// A full program-shaped case: functions with local labels, branches
/// across randomized distances (relaxation stress), int + SSE bodies,
/// RIP references into .data/.rodata, externals, and a data tail.
pub fn generate_supported_case(seed: u64) -> String {
    let mut rng = Rng::new(seed);
    let mut src = String::new();
    let _ = writeln!(src, ".text");
    let nfuncs = 1 + rng.bounded(3);
    for f in 0..nfuncs {
        let name = format!("fuzz_{}_{}", seed, f);
        let _ = writeln!(src, ".globl {}", name);
        let _ = writeln!(src, ".p2align 4");
        let _ = writeln!(src, ".type {}, @function", name);
        let _ = writeln!(src, "{}:", name);
        line(&mut src, "pushq %rbp");
        line(&mut src, "movq %rsp, %rbp");

        let nblocks = 1 + rng.bounded(4);
        for b in 0..nblocks {
            let _ = writeln!(src, ".Lblk_{}_{}_{}:", seed, f, b);
            let nops = rng.bounded(3);
            for _ in 0..nops {
                if rng.chance(60) {
                    int_op(&mut src, &mut rng, seed);
                } else {
                    sse_op(&mut src, &mut rng, seed);
                }
            }
            // Padding shifts branch distances across the rel8 edge.
            for _ in 0..rng.bounded(60) {
                line(&mut src, "nop");
            }
            // Branch somewhere: forward, backward, or fallthrough.
            match rng.bounded(4) {
                0 => {
                    let t = rng.bounded(nblocks);
                    line(&mut src, "cmpq $0, %rax");
                    line(
                        &mut src,
                        &format!("j{} .Lblk_{}_{}_{}", rng.pick(CONDS), seed, f, t),
                    );
                }
                1 if b + 1 < nblocks => {
                    line(&mut src, &format!("jmp .Lblk_{}_{}_{}", seed, f, b + 1));
                }
                2 => line(&mut src, &format!("call ext_fn_{}", rng.bounded(3))),
                _ => {}
            }
        }
        line(&mut src, "movq %rbp, %rsp");
        line(&mut src, "popq %rbp");
        line(&mut src, "ret");
        let _ = writeln!(src, ".size {}, .-{}", name, name);
    }

    // Data tail: RIP targets, symbolic quads, strings, commons.
    let _ = writeln!(src, ".data");
    let _ = writeln!(src, ".p2align 3");
    let _ = writeln!(src, "gdat_{}:", seed);
    let _ = writeln!(src, "    .quad fuzz_{}_0", seed);
    let _ = writeln!(src, "    .quad gdat_{}+8", seed);
    let _ = writeln!(src, "    .long {}", rng.next_u32() as i32);
    let _ = writeln!(src, "    .zero {}", 4 + rng.bounded(12));
    let _ = writeln!(src, ".section .rodata");
    let _ = writeln!(src, ".p2align 3");
    let _ = writeln!(src, "lit_{}:", seed);
    let _ = writeln!(src, "    .quad 0x{:016x}", (rng.next_u32() as u64) << 20);
    let _ = writeln!(src, "    .asciz \"fuzz case {}\"", seed);
    if seed.is_multiple_of(3) {
        let _ = writeln!(src, ".local comm_{}", seed);
        let _ = writeln!(src, ".comm comm_{},{},8", seed, 8 + rng.bounded(64));
    } else if seed.is_multiple_of(2) {
        let _ = writeln!(src, ".comm gcomm_{},16,8", seed);
    }
    let _ = writeln!(src, ".section .note.GNU-stack,\"\",@progbits");
    src
}

/// Token soup that must produce an error, never a panic.
pub fn generate_garbage_case(seed: u64) -> String {
    let mut rng = Rng::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    const FRAGS: &[&str] = &[
        "movq",
        "%rax",
        "%zmm9",
        "$$",
        "(((",
        ")))",
        ".quad",
        ".globl",
        "0x",
        "-",
        ",",
        ",,",
        "%rip",
        "(%rip",
        "jmp",
        "call *",
        ".byte 300",
        ".ascii \"unterminated",
        "lbl:",
        ":",
        "$0x10000000000000000",
        "%r16",
        "mov q",
        ".p2align 99",
        ".comm",
        "@plt",
        "#comment",
        ".size f",
        "je",
        "leaq (%rax,%rsp,3)",
        "\u{7f}",
        "\t\t",
        "%%",
    ];
    let mut src = String::new();
    let n = 1 + rng.bounded(20);
    for _ in 0..n {
        let m = 1 + rng.bounded(5);
        for _ in 0..m {
            src.push_str(rng.pick::<&str>(FRAGS));
            if rng.chance(60) {
                src.push(' ');
            }
        }
        src.push('\n');
    }
    src
}
