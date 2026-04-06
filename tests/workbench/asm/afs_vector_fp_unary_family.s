// Workbench note: this family was promoted after the SIMD FP unary probe
// showed that clang was emitting `fabs.4s` and `fsqrt.4s`, with `fneg.4s`
// naturally appearing alongside them in the negated-abs path.
// Reference encodings pinned during promotion:
//   fabs.4s v0, v0   -> 0x4EA0F800
//   fsqrt.4s v1, v2  -> 0x6EA1F841
//   fneg.4s v6, v7   -> 0x6EA0F8E6

.text
fabs.4s v0, v0
fsqrt.4s v1, v2
fneg.4s v6, v7
