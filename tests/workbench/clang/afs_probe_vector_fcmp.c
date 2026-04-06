// Workbench note: this probe showed that Apple clang emits a clean floating-
// point compare-mask family after the FP reduction slice, which led directly
// to landing `fcmeq.4s`, `fcmge.4s`, and `fcmgt.4s` together.
#include <arm_neon.h>

uint32x4_t eq_mask_f32(float32x4_t a, float32x4_t b) { return vceqq_f32(a, b); }
uint32x4_t ge_mask_f32(float32x4_t a, float32x4_t b) { return vcgeq_f32(a, b); }
uint32x4_t gt_mask_f32(float32x4_t a, float32x4_t b) { return vcgtq_f32(a, b); }
