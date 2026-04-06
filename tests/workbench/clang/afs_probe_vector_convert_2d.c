/* Workbench note: probe the float64 SIMD conversion neighborhood after landing
 * the `.4s` family, to see whether clang emits `scvtf.2d`, `ucvtf.2d`,
 * `fcvtzs.2d`, and `fcvtzu.2d` as the next practical sibling set.
 */
#include <arm_neon.h>

float64x2_t to_float_s64(int64x2_t a) { return vcvtq_f64_s64(a); }
float64x2_t to_float_u64(uint64x2_t a) { return vcvtq_f64_u64(a); }
int64x2_t to_int_s64(float64x2_t a) { return vcvtq_s64_f64(a); }
uint64x2_t to_uint_u64(float64x2_t a) { return vcvtq_u64_f64(a); }
