/*
 * Workbench note: this probe exposed the float64 SIMD absolute-difference
 * sibling `fabd.2d` after the `.2d` arithmetic and fused families were added,
 * and it led to promoting `tests/clang_probe/vector_fabd_2d.c`.
 */
#include <arm_neon.h>

float64x2_t absdiff2d(float64x2_t a, float64x2_t b) { return vabdq_f64(a, b); }
