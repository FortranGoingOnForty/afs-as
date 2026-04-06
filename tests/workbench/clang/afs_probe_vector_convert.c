// Probe result: this exposed the next compact SIMD conversion neighborhood
// after the unary FP slice. It led to adding `scvtf.4s`, `ucvtf.4s`,
// `fcvtzs.4s`, and `fcvtzu.4s`, promoted into `tests/clang_probe/vector_fp_convert.c`.
#include <arm_neon.h>

float32x4_t to_float_s32(int32x4_t a) { return vcvtq_f32_s32(a); }
float32x4_t to_float_u32(uint32x4_t a) { return vcvtq_f32_u32(a); }
int32x4_t to_int_s32(float32x4_t a) { return vcvtq_s32_f32(a); }
uint32x4_t to_uint_u32(float32x4_t a) { return vcvtq_u32_f32(a); }
