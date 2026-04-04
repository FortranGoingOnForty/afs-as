# afs-as Release Readiness Checklist

`afs-as` is ready to be presented as a standalone assembler only when its documented surface, its enforced CI gates, and its known limitations all line up.

This checklist is the release bar for that claim.

## Standalone-Ready Criteria

`afs-as` is standalone-ready when all of the following are true:

- the supported source and CLI surface in [`standalone.md`](standalone.md) is accurate
- unsupported directives, operands, sections, or relocations fail explicitly instead of assembling silently
- no known silent-wrong-code issues remain inside the documented supported surface
- macOS ARM64 CI is green across build, lint, compatibility, hardening, and CLI jobs
- differential coverage against Apple `as` remains green for the tracked corpus
- linker/runtime smoke coverage remains green for representative objects

## Required Release Gates

These commands are the minimum release gate for a standalone claim:

### Build and Lint

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`

### Core Assembler Correctness

- `cargo test --lib`
- `cargo test --test roundtrip`
- `cargo test --test verify_against_system_as`
- `cargo test --test corpus_compat`
- `cargo test --test hello_world`

### User-Facing Tooling

- `cargo test --test cli_smoke`
- `cargo test --test diagnostic_snapshots`

### Hardening and Regression Defense

- `cargo test --test clang_probe_dashboard`
- `cargo test --test generated_stress`
- `cargo test --test differential_fuzz`
- `cargo test --test malformed_mutation`
- `cargo test --test perf_sanity`

The `afs-as` and `armfortas` CI workflows both run these gates as separate jobs so failures stay localized.

## Release Checklist

Before calling a revision standalone-ready:

- confirm [`standalone.md`](standalone.md) still matches the accepted directives, relocations, sections, and instruction families
- document any newly unsupported-but-explicitly-rejected surface instead of leaving it implicit
- add at least one permanent regression test for every bug fixed during the release window
- add or refresh at least one real compiler-emitted corpus case if the supported instruction or relocation surface grew
- verify the hardening jobs are green, not just the happy-path corpus jobs
- verify the CLI help and diagnostics still match the documented behavior
- verify the current branch has no known silent-wrong-code issue left open for supported input

## Do Not Claim Standalone Readiness If

Do not publish or describe `afs-as` as standalone-ready if any of these are true:

- a supported source form only works because a directive is ignored
- an unsupported form is accepted and encoded as something else
- a differential mismatch is understood but intentionally left unresolved inside supported surface
- the release notes depend on undocumented caveats that are not captured in [`standalone.md`](standalone.md)
- CI is green only because hardening suites were skipped or disabled

## Testing Opportunities

When this checklist changes, useful follow-up coverage usually includes:

- a CI job update so the new gate is enforced automatically
- a CLI or diagnostic regression if the checklist changes user-visible behavior
- a corpus or dashboard addition if the checklist expands the supported surface
- a stress, fuzz, or malformed-input regression if the checklist was updated because of a hardening issue
