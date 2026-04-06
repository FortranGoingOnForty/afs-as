// Workbench note: this probe pinned the float64 SIMD absolute-difference
// sibling emitted by Apple clang after the `.2d` arithmetic and fused families.
// It led to adding `fabd.2d` and promoting `tests/clang_probe/vector_fabd_2d.c`.
//   fabd.2d v0, v1, v2 -> 0x6EE2D420

.text
fabd.2d v0, v1, v2
