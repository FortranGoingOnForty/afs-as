// Workbench note: Apple `as` encodings for the 64-bit-lane pairwise family
// exposed by the `vpaddq_u64` / `vpmaxq_f64` / `vpminq_f64` clang probe.
addp.2d v0, v1, v2
fmaxp.2d v3, v4, v5
fminp.2d v6, v7, v8
