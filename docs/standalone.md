# afs-as Standalone Surface

`afs-as` is intended to be usable as a real standalone assembler on Apple Silicon macOS, not only as ARMFORTAS's backend object emitter.

This document describes the support we intentionally rely on today.
The release bar for calling that support standalone-ready lives in [release-readiness.md](release-readiness.md).

## Target

- object format: Mach-O relocatable objects
- architecture: ARM64 / AArch64
- platform: macOS on Apple Silicon
- linker expectation: Apple's `ld`

## CLI Surface

Supported CLI behavior:

- `afs-as input.s -o output.o`
- `afs-as input.s`
- `afs-as - -o output.o`
- `afs-as - -o -`
- `afs-as --help`
- `afs-as --version`

Behavior:

- a single input file is accepted
- default output path replaces the input extension with `.o`
- `--` stops option parsing so dash-prefixed input filenames can still be assembled
- `-` can be used as stdin input or stdout output
- stdin input requires explicit `-o <output.o>` or `-o -`
- usage errors exit `2`
- parse / assembly failures exit `1`
- diagnostics include file, line, column, source line, and caret when source text is available

### Exit Status Policy

- `0`: success
- `1`: parse or assembly failure
- `2`: command-line usage error

Library entry points remain available through [`assemble::assemble_source`](../src/assemble.rs) and [`assemble::assemble_instructions`](../src/assemble.rs).

## Supported Source Forms

### Labels and Symbols

- global labels such as `_main:`
- local labels such as `.Ltmp0:`
- uppercase-`L` temporary labels emitted by compiler output such as `LBB0_2:`
- numeric local labels such as `1:` with `1f` / `1b` references
- dotted symbol names such as `l_.str`
- symbol visibility directives:
  - `.global` / `.globl`
  - `.extern`
  - `.private_extern`
  - `.weak_reference`
  - `.weak_definition`
- absolute symbol assignments:
  - `.set`
  - `.equ`

### Sections and Data Directives

Supported section directives:

- `.text`
- `.data`
- `.cstring`
- `.section __TEXT,__text`
- `.section __TEXT,__cstring`
- `.section __TEXT,__const`
- `.section __DATA,__data`
- `.section __DATA,__thread_data`
- `.section __DATA,__thread_vars`
- `.section __DATA,__thread_bss`
- `.section __DATA,__bss`

Supported data / layout directives:

- `.byte`
- `.short`
- `.word` / `.long`
- `.quad`
- `.ascii`
- `.asciz` / `.string`
- `.space` / `.skip`
- `.zero`
- `.fill`
- `.align`
- `.p2align`
- `.comm`
- `.zerofill`
- `.tbss`

### Metadata and Unwind Directives

Supported metadata directives:

- `.subsections_via_symbols`
- `.build_version` for `macos`
- `.loh` for the linker-optimization hint forms currently emitted by `clang`

Supported CFI subset:

- `.cfi_startproc`
- `.cfi_endproc`
- `.cfi_def_cfa`
- `.cfi_def_cfa_offset`
- `.cfi_def_cfa_register`
- `.cfi_offset`
- `.cfi_restore`
- `.cfi_adjust_cfa_offset`

These feed compact unwind emission where possible and DWARF fallback when required.

### Expressions and Relocations

Supported symbolic expression / relocation surface includes:

- `symbol@PAGE` / `symbol@PAGEOFF`
- `symbol@GOTPAGE` / `symbol@GOTPAGEOFF`
- `symbol@TLVPPAGE` / `symbol@TLVPPAGEOFF`
- `symbol@GOT`
- relocation addends where the Mach-O relocation model supports them
- `.quad foo`
- `.quad foo - bar + constant`
- `.long foo@GOT - .`

Assembler-resolved local-label forms are supported for:

- `b`
- `bl`
- `b.cond`
- `cbz`
- `cbnz`
- `tbz`
- `tbnz`
- `adr`
- literal `ldr`
- literal `ldrsw`

## Instruction Families

The supported instruction surface is driven by real compiler output plus standalone compatibility coverage.

Major covered families include:

- integer arithmetic and aliases:
  - `add`, `sub`, `adds`, `subs`
  - `cmp`, `cmn`, `neg`
  - shifted and extended register forms
- logic / move:
  - `and`, `orr`, `eor`, `ands`
  - `mov`, `movz`, `movk`, `movn`, `mvn`
- conditional select family:
  - `csel`, `csinc`, `csinv`, `csneg`
  - `cset`, `csetm`, `cinc`, `cinv`, `cneg`
  - `ccmp`, `ccmn`
- branch and control flow:
  - `b`, `bl`, `b.cond`
  - `cbz`, `cbnz`, `tbz`, `tbnz`
  - `ret`, `br`, `blr`
- addressing and literal forms:
  - `adr`, `adrp`
  - literal `ldr` / `ldrsw`
- load/store:
  - `ldr`, `str`
  - `ldrb`, `ldrsb`, `ldrh`, `ldrsh`, `ldrsw`
  - `strb`, `strh`
  - `ldp`, `stp`
  - integer, FP scalar, and `q` vector forms across the supported addressing modes
  - FP/SIMD pair forms across the supported addressing modes
