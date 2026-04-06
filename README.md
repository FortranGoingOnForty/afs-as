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

The public standalone surface is intentionally small and explicit.

CLI:

- one input file
- default `.o` derivation from the input path
- `--` stops option parsing
- `-` may be used for stdin input or stdout output
- stdin requires explicit `-o <path>` or `-o -`
- usage errors exit `2`; parse or assembly failures exit `1`

Supported section surface:

- `__TEXT,__text`
- `__TEXT,__cstring`
- `__TEXT,__literal16`
- `__TEXT,__const`
- `__DATA,__data`
- `__DATA,__thread_data`
- `__DATA,__thread_vars`
- `__DATA,__thread_bss`
- `__DATA,__bss`

Supported directive families include:

- symbol directives: `.global` / `.globl`, `.extern`, `.private_extern`, `.weak_reference`, `.weak_definition`, `.set`, `.equ`
- data/layout directives: `.byte`, `.short`, `.word`, `.long`, `.quad`, `.ascii`, `.asciz`, `.string`, `.space`, `.skip`, `.zero`, `.fill`, `.align`, `.p2align`, `.comm`, `.zerofill`, `.tbss`
- section-selection directives: `.text`, `.data`, `.cstring`, and `.section` for the supported section set
- metadata directives: `.subsections_via_symbols`, `.build_version` for `macos`
- linker-optimization hints: `.loh AdrpAdd`, `.loh AdrpLdr`, `.loh AdrpLdrGot`, `.loh AdrpLdrGotLdr`
- CFI subset: `.cfi_startproc`, `.cfi_endproc`, `.cfi_def_cfa`, `.cfi_def_cfa_offset`, `.cfi_def_cfa_register`, `.cfi_offset`, `.cfi_restore`, `.cfi_adjust_cfa_offset`

Unsupported forms are expected to fail explicitly rather than assemble silently.

Release gates for a standalone claim:

- `cargo test -p afs-as`
- `cargo clippy -p afs-as --all-targets -- -D warnings`
- green differential, corpus, CLI, diagnostic, dashboard, stress, fuzz, malformed-input, and perf suites in CI

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

`assemble_instructions` is the compiler-facing fast path. It assumes callers build valid
`Inst` values; source-level validation and diagnostics live in `assemble_source`.

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
