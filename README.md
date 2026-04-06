# afs-as

*(noun): a barrel of shifted registers*

Standalone ARM64 assembler for macOS. Reads `.s` assembly text, encodes ARM64 instructions, and emits Mach-O object files linkable with Apple's `ld`.

Part of [ARMFORTAS](https://github.com/FortranGoingOnForty/armfortas), a bespoke ARM64 Fortran compiler.

## Usage

```bash
# Assemble
afs-as hello.s -o hello.o

# Or let afs-as derive hello.o automatically
afs-as hello.s

# Assemble from stdin to stdout
cat hello.s | afs-as - -o - > hello.o

# Inspect CLI help
afs-as --help

# Link and run
ld hello.o -o hello -lSystem -syslibroot $(xcrun --show-sdk-path) -e _main
./hello
```

CLI behavior is intentionally small and explicit:

- `--help` and `--version` print to stdout and exit `0`
- usage errors exit `2`
- parse / assembly failures exit `1` with file, line, column, source line, and caret diagnostics
- `--` stops option parsing
- `-` can be used for stdin input or stdout output
- stdin input requires explicit `-o <output.o>` or `-o -`

## Standalone Support Matrix

The tracked support matrix lives in [docs/standalone.md](docs/standalone.md).
The standalone release checklist lives in [docs/release-readiness.md](docs/release-readiness.md).

That document covers:

- supported labels, expressions, relocations, directives, and instruction families
- known unsupported features that currently fail explicitly
- library API vs CLI usage
- testing strategy for expanding the standalone surface safely

The release-readiness checklist covers:

- the required CI and local gates for a standalone claim
- the hard failure conditions that block that claim
- testing opportunities when the release bar changes

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

## Tests

`afs-as` is validated through layered coverage rather than a single golden path:

- unit tests for parsing, encoding, expression classification, Mach-O writing, and diagnostics
- differential tests against Apple `as`
- raw-object parity corpus tests
- linker / runtime end-to-end tests
- CLI smoke tests for user-facing behavior

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
