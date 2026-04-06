// Workbench note: Apple `as` encoding pin for the float64 SIMD FP unary family
// emitted by clang from Neon intrinsics.
.text
fabs.2d v0, v0
fsqrt.2d v1, v2
fneg.2d v3, v4
