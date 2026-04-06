#include <arm_neon.h>

float32x4_t dup_f32(float32x4_t a) {
    return vdupq_laneq_f32(a, 3);
}

float64x2_t dup_f64(float64x2_t a) {
    return vdupq_laneq_f64(a, 1);
}

int16x8_t dup_s16(int16x8_t a) {
    return vdupq_laneq_s16(a, 7);
}

int8x16_t dup_s8(int8x16_t a) {
    return vdupq_laneq_s8(a, 15);
}
