// Workbench note: Apple `as` encoding pin for scalar-result float64 NaN-aware
// horizontal reductions emitted by clang from Neon intrinsics.
.text
fmaxnmp.2d d0, v0
fminnmp.2d d1, v2
