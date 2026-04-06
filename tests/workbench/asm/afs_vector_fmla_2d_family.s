// Workbench note: this probe exposed the float64 SIMD fused family from Apple
// clang after the `.2d` arithmetic and min/max families. It led to adding
// `fmla.2d` and `fmls.2d`, and the exact encodings are pinned below:
//   fmla.2d v0, v1, v2 -> 0x4E62CC20
//   fmls.2d v3, v4, v5 -> 0x4EE5CC83
.text
fmla.2d v0, v1, v2
fmls.2d v3, v4, v5
