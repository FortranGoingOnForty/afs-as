// Probe outcome: clang emits the NaN-aware FP reduction family here.
// The Sprint 13 fix added the practical floating-point reduction neighbors
// `fmaxnmv.4s` and `fminnmv.4s`.
.text
fmaxnmv.4s s1, v2
fminnmv.4s s3, v4