- scalar atomic memory:
  - `ldaprb`, `ldaprh`, `ldapr`
  - `stlrb`, `stlrh`, `stlr`
  - `ldaddalb`, `ldaddalh`, `ldaddal`
  - `ldclralb`, `ldclralh`, `ldclral`
  - `ldeoralb`, `ldeoralh`, `ldeoral`
  - `ldsetalb`, `ldsetalh`, `ldsetal`
  - `ldumaxalb`, `ldumaxalh`, `ldumaxal`
  - `ldsmaxalb`, `ldsmaxalh`, `ldsmaxal`
  - `lduminalb`, `lduminalh`, `lduminal`
  - `ldsminalb`, `ldsminalh`, `ldsminal`
  - `swpalb`, `swpalh`, `swpal`
  - `casalb`, `casalh`, `casal`
- floating point:
  - `fadd`, `fsub`, `fmul`, `fdiv`
  - `fabs`, `fneg`, `fsqrt`, `fcmp`, `fmadd`
  - `fcvtzs`, `scvtf`, `fmov`
- SIMD / vector:
  - `add.4s`, `sub.4s`, `smax.4s`, `smin.4s`, `umax.4s`, `umin.4s`
  - `addp.16b`, `addp.8h`, `addp.4s`, `addp.2d`, `smaxp.4s`, `sminp.4s`, `umaxp.4s`, `uminp.4s`
  - `smaxp.8h`, `sminp.8h`, `umaxp.8h`, `uminp.8h`
  - `smaxp.16b`, `sminp.16b`, `umaxp.16b`, `uminp.16b`
  - `addv.4s`, `smaxv.4s`, `umaxv.4s`
  - `sminv.4s`, `uminv.4s`
  - `faddp.4s`, `faddp.2s`, `faddp.2d`, `fmaxp.2d`, `fminp.2d`, `fmaxnmp.2d`, `fminnmp.2d`, `fmaxv.4s`, `fminv.4s`
  - `fmaxp.4s`, `fminp.4s`
  - `fmaxnmp.4s`, `fminnmp.4s`
  - `fmaxnmv.4s`, `fminnmv.4s`
  - `fmla.4s`, `fmls.4s`, `fneg.4s`
  - `fabs.4s`, `fsqrt.4s`
  - `scvtf.4s`, `ucvtf.4s`, `fcvtzs.4s`, `fcvtzu.4s`
  - `frecpe.4s`, `frecps.4s`, `frsqrte.4s`, `frsqrts.4s`
  - `frintn.4s`, `frintm.4s`, `frintp.4s`, `frintz.4s`
  - `frinta.4s`, `frinti.4s`
  - `and.16b`, `bic.16b`, `orr.16b`, `eor.16b`
  - `bif.16b`, `bit.16b`, `bsl.16b`
  - `cmeq.4s`, `cmhs.4s`, `cmhi.4s`, `cmge.4s`, `cmgt.4s`
  - `fcmeq.4s`, `fcmge.4s`, `fcmgt.4s`
  - `ext.16b`, `rev64.4s`, `zip1.4s`, `zip2.4s`, `uzp1.4s`, `uzp2.4s`, `trn1.4s`, `trn2.4s`
  - `tbl.16b`, `tbx.16b`
  - `fadd.4s`, `fsub.4s`, `fmul.4s`, `fdiv.4s`, `fmax.4s`, `fmin.4s`
  - `fmaxnm.4s`, `fminnm.4s`
  - `mov.8b`, `mov.16b`, `mov.4s`, `mov.2d`
  - `dup.16b`, `dup.8h`, `dup.4s`, `dup.2d`
  - `mov sN, vM[idx]`, `mov dN, vM[idx]`
  - `mov.s wN, vM[idx]`, `mov.d xN, vM[idx]`, `umov.h wN, vM[idx]`, `umov.b wN, vM[idx]`
  - `mov.s vN[idx], vM[idx]`, `mov.d vN[idx], vM[idx]`, `mov.h vN[idx], vM[idx]`, `mov.b vN[idx], vM[idx]`
  - `mov.s vN[idx], wM`, `mov.d vN[idx], xM`, `mov.h vN[idx], wM`, `mov.b vN[idx], wM`
- system / hint:
  - `svc`, `brk`, `nop`
  - `yield`, `wfe`, `wfi`, `sev`, `sevl`
  - `isb`, `dmb`, `dsb`

## Explicitly Unsupported Today

Unsupported cases should fail explicitly rather than silently assembling to the wrong thing.

Current examples:

- unknown directives
- unsupported `.cfi_*` directives outside the modeled subset
- unsupported section names outside the small Mach-O section set listed above
- `.build_version` platforms other than `macos`
- operand classes that require assembler-local labels when given externals instead
- instruction mnemonics or addressing forms that have not been implemented yet
- non-Mach-O / non-macOS targets
- multi-input assembly jobs

If a source file currently assembles only because a directive is silently ignored, that is considered a bug in `afs-as`.

## Testing Strategy

Standalone support is only considered landed when it is covered in layers:

1. unit tests for parser, encoder, expression, or Mach-O writer behavior
2. Apple `as` differential tests when there is a system reference
3. raw object parity when output shape matters
4. linker or runtime tests when object acceptance matters
5. CLI smoke tests when user-facing diagnostics or exit behavior changes

## Testing Opportunities

Whenever the standalone surface expands, good follow-up coverage usually includes:

- one parser regression for the accepted syntax
- one encoder or writer regression for the emitted object details
- one corpus fixture that matches Apple `as`
- one failure-path test if the feature has unsupported sibling forms
- one CLI smoke case if the change affects diagnostics or tool behavior
