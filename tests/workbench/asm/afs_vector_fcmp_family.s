// Probe outcome: clang emits a clean FP vector compare-mask family here.
// The Sprint 13 fix added the first practical floating-point SIMD compare
// neighborhood: fcmeq.4s, fcmge.4s, and fcmgt.4s.
.text
fcmeq.4s v0, v1, v2
fcmge.4s v3, v4, v5
fcmgt.4s v6, v7, v8
