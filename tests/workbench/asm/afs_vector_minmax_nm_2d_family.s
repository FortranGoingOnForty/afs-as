// Workbench note: Apple `as` encoding pin for the float64 NaN-aware SIMD
// min/max family emitted by clang from Neon intrinsics.
.text
fmaxnm.2d v0, v0, v1
fminnm.2d v2, v3, v4
