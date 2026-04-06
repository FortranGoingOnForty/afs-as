// Workbench note: Apple `as` encoding pin for scalar-result float64 horizontal
// reductions emitted by clang from Neon intrinsics.
.text
faddp.2d d0, v0
fmaxp.2d d1, v2
fminp.2d d3, v4
