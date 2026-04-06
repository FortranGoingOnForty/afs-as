// Workbench note: Apple `as` encoding pin for the plain float64 SIMD
// arithmetic family emitted by clang from Neon intrinsics.
.text
fadd.2d v0, v1, v2
fsub.2d v3, v4, v5
fmul.2d v6, v7, v8
fdiv.2d v9, v10, v11
