// Probe result: this exposed the next practical FP/vector arithmetic family
// after compare/reduction/minmax coverage. It led to adding `fmla.4s`,
// `fmls.4s`, and `fneg.4s`, and it was promoted into `tests/clang_probe/vector_fmla.c`.
#include <arm_neon.h>

float32x4_t mla4f(float32x4_t a, float32x4_t b, float32x4_t c) {
    return vfmaq_f32(a, b, c);
}

float32x4_t mls4f(float32x4_t a, float32x4_t b, float32x4_t c) {
    return vfmsq_f32(a, b, c);
}
