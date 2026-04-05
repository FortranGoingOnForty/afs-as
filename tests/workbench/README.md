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
- once promoted, remove or ignore the workbench copy

This directory is intentionally gitignored except for this README, so we can
keep useful scratch material in the repo worktree without polluting history.
