// Workbench note: Apple `as` encoding pin for the float64 SIMD conversion
// family emitted by clang from Neon intrinsics.
.text
scvtf.2d v0, v0
ucvtf.2d v1, v2
fcvtzs.2d v3, v4
fcvtzu.2d v5, v6
