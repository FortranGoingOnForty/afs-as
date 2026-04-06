// Workbench note: this family was promoted after the fused FP/vector probe
// showed that clang was emitting `fmla.4s` and `fmls.4s`, and that the
// unoptimized subtract path also needed `fneg.4s`.
// Reference encodings pinned during promotion:
//   fmla.4s v0, v1, v2 -> 0x4E22CC20
//   fmls.4s v3, v4, v5 -> 0x4EA5CC83
//   fneg.4s v6, v7     -> 0x6EA0F8E6

.text
fmla.4s v0, v1, v2
fmls.4s v3, v4, v5
fneg.4s v6, v7
