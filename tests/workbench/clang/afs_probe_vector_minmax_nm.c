// Workbench note: this probe showed that Apple clang emits a clean NaN-aware
// floating-point vector min/max family after the FP compare/reduction slices,
// which led directly to landing `fmaxnm.4s` and `fminnm.4s`.
#include <arm_neon.h>

float32x4_t maxnm4f(float32x4_t a, float32x4_t b) { return vmaxnmq_f32(a, b); }
float32x4_t minnm4f(float32x4_t a, float32x4_t b) { return vminnmq_f32(a, b); }
