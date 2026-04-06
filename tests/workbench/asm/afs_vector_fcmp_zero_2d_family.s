// Workbench note: this probe exposed the float64 SIMD compare-with-zero family
// from Apple clang after the `.2d` fused family. The exact encodings are pinned
// here before promotion into tracked coverage.
.text
fcmge.2d v0, v0, #0.0
fcmgt.2d v1, v1, #0.0
fcmle.2d v2, v2, #0.0
fcmlt.2d v3, v3, #0.0
