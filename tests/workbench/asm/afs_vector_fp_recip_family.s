// Workbench note: this family was promoted after the Neon reciprocal / rsqrt
// probe showed that clang was emitting a compact SIMD FP estimate/refinement
// neighborhood.
// Reference encodings pinned during promotion:
//   frecpe.4s  v0, v1      -> 0x4EA1D820
//   frecps.4s  v2, v3, v4  -> 0x4E24FC62
//   frsqrte.4s v5, v6      -> 0x6EA1D8C5
//   frsqrts.4s v7, v8, v9  -> 0x4EA9FD07

.text
frecpe.4s v0, v1
frecps.4s v2, v3, v4
frsqrte.4s v5, v6
frsqrts.4s v7, v8, v9
