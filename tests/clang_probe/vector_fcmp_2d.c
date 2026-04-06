#include <arm_neon.h>

uint64x2_t eq_mask_f64(float64x2_t a, float64x2_t b) { return vceqq_f64(a, b); }
uint64x2_t ge_mask_f64(float64x2_t a, float64x2_t b) { return vcgeq_f64(a, b); }
uint64x2_t gt_mask_f64(float64x2_t a, float64x2_t b) { return vcgtq_f64(a, b); }
