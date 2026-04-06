// Probe outcome: clang emits the NaN-aware FP vector min/max family here.
// The Sprint 13 fix added the practical floating-point min/max neighbor
// slice: fmaxnm.4s and fminnm.4s.
.text
fmaxnm.4s v0, v1, v2
fminnm.4s v3, v4, v5
