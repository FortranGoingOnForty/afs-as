# Changelog

All notable changes to `afs-as` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-04-06

Initial release. The standalone surface, library API, and differential test
suite are documented in `README.md`.

### Added

- ARM64 instruction encoding for the supported standalone surface, including
  data processing, loads/stores, branches, FP/SIMD, atomics, system, and CFI.
- Two-pass assembler with label resolution, expression classification, and
  Mach-O relocation emission (`PAGE21`, `PAGEOFF12`, `BRANCH26`, `GOT_LOAD_*`,
  `TLVP_LOAD_*`, `SUBTRACTOR`, `UNSIGNED`, `ADDEND`).
- Mach-O 64 object writer: header, segment, sections, symbol table, dynamic
  symbol table, build version, and linker optimization hints.
- Standalone CLI (`afs-as`) with `--help`, `--version`, `-o`, `-`, `--`, and
  documented exit codes.
- Library API: `assemble_source`, `assemble_stmts`, `assemble_instructions`.
  `assemble_instructions` is a trusted fast path used by the ARMFORTAS
  compiler and panics on invalid `Inst` values by design.
- Diagnostics with file, line, column, source snippet, and caret.
- Layered test infrastructure: instruction-level differential against Apple
  `as`, full-object corpus parity, round-trip stability, structured fuzzing,
  garbage panic-resistance fuzzing, structured malformed-mutation rejection,
  end-to-end link/run, CLI smoke, diagnostic snapshots, performance sanity,
  and a clang probe dashboard covering ~94 C patterns at `-O0` and `-O2`.

[Unreleased]: https://github.com/FortranGoingOnForty/afs-as/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/FortranGoingOnForty/afs-as/releases/tag/v0.1.0
