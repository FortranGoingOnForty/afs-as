// Probe outcome: clang emits the first practical FP reduction family here.
// The Sprint 13 fix added pairwise FP reduction plus scalar FP min/max
// reductions: faddp.4s, faddp.2s, fmaxv.4s, and fminv.4s.
.text
faddp.4s v0, v1, v2
faddp.2s s3, v4
fmaxv.4s s1, v2
fminv.4s s3, v4
