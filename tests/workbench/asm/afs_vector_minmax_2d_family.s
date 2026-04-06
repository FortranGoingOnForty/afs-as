// Workbench note: Apple `as` encoding pin for the float64 SIMD min/max family
// emitted by clang from Neon intrinsics after the `.2d` compare-mask work.
.text
fmax.2d v0, v0, v1
fmin.2d v2, v3, v4
