# Probe Workbench

This directory is the in-repo home for ad hoc discovery probes that would
otherwise get scattered under `/tmp`.

Use it for:
- one-off `clang -S` source snippets used to discover what Apple `clang` emits
- tiny `.s` files used to extract exact Apple `as` encodings
- temporary compare/repro artifacts while bringing up a new family

Do not treat this directory as the canonical test corpus.

Promotion rules:
- if a C probe becomes a real tracked regression or dashboard case, move it into
  `tests/clang_probe/`
- if an assembly fixture becomes a real compatibility or writer-parity case,
  move it into `tests/corpus/`
- once promoted, either remove the workbench copy or keep it only if it still
  adds discovery value beyond the promoted regression

This directory is tracked on purpose. It is the portable notebook for SIMD,
relocation, writer-parity, and `clang`-output discovery work across machines.

Some probes here may be incomplete or intentionally rough:
- a source may exist only to see what `clang -S` emits
- an assembly snippet may exist only to pin raw Apple `as` encodings
- not every file here is expected to build as part of the normal test suite

That is fine. The canonical regression surface still lives under
`tests/clang_probe/` and `tests/corpus/`; this workbench just keeps the
exploration trail durable and close to the codebase.
