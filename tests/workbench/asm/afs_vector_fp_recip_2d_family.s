// Workbench note: Apple `as` encoding pin for the float64 SIMD reciprocal and
// reciprocal-square-root estimate/refinement family emitted by clang.
.text
frecpe.2d v0, v0
frecps.2d v1, v2, v3
frsqrte.2d v4, v5
frsqrts.2d v6, v7, v8
