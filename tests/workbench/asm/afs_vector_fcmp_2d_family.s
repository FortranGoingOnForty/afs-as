// Workbench note: Apple `as` encoding pin for the float64 SIMD compare-mask family
// emitted by clang from Neon intrinsics.
.text
fcmeq.2d v0, v1, v2
fcmge.2d v3, v4, v5
fcmgt.2d v6, v7, v8
