/* Workbench note: probe vector min/max families after compare coverage, using intrinsics so clang emits a stable SIMD surface at both O0 and O2. */
#include <arm_neon.h>

float32x4_t max4f(float32x4_t a, float32x4_t b) { return vmaxq_f32(a, b); }
float32x4_t min4f(float32x4_t a, float32x4_t b) { return vminq_f32(a, b); }
int32x4_t max4s(int32x4_t a, int32x4_t b) { return vmaxq_s32(a, b); }
int32x4_t min4s(int32x4_t a, int32x4_t b) { return vminq_s32(a, b); }
