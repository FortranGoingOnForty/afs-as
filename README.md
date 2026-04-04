# afs-as

*(noun): a barrel of shifted registers*

Standalone ARM64 assembler for macOS. Reads `.s` assembly text, encodes ARM64 instructions, and emits Mach-O object files linkable with Apple's `ld`.

Part of [ARMFORTAS](https://github.com/FortranGoingOnForty/armfortas), a bespoke ARM64 Fortran compiler.

## Usage

```bash
# Assemble
afs-as hello.s -o hello.o

# Link and run
ld hello.o -o hello -lSystem -syslibroot $(xcrun --show-sdk-path) -e _main
./hello
```

## Library API

```rust
use afs_as::assemble;
use afs_as::encode::Inst;
use afs_as::reg::*;

// From source text
let obj = assemble::assemble_source(".global _main\n_main:\nret\n").unwrap();

// From pre-built instructions (no parsing)
let obj = assemble::assemble_instructions(
    &[Inst::Ret { rn: X30 }],
    &["_main"],
);
```

## Instruction Coverage

- **Data processing**: ADD, SUB, MUL, SDIV, UDIV (register + immediate, 32/64-bit)
- **Logic**: AND, ORR, EOR, ANDS
- **Shifts**: LSL, LSR, ASR (immediate)
- **Moves**: MOV, MOVZ, MOVK, MOVN
- **Compare**: CMP, CMN, TST (aliases)
- **Branches**: B, BL, B.cond (all 16 codes), CBZ, CBNZ, RET, BR, BLR
- **Address**: ADRP (with PAGE/PAGEOFF relocations)
- **Load/Store**: LDR, STR (64/32-bit, unsigned/pre/post-index), LDRB, LDRH, LDRSW, LDP, STP (all addressing modes)
- **FP**: FADD, FSUB, FMUL, FDIV, FNEG, FABS, FSQRT, FCMP, FMADD (single + double)
- **FP conversion**: FCVTZS, SCVTF, FMOV
- **System**: SVC, NOP, BRK

## Tests

365 tests across four levels:

| Suite | Count | What it validates |
|-------|-------|-------------------|
| Unit (encoding) | 217 | Each instruction encodes to the correct 4-byte value (hardcoded ground truth) |
| Round-trip | 88 | Parse text, encode, compare byte-for-byte against Apple `as` |
| System validation | 55 | Shell out to Apple `as`, compare our encoding against theirs |
| End-to-end | 5 | Assemble, link with `ld`, run binary, verify output |

```bash
cargo test -p afs-as
```

## Building

```bash
cargo build -p afs-as          # build
cargo test -p afs-as           # test
cargo clippy -p afs-as         # lint
```

Requires macOS on ARM64 (Apple Silicon) for integration tests that invoke the system assembler and linker.

## License

GPL-3.0
