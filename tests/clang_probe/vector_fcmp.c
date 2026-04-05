#include <arm_neon.h>

uint32x4_t eq_mask_f32(float32x4_t a, float32x4_t b) { return vceqq_f32(a, b); }
uint32x4_t ge_mask_f32(float32x4_t a, float32x4_t b) { return vcgeq_f32(a, b); }
uint32x4_t gt_mask_f32(float32x4_t a, float32x4_t b) { return vcgtq_f32(a, b); }
