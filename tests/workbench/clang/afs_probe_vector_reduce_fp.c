// Probe: find the first practical floating-point vector reduction family that
// clang emits after the integer reduction slices, so we can promote the whole
// family into Sprint 13 instead of adding isolated SIMD mnemonics.
#include <arm_neon.h>

float add4f_reduce(float32x4_t v) {
    return vaddvq_f32(v);
}

float max4f_reduce(float32x4_t v) {
    return vmaxvq_f32(v);
}

float min4f_reduce(float32x4_t v) {
    return vminvq_f32(v);
}
